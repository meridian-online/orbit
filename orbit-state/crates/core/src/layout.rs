//! `.orbit/` directory layout.
//!
//! Single source of truth for where each entity type lives on disk. Paths are
//! relative to a root that the caller supplies (typically the repo root).
//!
//! Layout (per card 0008 + ac-20). See `.orbit/conventions/spec-layout.md`
//! for the canonical sidecar inventory; the rule below is its mechanical
//! enforcement.
//!
//! ```text
//! .orbit/
//!   schema-version       (substrate-written, gitignored)
//!   state.db             (derived index, gitignored)
//!   locks/               (lock files, gitignored)
//!   specs/<id>.yaml      (substrate-written, tracked) — primary spec
//!   specs/<id>.tasks.jsonl       (append-only events, tracked)
//!   specs/<id>.notes.jsonl       (append-only notes, tracked)
//!   specs/<id>.drive.yaml        (drive sidecar, tracked)
//!   specs/<id>.rally.yaml        (rally sidecar, tracked)
//!   specs/<id>.review-spec-<date>.md   (review artefact, tracked)
//!   specs/<id>.review-pr-<date>.md     (review artefact, tracked)
//!   cards/<slug>.yaml    (human-written, tracked)
//!   cards/memos/         (memos awaiting distillation, tracked)
//!   choices/<slug>.yaml  (human-written, tracked)
//!   memories/<slug>.yaml (substrate-written, tracked)
//! ```
//!
//! Spec files are loaded from `specs/*.yaml` filtered to dotless stems.
//! `<id>.drive.yaml` and `<id>.rally.yaml` are sidecars, not specs — the
//! scanner skips them because their file stems contain a `.`.

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

    pub fn spec_file(&self, id: &str) -> PathBuf {
        self.specs_dir().join(format!("{id}.yaml"))
    }

    pub fn task_stream(&self, spec_id: &str) -> PathBuf {
        self.specs_dir().join(format!("{spec_id}.tasks.jsonl"))
    }

    pub fn notes_stream(&self, spec_id: &str) -> PathBuf {
        self.specs_dir().join(format!("{spec_id}.notes.jsonl"))
    }

    pub fn cards_dir(&self) -> PathBuf {
        self.root.join("cards")
    }

    pub fn card_file(&self, slug: &str) -> PathBuf {
        self.cards_dir().join(format!("{slug}.yaml"))
    }

    pub fn memos_dir(&self) -> PathBuf {
        self.cards_dir().join("memos")
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

    /// Create all expected subdirectories. Idempotent.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.root,
            &self.specs_dir(),
            &self.cards_dir(),
            &self.memos_dir(),
            &self.choices_dir(),
            &self.memories_dir(),
            &self.locks_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Return all spec YAML files (not the .tasks.jsonl streams) under specs/.
    pub fn list_spec_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.specs_dir())
    }

    pub fn list_card_files(&self) -> std::io::Result<Vec<PathBuf>> {
        // Cards live directly in cards/, not under cards/memos/.
        list_yaml_files_shallow(&self.cards_dir())
    }

    pub fn list_choice_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.choices_dir())
    }

    pub fn list_memory_files(&self) -> std::io::Result<Vec<PathBuf>> {
        list_yaml_files(&self.memories_dir())
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

fn list_yaml_files_shallow(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    // Like list_yaml_files but explicitly does NOT recurse — used for cards/
    // where we want to skip cards/memos/.
    list_yaml_files(dir)
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
            layout.spec_file("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001.yaml")
        );
        assert_eq!(
            layout.task_stream("0001"),
            PathBuf::from("/tmp/repo/.orbit/specs/0001.tasks.jsonl")
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
    fn list_yaml_filters_extension() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.spec_file("0001"), "id: '0001'\n").unwrap();
        std::fs::write(layout.specs_dir().join("readme.md"), "ignore me").unwrap();
        std::fs::write(
            layout.task_stream("0001"),
            r#"{"task_id":"t","spec_id":"0001","event":"open","timestamp":"x"}"#,
        )
        .unwrap();
        let files = layout.list_spec_files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "0001.yaml");
    }

    #[test]
    fn list_spec_files_skips_sidecar_shapes() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        // Primary spec
        std::fs::write(layout.spec_file("2026-05-09-foo"), "id: '2026-05-09-foo'\n").unwrap();
        // Sidecars — must NOT be returned by list_spec_files
        std::fs::write(
            layout.specs_dir().join("2026-05-09-foo.drive.yaml"),
            "spec_id: '2026-05-09-foo'\nstage: review-spec\n",
        )
        .unwrap();
        std::fs::write(
            layout.specs_dir().join("2026-05-09-bar.rally.yaml"),
            "rally_id: '2026-05-09-bar'\n",
        )
        .unwrap();
        std::fs::write(
            layout.specs_dir().join("2026-05-09-foo.review-spec-2026-05-09.md"),
            "# Review",
        )
        .unwrap();

        let files = layout.list_spec_files().unwrap();
        assert_eq!(files.len(), 1, "only the dotless-stem yaml should be returned");
        assert_eq!(files[0].file_name().unwrap(), "2026-05-09-foo.yaml");
    }

    #[test]
    fn list_card_files_does_not_recurse_into_memos() {
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
