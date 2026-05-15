//! Session identity sourcing — shared helpers for session-scoped verbs.
//!
//! Sourcing precedence (per spec 2026-05-15-agent-learning-loop ac-07):
//!   1. `ORBIT_SESSION_ID` environment variable, if set and non-empty.
//!   2. `.orbit/.session-id` file (single-line), if present and non-empty.
//!   3. Otherwise `Error::unavailable` naming both sources.
//!
//! The Stop hook owns deletion of `.orbit/.session-id`; verbs in this module
//! only read.

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
