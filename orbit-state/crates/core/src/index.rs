//! SQLite index over the canonical files.
//!
//! Per ac-02 (gate): the index is rebuildable from files alone. Files are the
//! canonical truth; the index is a derived query layer. `orbit verify` mode
//! rebuilds from scratch into a temp database and diffs against the current
//! index, surfacing any drift.
//!
//! Schema choices:
//! - One table per entity type.
//! - Primary key matches the entity's natural ID (spec.id, card slug, etc.).
//! - List/array fields stored as JSON text for round-trip preservation.
//! - `path` column on every table holds the absolute on-disk path of the
//!   canonical file the row was derived from.
//! - `last_modified` records the file's mtime in nanoseconds (best-effort)
//!   for cheap incremental-update decisions later.
//!
//! Tasks are NOT indexed in v0.1 — they're append-only JSONL streams that the
//! `task.*` verbs read directly. ac-07 may add a derived `task_state` view
//! later if query needs justify it.

use crate::canonical::parse_yaml;
use crate::error::{Error, Result};
use crate::layout::OrbitLayout;
use crate::schema::{Card, Choice, Memory, SchemaVersion, Spec};
use rusqlite::{params, Connection};
use std::path::Path;

/// SQL DDL for the entity tables. Single source of truth — `Index::open`
/// applies this on every connection.
const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS specs (
    id            TEXT PRIMARY KEY,
    path          TEXT NOT NULL,
    goal          TEXT NOT NULL,
    status        TEXT NOT NULL,
    cards         TEXT NOT NULL,  -- JSON array
    labels        TEXT NOT NULL,  -- JSON array
    last_modified INTEGER
);
CREATE INDEX IF NOT EXISTS idx_specs_status ON specs(status);

CREATE TABLE IF NOT EXISTS cards (
    slug          TEXT PRIMARY KEY,
    path          TEXT NOT NULL,
    feature       TEXT NOT NULL,
    goal          TEXT NOT NULL,
    maturity      TEXT NOT NULL,
    specs         TEXT NOT NULL,  -- JSON array
    last_modified INTEGER
);
CREATE INDEX IF NOT EXISTS idx_cards_maturity ON cards(maturity);

CREATE TABLE IF NOT EXISTS choices (
    id            TEXT PRIMARY KEY,
    path          TEXT NOT NULL,
    title         TEXT NOT NULL,
    status        TEXT NOT NULL,
    date_created  TEXT NOT NULL,
    date_modified TEXT,
    last_modified INTEGER
);
CREATE INDEX IF NOT EXISTS idx_choices_status ON choices(status);

CREATE TABLE IF NOT EXISTS memories (
    key           TEXT PRIMARY KEY,
    path          TEXT NOT NULL,
    body          TEXT NOT NULL,
    timestamp     TEXT NOT NULL,
    labels        TEXT NOT NULL,  -- JSON array
    last_modified INTEGER
);

CREATE TABLE IF NOT EXISTS meta (
    key           TEXT PRIMARY KEY,
    value         TEXT NOT NULL
);
"#;

