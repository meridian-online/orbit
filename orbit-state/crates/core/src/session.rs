//! Session identity sourcing — shared helpers for session-scoped verbs.
//!
//! Sourcing precedence (per spec 2026-05-15-agent-learning-loop ac-07):
//!   1. `ORBIT_SESSION_ID` environment variable, if set and non-empty.
//!   2. `.orbit/.session-id` file (single-line), if present and non-empty.
//!   3. Otherwise `Error::unavailable` naming both sources.
//!
//! The Stop hook owns deletion of `.orbit/.session-id`; verbs in this module
//! only read.
//!
//! Per spec 2026-05-16-session-handover ac-03, `read_session_card` is the
//! sibling fallback for the optional card slug — present as a single-line
//! file at `.orbit/.session-card` only when the operator has run
//! `orbit session set-card <id>`. Absence is normal and returns `Ok(None)`,
//! not an error.

use std::fs;

use crate::error::{Error, Result};
use crate::layout::OrbitLayout;

const ENV_VAR: &str = "ORBIT_SESSION_ID";

/// Resolve the current session id using the documented precedence.
///
/// `verb` is the name of the calling verb so any error surfaces under the
/// caller's namespace (per ac-05 error format).
pub fn read_session_id(layout: &OrbitLayout, verb: &str) -> Result<String> {
    if let Ok(value) = std::env::var(ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let path = layout.session_id_file();
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let trimmed = contents.trim();
            if trimmed.is_empty() {
                Err(Error::unavailable(
                    verb,
                    format!(
                        "no session id available: set {ENV_VAR} or write {}",
                        path.display()
                    ),
                ))
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::unavailable(
            verb,
            format!(
                "no session id available: set {ENV_VAR} or write {}",
                path.display()
            ),
        )),
        Err(e) => Err(Error::unavailable(
            verb,
            format!("read {}: {e}", path.display()),
        )),
    }
}

/// Extract the agent's curated reflection from raw stdin bytes for
/// `orbit session distill`. Per spec 2026-05-16-session-handover ac-05:
///
/// - Lossy-utf8-convert the bytes so non-UTF-8 input never panics; the
///   U+FFFD replacement character lands in the distillate and the
///   operator sees the corruption rather than a verb failure.
/// - Attempt `serde_json::from_str` on the resulting string. If parsing
///   succeeds AND the value is a JSON object containing string field
///   `hook_event_name` with value `"Stop"`, extract the string field
///   `last_assistant_message` and return that as the distillate body.
/// - Otherwise (parse fails, value is not an object, missing
///   `hook_event_name`, value isn't `"Stop"`, or `last_assistant_message`
///   is missing or not a string), return the lossy-utf8 string verbatim
///   — preserving today's plain-text-stdin behaviour.
///
/// The extraction is non-fatal by design — the verb never refuses a
/// distill on stdin shape grounds. This is the load-bearing fix for the
/// previous behaviour where Stop-hook-piped JSON landed verbatim in
/// `Session.distillate`. The Stop-hook payload shape is documented at
/// https://docs.claude.com/en/docs/claude-code/hooks (Stop event).
pub fn extract_distillate_from_stdin(buf: &[u8]) -> String {
    let s = String::from_utf8_lossy(buf).into_owned();
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&s) {
        let is_stop = map
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .map(|name| name == "Stop")
            .unwrap_or(false);
        if is_stop {
            if let Some(msg) = map.get("last_assistant_message").and_then(|v| v.as_str()) {
                return msg.to_string();
            }
        }
    }
    s
}

