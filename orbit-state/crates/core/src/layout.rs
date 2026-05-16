//! `.orbit/` directory layout.
//!
//! Single source of truth for where each entity type lives on disk. Paths are
//! relative to a root that the caller supplies (typically the repo root).
//!
//! Layout (per card 0008 + ac-20, choice 0021). See
//! `.orbit/conventions/spec-layout.md` for the canonical sidecar inventory;
//! the rule below is its mechanical enforcement.
//!
//! ```text
//! .orbit/
//!   schema-version             (substrate-written, gitignored)
//!   state.db                   (derived index, gitignored)
//!   locks/                     (lock files, gitignored)
//!   specs/<id>/spec.yaml                   (substrate-written, tracked) — primary spec
//!   specs/<id>/tasks.jsonl                 (append-only events, tracked)
//!   specs/<id>/notes.jsonl                 (append-only notes, tracked)
//!   specs/<id>/drive.yaml                  (drive sidecar, tracked)
//!   specs/<id>/rally.yaml                  (rally sidecar, tracked)
//!   specs/<id>/review-spec-<date>.md       (review artefact, tracked)
//!   specs/<id>/review-pr-<date>.md         (review artefact, tracked)
//!   specs/<id>/interview.md                (interview sidecar, tracked)
//!   cards/<slug>.yaml          (human-written, tracked)
//!   memos/<date>-<slug>.md     (memos awaiting distillation, tracked)
//!   choices/<slug>.yaml        (human-written, tracked)
//!   memories/<slug>.yaml       (substrate-written, tracked)
//! ```
//!
//! Specs live in per-id folders: `list_spec_files()` scans immediate
//! subdirectories of `specs/` and returns every `<id>/spec.yaml` it finds.
//! Sidecars live alongside `spec.yaml` inside the same folder and are never
//! loaded as primary specs.

use std::path::{Path, PathBuf};

/// Resolve all canonical subpaths of an `.orbit/` root.
#[derive(Debug, Clone)]
pub struct OrbitLayout {
    pub root: PathBuf,
}

impl OrbitLayout {
    /// Construct a layout rooted at `<repo>/.orbit/`.
    pub fn at(repo_root: impl AsRef<Path>) -> Self {
        Self {
            root: repo_root.as_ref().join(".orbit"),
        }
    }

    /// Construct a layout where the supplied path IS the `.orbit/` root.
    pub fn at_orbit_dir(orbit_dir: impl AsRef<Path>) -> Self {
        Self { root: orbit_dir.as_ref().to_path_buf() }
    }

    pub fn schema_version_file(&self) -> PathBuf {
        self.root.join("schema-version")
    }

    pub fn state_db(&self) -> PathBuf {
        self.root.join("state.db")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.root.join("specs")
    }

    /// Per-spec folder: `specs/<id>/`. Holds `spec.yaml` plus all sidecars.
    pub fn spec_dir(&self, id: &str) -> PathBuf {
        self.specs_dir().join(id)
    }

    pub fn spec_file(&self, id: &str) -> PathBuf {
        self.spec_dir(id).join("spec.yaml")
    }

    pub fn task_stream(&self, spec_id: &str) -> PathBuf {
        self.spec_dir(spec_id).join("tasks.jsonl")
    }

    pub fn notes_stream(&self, spec_id: &str) -> PathBuf {
        self.spec_dir(spec_id).join("notes.jsonl")
    }

    /// Create the per-spec folder. Idempotent. Callers must invoke this
    /// before writing `spec_file(id)`, `task_stream(id)`, `notes_stream(id)`,
    /// or any sidecar — `write_atomic` and `append_jsonl_line` reject a
    /// missing parent directory by design.
    pub fn ensure_spec_dir(&self, id: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(self.spec_dir(id))
    }

    pub fn cards_dir(&self) -> PathBuf {
        self.root.join("cards")
    }

    pub fn card_file(&self, slug: &str) -> PathBuf {
        self.cards_dir().join(format!("{slug}.yaml"))
    }

    pub fn memos_dir(&self) -> PathBuf {
        self.root.join("memos")
    }

    pub fn choices_dir(&self) -> PathBuf {
        self.root.join("choices")
    }

    pub fn choice_file(&self, id: &str) -> PathBuf {
        self.choices_dir().join(format!("{id}.yaml"))
    }

    pub fn memories_dir(&self) -> PathBuf {
        self.root.join("memories")
    }