/// A handle to the on-disk SQLite index.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open the index at `path`, creating it (and applying the schema) if it
    /// does not exist. Pragmas: WAL for concurrent reads, foreign_keys on.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref()).map_err(|e| {
            Error::unavailable("index.open", format!("open failed: {e}")).with_source(e)
        })?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| {
                Error::unavailable("index.open", format!("pragma failed: {e}")).with_source(e)
            })?;
        conn.execute_batch(SCHEMA_DDL).map_err(|e| {
            Error::unavailable("index.open", format!("schema apply failed: {e}"))
                .with_source(e)
        })?;
        Ok(Self { conn })
    }

    /// Open an in-memory index (used for the rebuild-and-diff path of
    /// `verify`).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| {
            Error::unavailable("index.open", format!("in-memory open failed: {e}"))
                .with_source(e)
        })?;
        conn.execute_batch(SCHEMA_DDL).map_err(|e| {
            Error::unavailable("index.open", format!("schema apply failed: {e}"))
                .with_source(e)
        })?;
        Ok(Self { conn })
    }

    /// Truncate every entity table — used at the start of a full rebuild.
    pub fn clear(&self) -> Result<()> {
        for table in ["specs", "cards", "choices", "memories", "meta"] {
            self.conn
                .execute(&format!("DELETE FROM {table}"), [])
                .map_err(|e| {
                    Error::unavailable(
                        "index.clear",
                        format!("delete from {table} failed: {e}"),
                    )
                    .with_source(e)
                })?;
        }
        Ok(())
    }

    /// Full rebuild: scan the layout, parse every canonical file, populate
    /// the tables. Existing rows are dropped first.
    ///
    /// Returns a [`RebuildSummary`] for logging / verification use.
    pub fn rebuild_from_files(&mut self, layout: &OrbitLayout) -> Result<RebuildSummary> {
        let mut summary = RebuildSummary::default();
        let tx = self.conn.transaction().map_err(map_sqlite("index.rebuild"))?;

        // Clear within the transaction so a failure rolls back to the prior
        // index state.
        for table in ["specs", "cards", "choices", "memories", "meta"] {
            tx.execute(&format!("DELETE FROM {table}"), [])
                .map_err(map_sqlite("index.rebuild"))?;
        }

        // schema-version → meta table
        if layout.schema_version_file().exists() {
            let text = std::fs::read_to_string(layout.schema_version_file())
                .map_err(|e| Error::unavailable("index.rebuild", format!("read schema-version: {e}")))?;
            let sv: SchemaVersion = parse_yaml(&text)?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![sv.version],
            )
            .map_err(map_sqlite("index.rebuild"))?;
            if let Some(note) = sv.note {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version_note', ?1)",
                    params![note],
                )
                .map_err(map_sqlite("index.rebuild"))?;
            }
        }

        // Specs
        for path in layout
            .list_spec_files()
            .map_err(|e| Error::unavailable("index.rebuild", format!("list specs: {e}")))?
        {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Error::unavailable("index.rebuild", format!("read {}: {e}", path.display()))
            })?;
            let spec: Spec = parse_yaml(&text)?;
            let cards_json = serde_json::to_string(&spec.cards).map_err(map_json("index.rebuild"))?;
            let labels_json = serde_json::to_string(&spec.labels).map_err(map_json("index.rebuild"))?;
            let mtime = file_mtime_nanos(&path);
            let status = match spec.status {
                crate::schema::SpecStatus::Open => "open",
                crate::schema::SpecStatus::Closed => "closed",
            };
            tx.execute(
                "INSERT INTO specs (id, path, goal, status, cards, labels, last_modified) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![spec.id, path.display().to_string(), spec.goal, status, cards_json, labels_json, mtime],
            )
            .map_err(map_sqlite("index.rebuild"))?;
            summary.specs += 1;
        }

        // Cards
        for path in layout
            .list_card_files()
            .map_err(|e| Error::unavailable("index.rebuild", format!("list cards: {e}")))?
        {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Error::unavailable("index.rebuild", format!("read {}: {e}", path.display()))
            })?;
            let card: Card = parse_yaml(&text)?;
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| {
                    Error::malformed(
                        "index.rebuild",
                        format!("card path has no file stem: {}", path.display()),
                    )
                })?
                .to_string();
            let specs_json = serde_json::to_string(&card.specs).map_err(map_json("index.rebuild"))?;
            let mtime = file_mtime_nanos(&path);
            let maturity = match card.maturity {
                crate::schema::CardMaturity::Planned => "planned",
                crate::schema::CardMaturity::Emerging => "emerging",
                crate::schema::CardMaturity::Established => "established",
            };
            tx.execute(
                "INSERT INTO cards (slug, path, feature, goal, maturity, specs, last_modified) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![slug, path.display().to_string(), card.feature, card.goal, maturity, specs_json, mtime],
            )
            .map_err(map_sqlite("index.rebuild"))?;
            summary.cards += 1;
        }

        // Choices
        for path in layout
            .list_choice_files()
            .map_err(|e| Error::unavailable("index.rebuild", format!("list choices: {e}")))?
        {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Error::unavailable("index.rebuild", format!("read {}: {e}", path.display()))
            })?;
            let choice: Choice = parse_yaml(&text)?;
            let mtime = file_mtime_nanos(&path);
            let status = match choice.status {
                crate::schema::ChoiceStatus::Proposed => "proposed",
                crate::schema::ChoiceStatus::Accepted => "accepted",
                crate::schema::ChoiceStatus::Rejected => "rejected",
                crate::schema::ChoiceStatus::Deprecated => "deprecated",
                crate::schema::ChoiceStatus::Superseded => "superseded",
            };
            tx.execute(
                "INSERT INTO choices (id, path, title, status, date_created, date_modified, last_modified) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    choice.id,
                    path.display().to_string(),
                    choice.title,
                    status,
                    choice.date_created,
                    choice.date_modified,
                    mtime,
                ],
            )
            .map_err(map_sqlite("index.rebuild"))?;
            summary.choices += 1;
        }

        // Memories
        for path in layout
            .list_memory_files()
            .map_err(|e| Error::unavailable("index.rebuild", format!("list memories: {e}")))?
        {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Error::unavailable("index.rebuild", format!("read {}: {e}", path.display()))
            })?;
            let memory: Memory = parse_yaml(&text)?;
            let labels_json = serde_json::to_string(&memory.labels).map_err(map_json("index.rebuild"))?;
            let mtime = file_mtime_nanos(&path);
            tx.execute(
                "INSERT INTO memories (key, path, body, timestamp, labels, last_modified) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![memory.key, path.display().to_string(), memory.body, memory.timestamp, labels_json, mtime],
            )
            .map_err(map_sqlite("index.rebuild"))?;
            summary.memories += 1;
        }

        tx.commit().map_err(map_sqlite("index.rebuild"))?;
        Ok(summary)
    }

    /// Verify the on-disk index against a fresh rebuild from files.
    ///
    /// Builds an in-memory index from the same layout, then diffs the
    /// row sets. Returns `Ok(VerifyReport)` describing any drift; the report
    /// is `is_clean()` when there is none. Per ac-02 verification, callers
    /// translate `!is_clean()` into a non-zero exit code.
    pub fn verify(&mut self, layout: &OrbitLayout) -> Result<VerifyReport> {
        let mut fresh = Index::open_in_memory()?;
        fresh.rebuild_from_files(layout)?;

        let mut report = VerifyReport::default();
        report.diff_table(&self.conn, &fresh.conn, "specs", "id")?;
        report.diff_table(&self.conn, &fresh.conn, "cards", "slug")?;
        report.diff_table(&self.conn, &fresh.conn, "choices", "id")?;
        report.diff_table(&self.conn, &fresh.conn, "memories", "key")?;
        Ok(report)
    }

    /// Borrow the underlying SQLite connection for read queries that the
    /// verb layer will eventually use. Kept narrow so we can swap the storage
    /// engine later without rewriting verb code.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Counts produced by a successful rebuild.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RebuildSummary {
    pub specs: usize,
    pub cards: usize,
    pub choices: usize,
    pub memories: usize,
}