/// Read the optional card slug from `.orbit/.session-card`. Returns
/// `Ok(None)` when the file is missing or empty — that's the normal
/// shape for a session where the operator hasn't called
/// `orbit session set-card`. Only an unexpected IO error surfaces.
///
/// `verb` is the calling verb name for any error namespacing.
pub fn read_session_card(layout: &OrbitLayout, verb: &str) -> Result<Option<String>> {
    let path = layout.session_card_file();
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let trimmed = contents.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::unavailable(
            verb,
            format!("read {}: {e}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // std::env::set_var / remove_var are process-global; tests in this module
    // must serialise on this mutex to avoid races with other env-touching tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env_unset<F: FnOnce()>(f: F) {
        let _g = ENV_LOCK.lock().unwrap();
        let prior = std::env::var(ENV_VAR).ok();
        std::env::remove_var(ENV_VAR);
        f();
        if let Some(v) = prior {
            std::env::set_var(ENV_VAR, v);
        }
    }

    fn with_env_set<F: FnOnce()>(value: &str, f: F) {
        let _g = ENV_LOCK.lock().unwrap();
        let prior = std::env::var(ENV_VAR).ok();
        std::env::set_var(ENV_VAR, value);
        f();
        match prior {
            Some(v) => std::env::set_var(ENV_VAR, v),
            None => std::env::remove_var(ENV_VAR),
        }
    }

    #[test]
    fn read_session_id_prefers_env_var() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        // File and env disagree — env must win.
        fs::write(layout.session_id_file(), "from-file\n").unwrap();
        with_env_set("from-env", || {
            let id = read_session_id(&layout, "test.verb").unwrap();
            assert_eq!(id, "from-env");
        });
    }

    #[test]
    fn read_session_id_falls_back_to_file() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        fs::write(layout.session_id_file(), "abc-123\n").unwrap();
        with_env_unset(|| {
            let id = read_session_id(&layout, "test.verb").unwrap();
            assert_eq!(id, "abc-123");
        });
    }

    #[test]
    fn read_session_id_unavailable_when_neither() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        with_env_unset(|| {
            let err = read_session_id(&layout, "test.verb").unwrap_err();
            assert_eq!(err.category, crate::error::Category::Unavailable);
            assert_eq!(err.verb, "test.verb");
            assert!(err.message.contains("ORBIT_SESSION_ID"));
            assert!(err.message.contains(".session-id"));
        });
    }

    // ------------------------------------------------------------------------
    // spec 2026-05-16-session-handover ac-05: stdin distillate extraction
    // ------------------------------------------------------------------------

    #[test]
    fn extract_distillate_extracts_last_assistant_message_from_stop_envelope() {
        let json = r#"{
            "hook_event_name": "Stop",
            "session_id": "abc",
            "transcript_path": "/tmp/x.jsonl",
            "last_assistant_message": "the agent's curated prose"
        }"#;
        let got = extract_distillate_from_stdin(json.as_bytes());
        assert_eq!(got, "the agent's curated prose");
    }

    #[test]
    fn extract_distillate_falls_through_on_non_stop_event() {
        let json = r#"{
            "hook_event_name": "SessionStart",
            "last_assistant_message": "should not be picked up"
        }"#;
        let got = extract_distillate_from_stdin(json.as_bytes());
        assert!(got.contains("SessionStart"), "expected raw JSON verbatim: {got}");
    }

    #[test]
    fn extract_distillate_falls_through_on_missing_event_name() {
        let json = r#"{
            "last_assistant_message": "no event name"
        }"#;
        let got = extract_distillate_from_stdin(json.as_bytes());
        assert!(got.contains("no event name"));
        assert!(got.starts_with("{"));
    }

    #[test]
    fn extract_distillate_falls_through_on_stop_without_message() {
        // ac-05 defensive clause: missing last_assistant_message falls
        // through to plain-text verbatim.
        let json = r#"{
            "hook_event_name": "Stop",
            "session_id": "abc"
        }"#;
        let got = extract_distillate_from_stdin(json.as_bytes());
        assert!(got.contains("\"hook_event_name\""));
    }

    #[test]
    fn extract_distillate_returns_plain_text_verbatim() {
        let plain = "just markdown\n- a list item\n- another";
        let got = extract_distillate_from_stdin(plain.as_bytes());
        assert_eq!(got, plain);
    }

    #[test]
    fn extract_distillate_handles_invalid_utf8_with_replacement_chars() {
        // ac-05: non-UTF-8 input does NOT panic; U+FFFD characters appear
        // and the operator sees the corruption in the distillate.
        let bytes = b"hello \xFF\xFE world";
        let got = extract_distillate_from_stdin(bytes);
        assert!(got.contains("hello"));
        assert!(got.contains("world"));
        assert!(got.contains('\u{FFFD}'));
    }

    #[test]
    fn extract_distillate_falls_through_on_message_wrong_type() {
        let json = r#"{
            "hook_event_name": "Stop",
            "last_assistant_message": ["not", "a", "string"]
        }"#;
        let got = extract_distillate_from_stdin(json.as_bytes());
        // The whole JSON object is returned verbatim.
        assert!(got.starts_with("{"));
        assert!(got.contains("hook_event_name"));
    }

    #[test]
    fn read_session_id_treats_blank_env_as_unset() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        fs::write(layout.session_id_file(), "file-id\n").unwrap();
        with_env_set("   ", || {
            let id = read_session_id(&layout, "test.verb").unwrap();
            assert_eq!(id, "file-id");
        });
    }
}