    pub fn memory_file(&self, key: &str) -> PathBuf {
        self.memories_dir().join(format!("{key}.yaml"))
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn session_file(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{session_id}.yaml"))
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    pub fn skill_invocations_file(&self, skill_id: &str) -> PathBuf {
        self.skills_dir().join(format!("{skill_id}.invocations.jsonl"))
    }

    pub fn session_id_file(&self) -> PathBuf {
        self.root.join(".session-id")
    }

    /// `.orbit/.session-card` — single-line newline-terminated card slug
    /// written by `orbit session set-card <id>` and read by
    /// `orbit session distill` as a fallback when no `--card` is passed.
    /// Same shape as `.session-id`. See spec 2026-05-16-session-handover
    /// ac-04 for the verb and ac-08 for the Stop-hook deletion contract.
    pub fn session_card_file(&self) -> PathBuf {
        self.root.join(".session-card")
    }

    /// Create all expected subdirectories. Idempotent.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.root,
            &self.specs_dir(),
            &self.cards_dir(),
            &self.memos_dir(),
            &self.choices_dir(),
            &self.memories_dir(),
            &self.sessions_dir(),
            &self.locks_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Return every `<id>/spec.yaml` under `specs/`, sorted by path.
    ///
    /// Scans immediate subdirectories of `specs_dir()`. A subdirectory
    /// without a `spec.yaml` is skipped silently (it's a partial migration
    /// state or a stray folder, not a spec to load). Top-level `<id>.yaml`
    /// files are ignored — see choice 0021 for the layout rationale.
    pub fn list_spec_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let dir = self.specs_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let spec_yaml = path.join("spec.yaml");
            if spec_yaml.is_file() {
                out.push(spec_yaml);
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn list_card_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.cards_dir())
    }

    pub fn list_choice_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.choices_dir())
    }

    pub fn list_memory_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.memories_dir())
    }

    pub fn list_session_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.sessions_dir())
    }
}

fn list_yaml_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        // Dotless-stem filter: `0001.yaml` keeps; `0001.drive.yaml` skips
        // (its stem `0001.drive` contains a dot). This excludes sidecar
        // shapes like `<id>.drive.yaml` / `<id>.rally.yaml` from primary
        // entity loads (specs/cards/choices/memories) — see the layout
        // doc-comment at the top of this file.
        let stem_has_dot = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.contains('.'));
        if stem_has_dot {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn layout_paths_are_deterministic() {
        let layout = OrbitLayout::at("/tmp/repo");
        assert_eq!(layout.root, PathBuf::from("/tmp/repo/.orbit"));
        assert_eq!(layout.state_db(), PathBuf::from("/tmp/repo/.orbit/state.db"));
        assert_eq!(
            layout.spec_dir("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001")
        );
        assert_eq!(
            layout.spec_file("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001/spec.yaml")
        );
        assert_eq!(
            layout.task_stream("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001/tasks.jsonl")
        );
        assert_eq!(
            layout.notes_stream("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001/notes.jsonl")
        );
        assert_eq!(
            layout.card_file("0020-orbit-state"),
            PathBuf::from("/tmp/repo/.orbit/cards/0020-orbit-state.yaml")
        );
    }

    #[test]
    fn ensure_dirs_creates_full_tree() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        assert!(layout.specs_dir().exists());
        assert!(layout.cards_dir().exists());
        assert!(layout.memos_dir().exists());
        assert!(layout.choices_dir().exists());
        assert!(layout.memories_dir().exists());
        assert!(layout.locks_dir().exists());
    }

    #[test]
    fn ensure_dirs_is_idempotent() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        layout.ensure_dirs().unwrap();
        assert!(layout.specs_dir().exists());
    }

    #[test]
    fn list_spec_files_returns_folder_shape() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(layout.spec_file("0001"), "id: '0001'\n").unwrap();
        std::fs::write(
            layout.task_stream("0001"),
            r#"{"task_id":"t","spec_id":"0001","event":"open","timestamp":"x"}"#,
        )
        .unwrap();
        // Stray flat YAML at the top of specs/ — must not be picked up.
        std::fs::write(
            layout.specs_dir().join("0002-other.yaml"),
            "id: '0002-other'\n",
        )
        .unwrap();
        // Subdirectory without spec.yaml — silently skipped.
        std::fs::create_dir_all(layout.specs_dir().join("0003-empty")).unwrap();
        // Random file at top of specs/ — ignored.
        std::fs::write(layout.specs_dir().join("readme.md"), "ignore me").unwrap();

        let files = layout.list_spec_files().unwrap();
        assert_eq!(files.len(), 1, "only the folder-shape spec should be returned");
        assert_eq!(files[0], layout.spec_file("0001"));
    }

    #[test]
    fn list_spec_files_ignores_sidecars_inside_folder() {
        // Sidecars sit alongside spec.yaml inside the folder; only spec.yaml
        // is the primary entity. The scanner returns spec.yaml paths and
        // never enumerates folder contents beyond that.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        layout.ensure_spec_dir("2026-05-09-foo").unwrap();
        std::fs::write(layout.spec_file("2026-05-09-foo"), "id: '2026-05-09-foo'\n").unwrap();
        std::fs::write(
            layout.spec_dir("2026-05-09-foo").join("drive.yaml"),
            "spec_id: '2026-05-09-foo'\nstage: review-spec\n",
        )
        .unwrap();
        std::fs::write(
            layout.spec_dir("2026-05-09-foo").join("review-spec-2026-05-09.md"),
            "# Review",
        )
        .unwrap();

        let files = layout.list_spec_files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], layout.spec_file("2026-05-09-foo"));
    }

    #[test]
    fn list_card_files_returns_only_files_in_cards_dir() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.card_file("0020-x"), "feature: x\ngoal: y\nmaturity: planned\n")
            .unwrap();
        std::fs::write(
            layout.memos_dir().join("2026-05-07-idea.yaml"),
            "this is a memo not a card",
        )
        .unwrap();
        let files = layout.list_card_files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "0020-x.yaml");
    }
}