/// Report from `Index::verify`. `drift` is empty on a healthy index.
#[derive(Debug, Default)]
pub struct VerifyReport {
    pub drift: Vec<DriftEntry>,
}

#[derive(Debug)]
pub struct DriftEntry {
    pub table: &'static str,
    pub key: String,
    pub kind: DriftKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// Row exists in current index but not in fresh rebuild from files.
    OnlyInIndex,
    /// Row exists in fresh rebuild but not in current index.
    OnlyInFiles,
    /// Row exists in both but the projected columns differ.
    Differs,
}

impl VerifyReport {
    pub fn is_clean(&self) -> bool {
        self.drift.is_empty()
    }

    fn diff_table(
        &mut self,
        current: &Connection,
        fresh: &Connection,
        table: &'static str,
        key_col: &str,
    ) -> Result<()> {
        // Pull a stable projection that captures the row's content for diff.
        // We hash the JSON-encoded row so fields don't have to be enumerated
        // in this code (any column drift surfaces as a content mismatch).
        let sql = format!(
            "SELECT {key_col}, json_object('row', json_group_object(name, value)) \
             FROM {table} \
             JOIN pragma_table_info('{table}') ON 1=1 \
             GROUP BY {key_col}"
        );
        // For simplicity, fall back to a column-list-aware dump that doesn't
        // require json_group_object semantics across SQLite versions.
        let _ = sql; // placeholder — implementation below uses a simpler comparison

        let mut current_rows = collect_rows(current, table, key_col)?;
        let mut fresh_rows = collect_rows(fresh, table, key_col)?;

        // Sort for deterministic diff output.
        current_rows.sort_by(|a, b| a.key.cmp(&b.key));
        fresh_rows.sort_by(|a, b| a.key.cmp(&b.key));

        let mut ci = current_rows.into_iter().peekable();
        let mut fi = fresh_rows.into_iter().peekable();
        loop {
            match (ci.peek(), fi.peek()) {
                (None, None) => break,
                (Some(_), None) => {
                    let r = ci.next().unwrap();
                    self.drift.push(DriftEntry {
                        table,
                        key: r.key,
                        kind: DriftKind::OnlyInIndex,
                    });
                }
                (None, Some(_)) => {
                    let r = fi.next().unwrap();
                    self.drift.push(DriftEntry {
                        table,
                        key: r.key,
                        kind: DriftKind::OnlyInFiles,
                    });
                }
                (Some(c), Some(f)) => {
                    use std::cmp::Ordering::*;
                    match c.key.cmp(&f.key) {
                        Less => {
                            let r = ci.next().unwrap();
                            self.drift.push(DriftEntry {
                                table,
                                key: r.key,
                                kind: DriftKind::OnlyInIndex,
                            });
                        }
                        Greater => {
                            let r = fi.next().unwrap();
                            self.drift.push(DriftEntry {
                                table,
                                key: r.key,
                                kind: DriftKind::OnlyInFiles,
                            });
                        }
                        Equal => {
                            let c = ci.next().unwrap();
                            let f = fi.next().unwrap();
                            // last_modified depends on filesystem mtime which
                            // can drift legitimately; we exclude it from the
                            // content hash by ignoring it here.
                            if c.content_hash != f.content_hash {
                                self.drift.push(DriftEntry {
                                    table,
                                    key: c.key,
                                    kind: DriftKind::Differs,
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct RowFingerprint {
    key: String,
    content_hash: u64,
}

fn collect_rows(conn: &Connection, table: &str, key_col: &str) -> Result<Vec<RowFingerprint>> {
    // Pull column list for the table so we hash content excluding last_modified.
    let mut col_stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(map_sqlite("index.verify"))?;
    let col_iter = col_stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sqlite("index.verify"))?;
    let columns: Vec<String> = col_iter
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(map_sqlite("index.verify"))?;
    let projection: Vec<&str> = columns
        .iter()
        .filter(|c| c.as_str() != "last_modified")
        .map(|s| s.as_str())
        .collect();

    let mut rows = Vec::new();
    let select = format!("SELECT {} FROM {}", projection.join(", "), table);
    let mut stmt = conn.prepare(&select).map_err(map_sqlite("index.verify"))?;
    let key_idx = projection
        .iter()
        .position(|c| c == &key_col)
        .ok_or_else(|| Error::malformed("index.verify", format!("missing key column {key_col}")))?;
    let row_iter = stmt
        .query_map([], |row| {
            let mut values: Vec<String> = Vec::with_capacity(projection.len());
            for i in 0..projection.len() {
                let v: rusqlite::types::Value = row.get(i)?;
                values.push(format!("{v:?}"));
            }
            Ok(values)
        })
        .map_err(map_sqlite("index.verify"))?;
    for r in row_iter {
        let values = r.map_err(map_sqlite("index.verify"))?;
        let key = values[key_idx].clone();
        let content_hash = hash_strings(&values);
        rows.push(RowFingerprint { key, content_hash });
    }
    Ok(rows)
}

fn hash_strings(values: &[String]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    values.hash(&mut h);
    h.finish()
}

fn file_mtime_nanos(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let dur = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_nanos()).ok()
}

fn map_sqlite(verb: &'static str) -> impl Fn(rusqlite::Error) -> Error {
    move |e| Error::unavailable(verb, format!("sqlite: {e}")).with_source(e)
}

fn map_json(verb: &'static str) -> impl Fn(serde_json::Error) -> Error {
    move |e| Error::malformed(verb, format!("json: {e}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::serialise_yaml;
    use crate::error::Category;
    use crate::schema::{
        AcType, AcceptanceCriterion, Card, CardMaturity, Choice, ChoiceStatus, Memory,
        SchemaVersion, Spec, SpecStatus,
    };
    use tempfile::tempdir;

    fn write_yaml<T: serde::Serialize>(path: impl AsRef<Path>, value: &T) {
        let text = serialise_yaml(value).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn populate_layout(layout: &OrbitLayout) {
        layout.ensure_dirs().unwrap();

        std::fs::write(
            layout.schema_version_file(),
            "version: '0.1'\nnote: bootstrap\n",
        )
        .unwrap();

        let spec = Spec {
            id: "0001".into(),
            goal: "ship orbit-state v0.1".into(),
            cards: vec!["0020-orbit-state".into(), "0021-tasks".into()],
            status: SpecStatus::Open,
            labels: vec!["spec".into()],
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "ac-01".into(),
                description: "rust core".into(),
                gate: true,
                checked: true,
                verification: None,
                ac_type: AcType::Code,
            }],
        };
        layout.ensure_spec_dir("0001").unwrap();
        write_yaml(layout.spec_file("0001"), &spec);

        let card = Card {
            id: Some("0001-orbit-state".into()),
            feature: "orbit-state".into(),
            as_a: None,
            i_want: None,
            so_that: None,
            goal: "files-canonical substrate".into(),
            maturity: CardMaturity::Planned,
            scenarios: vec![],
            specs: vec![],
            relations: vec![],
            references: vec![],
            notes: vec![],
        };
        write_yaml(layout.card_file("0020-orbit-state"), &card);

        let choice = Choice {
            id: "0015".into(),
            title: "orbit-state architecture".into(),
            status: ChoiceStatus::Accepted,
            date_created: "2026-05-07".into(),
            date_modified: None,
            body: "files canonical\n".into(),
            references: vec![],
        };
        write_yaml(layout.choice_file("0015-orbit-state-architecture"), &choice);

        let memory = Memory {
            key: "estimate-inflation-guard".into(),
            body: "recut at Claude-pace".into(),
            timestamp: "2026-05-07T00:00:00Z".into(),
            labels: vec![],
        };
        write_yaml(
            layout.memory_file("estimate-inflation-guard"),
            &memory,
        );
    }

    #[test]
    fn rebuild_populates_all_tables() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let mut idx = Index::open(layout.state_db()).unwrap();
        let summary = idx.rebuild_from_files(&layout).unwrap();
        assert_eq!(summary.specs, 1);
        assert_eq!(summary.cards, 1);
        assert_eq!(summary.choices, 1);
        assert_eq!(summary.memories, 1);
    }

    #[test]
    fn delete_state_db_then_rebuild_reproduces_query_results() {
        // ac-02 verification: delete state.db, rebuild, query results match.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let pre_query: Vec<(String, String)> = {
            let mut idx = Index::open(layout.state_db()).unwrap();
            idx.rebuild_from_files(&layout).unwrap();
            let mut stmt = idx
                .conn()
                .prepare("SELECT id, status FROM specs ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };

        // Drop the index file completely.
        std::fs::remove_file(layout.state_db()).unwrap();
        assert!(!layout.state_db().exists());

        let post_query: Vec<(String, String)> = {
            let mut idx = Index::open(layout.state_db()).unwrap();
            idx.rebuild_from_files(&layout).unwrap();
            let mut stmt = idx
                .conn()
                .prepare("SELECT id, status FROM specs ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };

        assert_eq!(
            pre_query, post_query,
            "queries must match across an index-delete / rebuild cycle"
        );
    }

    #[test]
    fn verify_clean_on_freshly_rebuilt_index() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let mut idx = Index::open(layout.state_db()).unwrap();
        idx.rebuild_from_files(&layout).unwrap();
        let report = idx.verify(&layout).unwrap();
        assert!(report.is_clean(), "drift after fresh rebuild: {:?}", report.drift);
    }

    #[test]
    fn verify_detects_only_in_index_drift() {
        // Inject drift: insert a row directly into the index that's not on disk.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let mut idx = Index::open(layout.state_db()).unwrap();
        idx.rebuild_from_files(&layout).unwrap();

        idx.conn()
            .execute(
                "INSERT INTO specs (id, path, goal, status, cards, labels, last_modified) \
                 VALUES ('phantom', '/nowhere.yaml', 'g', 'open', '[]', '[]', NULL)",
                [],
            )
            .unwrap();

        let report = idx.verify(&layout).unwrap();
        assert!(!report.is_clean(), "drift not detected");
        let phantom_drift = report
            .drift
            .iter()
            .find(|d| d.table == "specs" && d.key.contains("phantom"));
        assert!(phantom_drift.is_some(), "phantom row not flagged: {:?}", report.drift);
    }

    #[test]
    fn verify_detects_only_in_files_drift() {
        // Inject drift: write a new spec on disk after the index was built.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let mut idx = Index::open(layout.state_db()).unwrap();
        idx.rebuild_from_files(&layout).unwrap();

        let new_spec = Spec {
            id: "0002".into(),
            goal: "follow-up".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0002").unwrap();
        write_yaml(layout.spec_file("0002"), &new_spec);

        let report = idx.verify(&layout).unwrap();
        assert!(!report.is_clean());
        assert!(report
            .drift
            .iter()
            .any(|d| d.table == "specs" && d.key.contains("0002")));
    }

    #[test]
    fn rebuild_failure_rolls_back_prior_state() {
        // If a malformed spec causes parse failure, the index must remain in
        // its prior state — transactional rebuild is the contract.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        populate_layout(&layout);

        let mut idx = Index::open(layout.state_db()).unwrap();
        let initial = idx.rebuild_from_files(&layout).unwrap();
        assert_eq!(initial.specs, 1);

        // Drop a malformed spec into the directory.
        layout.ensure_spec_dir("malformed").unwrap();
        std::fs::write(
            layout.spec_file("malformed"),
            "id: '0003'\nstatus: open\nunknown_field: oops\n",
        )
        .unwrap();

        let err = idx.rebuild_from_files(&layout).unwrap_err();
        assert_eq!(err.category, Category::Malformed);

        // Original spec should still be queryable.
        let mut stmt = idx
            .conn()
            .prepare("SELECT id FROM specs WHERE id = '0001'")
            .unwrap();
        let count = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap().count();
        assert_eq!(count, 1, "rollback did not preserve prior spec rows");
    }

    #[test]
    fn schema_version_lands_in_meta_table() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        let sv = SchemaVersion {
            version: "0.1".into(),
            note: Some("bootstrap".into()),
        };
        write_yaml(layout.schema_version_file(), &sv);

        let mut idx = Index::open(layout.state_db()).unwrap();
        idx.rebuild_from_files(&layout).unwrap();

        let value: String = idx
            .conn()
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "0.1");
    }
}
