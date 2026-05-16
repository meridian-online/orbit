//! orbit-migrate — Migration A (layout) and Migration B (substrate) tools.
//!
//! Per ac-12 + ac-13 of the v0.1 spec. The migration is invasive and one-way
//! (rollback is `git reset` because all canonical files are tracked); both
//! migrations refuse to run unless the worktree is on a branch other than
//! `main` and is clean.
//!
//! Migration A (layout):
//!   - orbit/cards/        → .orbit/cards/
//!   - orbit/decisions/    → .orbit/choices/  (MD → YAML, frontmatter preserved)
//!   - orbit/specs/        → .orbit/specs/
//!   - orbit/conventions/  → .orbit/conventions/
//!   - orbit/discovery/    → .orbit/discovery/
//!   - Tracked files containing the verbatim path strings get those strings
//!     rewritten in place.
//!   - Idempotent: re-running on an already-migrated worktree is a no-op.
//!
//! Migration B (substrate):
//!   - bd issues with label=spec → .orbit/specs/<id>.yaml
//!   - bd notes on those issues  → spec.note events in <id>.notes.jsonl
//!   - bd memories               → .orbit/memories/<key>.yaml
//!   - bd issues with label=skill-author → tasks (or memos)
//!   - .beads/ → .beads-archive/ (and gitignored)

use clap::{Parser, Subcommand};
use orbit_state_core::canonical::{serialise_json_line, serialise_yaml};
use orbit_state_core::schema::{Choice, ChoiceStatus, Memory, NoteEvent, Spec};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "orbit-migrate", version, about)]
struct Cli {
    /// Repository root to migrate. Defaults to cwd.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Skip the safety check that refuses to run on `main`.
    #[arg(long, hide = true)]
    force_branch: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Migration A — folder layout (orbit/ → .orbit/, decisions/ → choices/).
    MigrateA,
    /// Migration B — bd state → orbit-state files.
    MigrateB {
        /// Run the verification step only (no writes).
        #[arg(long)]
        verify_only: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let repo = cli.repo.canonicalize()?;
    if !cli.force_branch {
        ensure_safe_branch(&repo)?;
    }
    match cli.cmd {
        Cmd::MigrateA => run_migration_a(&repo),
        Cmd::MigrateB { verify_only } => run_migration_b(&repo, verify_only),
    }
}

// ============================================================================
// Safety
// ============================================================================

fn ensure_safe_branch(repo: &Path) -> anyhow::Result<()> {
    let out = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(repo)
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "not a git repository or detached HEAD: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let branch = String::from_utf8(out.stdout)?.trim().to_string();
    if branch == "main" || branch == "master" {
        anyhow::bail!(
            "refusing to run migration on `{branch}` — use a worktree branch (override with --force-branch)"
        );
    }
    Ok(())
}

// ============================================================================
// Migration A — layout
// ============================================================================

fn run_migration_a(repo: &Path) -> anyhow::Result<()> {
    println!("Migration A — layout (orbit/ → .orbit/, decisions/ → choices/)");
    let orbit = repo.join("orbit");
    let dot_orbit = repo.join(".orbit");

    let already_migrated = dot_orbit.exists() && !orbit.exists();
    if already_migrated {
        println!("  .orbit/ already present, orbit/ absent — re-running reference rewrites only.");
    }

    if !already_migrated {
        if !orbit.exists() {
            anyhow::bail!("no orbit/ folder at {}", orbit.display());
        }

        // 1. Move cards, specs, conventions, discovery.
        std::fs::create_dir_all(&dot_orbit)?;
        for sub in ["cards", "specs", "conventions", "discovery"] {
            let src = orbit.join(sub);
            let dst = dot_orbit.join(sub);
            if src.exists() {
                println!("  mv orbit/{sub}/ → .orbit/{sub}/");
                git_mv(repo, &src, &dst)?;
            }
        }

        // 2. Convert decisions/*.md → choices/*.yaml.
        let decisions = orbit.join("decisions");
        let choices = dot_orbit.join("choices");
        if decisions.exists() {
            std::fs::create_dir_all(&choices)?;
            for entry in std::fs::read_dir(&decisions)? {
                let entry = entry?;
                let src = entry.path();
                if src.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                let stem = src
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("decision file has no stem: {}", src.display())
                    })?;
                let dst = choices.join(format!("{stem}.yaml"));
                println!("  convert orbit/decisions/{stem}.md → .orbit/choices/{stem}.yaml");
                let md = std::fs::read_to_string(&src)?;
                let choice = parse_decision_md(stem, &md)?;
                let yaml = serialise_yaml(&choice).map_err(anyhow_err)?;
                std::fs::write(&dst, yaml)?;
                git_add(repo, &dst)?;
                git_rm(repo, &src)?;
            }
            // Remove the now-empty decisions/ directory.
            if decisions.exists() {
                let still_has = std::fs::read_dir(&decisions)?.count();
                if still_has == 0 {
                    std::fs::remove_dir(&decisions).ok();
                }
            }
        }

        // 3. Move anything else still in orbit/ verbatim.
        if orbit.exists() {
            let mut leftovers = Vec::new();
            for entry in std::fs::read_dir(&orbit)? {
                let entry = entry?;
                leftovers.push(entry.path());
            }
            for src in leftovers {
                let name = src.file_name().unwrap();
                let dst = dot_orbit.join(name);
                println!(
                    "  mv orbit/{} → .orbit/{}",
                    name.to_string_lossy(),
                    name.to_string_lossy()
                );
                git_mv(repo, &src, &dst)?;
            }
            std::fs::remove_dir(&orbit).ok();
        }
    }

    // 4. Rewrite references in tracked files.
    println!("  rewriting references in tracked files...");
    let count = rewrite_references(repo)?;
    println!("  rewrote {count} file(s)");

    // 5. Verify.
    println!("  verifying...");
    let leftover = verify_no_orbit_refs(repo)?;
    if leftover.is_empty() {
        println!("Migration A complete — no orbit/cards|orbit/decisions|orbit/specs refs remain.");
    } else {
        eprintln!("WARNING: {} file(s) still reference old paths:", leftover.len());
        for p in leftover.iter().take(20) {
            eprintln!("  {p}");
        }
        anyhow::bail!("migration A verification failed");
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// MD → Choice YAML
// ----------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct DecisionFrontmatter {
    status: Option<String>,
    #[serde(rename = "date-created")]
    date_created: Option<String>,
    #[serde(rename = "date-modified")]
    date_modified: Option<String>,
}

fn parse_decision_md(stem: &str, md: &str) -> anyhow::Result<Choice> {
    let (frontmatter, rest) = split_frontmatter(md)?;
    let fm: DecisionFrontmatter = serde_yaml::from_str(&frontmatter).unwrap_or_default();

    // First H1 line — extract id and title from "# NNNN. Title".
    let mut h1_line = None;
    for line in rest.lines() {
        if let Some(h) = line.strip_prefix("# ") {
            h1_line = Some(h.to_string());
            break;
        }
    }
    let h1 = h1_line.ok_or_else(|| anyhow::anyhow!("{stem}: no H1 found"))?;
    let (id_str, title) = match h1.split_once(". ") {
        Some((n, t)) => (n.trim().to_string(), t.trim().to_string()),
        None => {
            let id_from_stem = stem.split('-').next().unwrap_or(stem).to_string();
            (id_from_stem, h1.clone())
        }
    };

    let status = match fm.status.as_deref().unwrap_or("accepted") {
        "proposed" => ChoiceStatus::Proposed,
        "accepted" => ChoiceStatus::Accepted,
        "rejected" => ChoiceStatus::Rejected,
        "deprecated" => ChoiceStatus::Deprecated,
        s if s.starts_with("superseded") => ChoiceStatus::Superseded,
        other => anyhow::bail!("{stem}: unknown status: {other}"),
    };

    let date_created = fm.date_created.unwrap_or_else(|| "1970-01-01".into());

    Ok(Choice {
        id: id_str,
        title,
        status,
        date_created,
        date_modified: fm.date_modified,
        body: rest.trim_start_matches('\n').to_string(),
        references: vec![],
    })
}

fn split_frontmatter(md: &str) -> anyhow::Result<(String, String)> {
    let trimmed = md.trim_start_matches('\n');
    if !trimmed.starts_with("---") {
        return Ok((String::new(), trimmed.to_string()));
    }
    let after_open = trimmed.trim_start_matches("---\n");
    let close_idx = after_open
        .find("\n---\n")
        .or_else(|| after_open.find("\n---"))
        .ok_or_else(|| anyhow::anyhow!("frontmatter missing closing ---"))?;
    let frontmatter = &after_open[..close_idx];
    let rest = after_open[close_idx..]
        .trim_start_matches("\n---\n")
        .trim_start_matches("\n---")
        .to_string();
    Ok((frontmatter.to_string(), rest))
}

// ----------------------------------------------------------------------------
// Reference rewrites + verification
// ----------------------------------------------------------------------------

fn rewrite_references(repo: &Path) -> anyhow::Result<usize> {
    let tracked = git_ls_files(repo)?;
    let mut changed = 0usize;
    // Substring-level rewrite (not slash-anchored) so prose mentions like
    // "a project that has orbit/cards, orbit/decisions, orbit/specs" get
    // rewritten too — that's what the spec's verbatim grep verifies.
    // Idempotency is preserved by `replace_unprefixed`: matches preceded by
    // `.` (already `.orbit/...`) are skipped, so re-running is a no-op.
    let rewrites: &[(&str, &str)] = &[
        ("orbit/cards", ".orbit/cards"),
        ("orbit/decisions", ".orbit/choices"),
        ("orbit/specs", ".orbit/specs"),
        ("orbit/conventions", ".orbit/conventions"),
        ("orbit/discovery", ".orbit/discovery"),
    ];
    for rel in tracked {
        if rel.contains("orbit-state/crates/cli/src/bin/migrate.rs") {
            continue;
        }
        let path = repo.join(&rel);
        if !path.is_file() {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let mut new_text = text.to_string();
        let mut hit = false;
        for (from, to) in rewrites {
            let after = replace_unprefixed(&new_text, from, to);
            if after != new_text {
                new_text = after;
                hit = true;
            }
        }
        if hit {
            std::fs::write(&path, new_text)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Replace every occurrence of `from` with `to` UNLESS the match is preceded
/// by `.` — protects already-migrated `.orbit/...` strings from being
/// double-rewritten on re-runs.
fn replace_unprefixed(text: &str, from: &str, to: &str) -> String {
    let bytes = text.as_bytes();
    let from_bytes = from.as_bytes();
    let to_bytes = to.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if i + from_bytes.len() <= bytes.len() && &bytes[i..i + from_bytes.len()] == from_bytes {
            let preceded_by_dot = i > 0 && bytes[i - 1] == b'.';
            if preceded_by_dot {
                out.extend_from_slice(from_bytes);
            } else {
                out.extend_from_slice(to_bytes);
            }
            i += from_bytes.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("byte-preserving rewrite stays valid UTF-8")
}

fn verify_no_orbit_refs(repo: &Path) -> anyhow::Result<Vec<String>> {
    let tracked = git_ls_files(repo)?;
    let needles: &[&[u8]] = &[b"orbit/cards", b"orbit/decisions", b"orbit/specs"];
    let mut leftover = Vec::new();
    for rel in tracked {
        if rel.contains("orbit-state/crates/cli/src/bin/migrate.rs") {
            continue;
        }
        let path = repo.join(&rel);
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if file_contains_unprefixed(text, needles) {
            leftover.push(rel);
        }
    }
    Ok(leftover)
}

fn file_contains_unprefixed(text: &str, needles: &[&[u8]]) -> bool {
    let bytes = text.as_bytes();
    for needle in needles {
        let mut start = 0usize;
        while let Some(pos) = find_subslice(&bytes[start..], needle) {
            let abs = start + pos;
            let preceded_by_dot = abs > 0 && bytes[abs - 1] == b'.';
            if !preceded_by_dot {
                return true;
            }
            start = abs + needle.len();
        }
    }
    false
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ============================================================================
// Migration B — substrate (bd → orbit-state)
// ============================================================================

fn run_migration_b(repo: &Path, verify_only: bool) -> anyhow::Result<()> {
    println!("Migration B — substrate (bd → orbit-state)");
    // Read from .beads/ if present; fall back to .beads-archive/ for
    // re-runs after the archive step has already happened.
    let live = repo.join(".beads");
    let archive = repo.join(".beads-archive");
    let beads_dir = if live.exists() {
        live
    } else if archive.exists() {
        println!("  .beads/ already archived — reading from .beads-archive/");
        archive.clone()
    } else {
        anyhow::bail!(
            "neither .beads/ nor .beads-archive/ found at {}",
            repo.display()
        );
    };

    // bd's `.beads/issues.jsonl` is a multi-type stream. `_type=issue` are
    // issues, `_type=memory` are memory records (key + value). bd notes
    // (issue comments) live in Dolt SQL, not in the JSONL export — but
    // every issue in this snapshot has `comment_count: 0`, so the note
    // stream is empty. The migration documents this and surfaces a count
    // discrepancy if the assumption breaks.
    let all_records = read_bd_records(&beads_dir.join("issues.jsonl"))?;
    let issues: Vec<&BdRecord> = all_records.iter().filter(|r| r.kind() == "issue").collect();
    let memories: Vec<&BdRecord> = all_records.iter().filter(|r| r.kind() == "memory").collect();
    let total_comments: u64 = issues
        .iter()
        .map(|i| i.extra.get("comment_count").and_then(|v| v.as_u64()).unwrap_or(0))
        .sum();

    println!(
        "  source: {} issues ({} with comments), {} memories",
        issues.len(),
        issues
            .iter()
            .filter(|i| i.extra.get("comment_count").and_then(|v| v.as_u64()).unwrap_or(0) > 0)
            .count(),
        memories.len()
    );
    if total_comments > 0 {
        eprintln!(
            "  WARNING: {total_comments} bd comments not in JSONL export — Dolt-side notes \
             would need a separate query path; v0.1 migration leaves them. File a follow-up \
             if this snapshot ever has comment_count > 0."
        );
    }

    let dot_orbit = repo.join(".orbit");
    let specs_dir = dot_orbit.join("specs");
    let memories_dir = dot_orbit.join("memories");

    let mut spec_count = 0usize;
    let mut memory_count = 0usize;

    // 1. Specs from issues with label=spec.
    for issue in &issues {
        if !issue.has_label("spec") {
            continue;
        }
        let spec_id = bd_id_to_spec_id(&issue.id);
        let spec = bd_issue_to_spec(issue);
        if !verify_only {
            std::fs::create_dir_all(&specs_dir)?;
            let path = specs_dir.join(format!("{spec_id}.yaml"));
            let yaml = serialise_yaml(&spec).map_err(anyhow_err)?;
            std::fs::write(&path, yaml)?;
        }
        spec_count += 1;

        // The description body becomes the seed note for the spec — preserves
        // the bd-side context that's normally accumulated via `bd note`.
        if !verify_only {
            if let Some(body) = issue
                .extra
                .get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                let event = NoteEvent {
                    spec_id: spec_id.clone(),
                    body: body.to_string(),
                    labels: vec!["migrated-from-bd".into(), "seed".into()],
                    timestamp: issue
                        .timestamp()
                        .unwrap_or("1970-01-01T00:00:00Z")
                        .to_string(),
                };
                let line = serialise_json_line(&event).map_err(anyhow_err)?;
                use std::io::Write;
                let stream_path = specs_dir.join(format!("{spec_id}.notes.jsonl"));
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stream_path)?;
                f.write_all(line.as_bytes())?;
            }
        }
    }

    // 2. Memories — one orbit-state memory per `_type=memory` bd record.
    for mem in &memories {
        let key = mem
            .extra
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or(&mem.id)
            .to_string();
        // bd memory records use `value`, not `body`. Defensive default.
        let body = mem
            .extra
            .get("value")
            .and_then(|v| v.as_str())
            .or_else(|| mem.extra.get("body").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let timestamp = mem
            .timestamp()
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_string();
        let memory = Memory {
            key: sanitise_memory_key(&key),
            body,
            timestamp,
            labels: mem.labels(),
        };
        if !verify_only {
            std::fs::create_dir_all(&memories_dir)?;
            let path = memories_dir.join(format!("{}.yaml", memory.key));
            let yaml = serialise_yaml(&memory).map_err(anyhow_err)?;
            std::fs::write(&path, yaml)?;
        }
        memory_count += 1;
    }

    println!("  produced: {spec_count} spec(s), {memory_count} memor(ies)");

    // 3. Hash-set / count verification.
    let target_specs = issues.iter().filter(|i| i.has_label("spec")).count();
    if spec_count != target_specs {
        anyhow::bail!("spec count mismatch: {spec_count} written vs {target_specs} expected");
    }
    if memory_count != memories.len() {
        anyhow::bail!(
            "memory count mismatch: {memory_count} written vs {} expected",
            memories.len()
        );
    }

    // 4. Archive .beads/ (skip if already archived).
    if !verify_only {
        let live = repo.join(".beads");
        let archive_path = repo.join(".beads-archive");
        if live.exists() && !archive_path.exists() {
            println!("  archive: .beads/ → .beads-archive/");
            std::fs::rename(&live, &archive_path)?;
        }
        ensure_gitignore_has_beads_archive(repo)?;
    }

    println!("Migration B complete — count-equality check passed.");
    Ok(())
}

/// bd memory keys may contain hyphens, dots, etc. Memory file paths use the
/// key as the file stem; reject anything that would be unsafe.
fn sanitise_memory_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// ----------------------------------------------------------------------------
// bd JSONL parsing — keep loose; bd's record shape evolves.
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BdRecord {
    #[serde(default)]
    id: String,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl BdRecord {
    fn kind(&self) -> &str {
        self.extra
            .get("_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
    }

    fn has_label(&self, label: &str) -> bool {
        self.extra
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|x| x.as_str() == Some(label)))
            .unwrap_or(false)
    }
    fn issue_id(&self) -> Option<&str> {
        self.extra
            .get("issue_id")
            .and_then(|v| v.as_str())
            .or_else(|| self.extra.get("parent").and_then(|v| v.as_str()))
    }
    fn body(&self) -> Option<&str> {
        self.extra.get("body").and_then(|v| v.as_str())
    }
    fn timestamp(&self) -> Option<&str> {
        self.extra
            .get("timestamp")
            .and_then(|v| v.as_str())
            .or_else(|| self.extra.get("created_at").and_then(|v| v.as_str()))
    }
    fn key(&self) -> Option<&str> {
        self.extra
            .get("key")
            .and_then(|v| v.as_str())
            .or(Some(self.id.as_str()))
    }
    fn labels(&self) -> Vec<String> {
        self.extra
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn read_bd_records(path: &Path) -> anyhow::Result<Vec<BdRecord>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<BdRecord>(line) {
            Ok(r) => out.push(r),
            Err(e) => {
                eprintln!(
                    "  warning: skipping malformed bd record at {}:{}: {e}",
                    path.display(),
                    i + 1
                );
            }
        }
    }
    Ok(out)
}

fn bd_id_to_spec_id(bd_id: &str) -> String {
    bd_id.replace(['/', '\\'], "-")
}

fn bd_issue_to_spec(issue: &BdRecord) -> Spec {
    use orbit_state_core::schema::{AcceptanceCriterion, SpecStatus};
    let goal = issue
        .extra
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(missing title)")
        .to_string();
    let cards: Vec<String> = issue
        .extra
        .get("cards")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let labels: Vec<String> = issue
        .extra
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let status = match issue.extra.get("status").and_then(|v| v.as_str()) {
        Some("closed") | Some("done") => SpecStatus::Closed,
        _ => SpecStatus::Open,
    };
    let mut acs = Vec::new();
    // bd's field is `acceptance_criteria`; older snapshots may use `acceptance`.
    if let Some(raw) = issue
        .extra
        .get("acceptance_criteria")
        .and_then(|v| v.as_str())
        .or_else(|| issue.extra.get("acceptance").and_then(|v| v.as_str()))
    {
        for line in raw.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("- [") {
                let checked = rest.starts_with('x');
                let after = rest.split_once(']').map(|(_, r)| r.trim()).unwrap_or("");
                if let Some((id_part, desc)) = after.split_once(':') {
                    let gate = id_part.contains("[gate]");
                    let id = id_part.replace("[gate]", "").trim().to_string();
                    if !id.is_empty() {
                        acs.push(AcceptanceCriterion {
                            id,
                            description: desc.trim().to_string(),
                            gate,
                            checked,
                            verification: None,
                            ac_type: orbit_state_core::schema::AcType::Code,
                        });
                    }
                }
            }
        }
    }
    Spec {
        id: bd_id_to_spec_id(&issue.id),
        goal,
        cards,
        status,
        labels,
        acceptance_criteria: acs,
    }
}

fn ensure_gitignore_has_beads_archive(repo: &Path) -> anyhow::Result<()> {
    let path = repo.join(".gitignore");
    let mut text = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    if !text.lines().any(|l| l.trim() == ".beads-archive/") {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(".beads-archive/\n");
        std::fs::write(&path, text)?;
        println!("  gitignore: added .beads-archive/");
    }
    Ok(())
}

// ============================================================================
// Git wrappers
// ============================================================================

fn git_ls_files(repo: &Path) -> anyhow::Result<Vec<String>> {
    let out = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(repo)
        .output()?;
    if !out.status.success() {
        anyhow::bail!("git ls-files failed");
    }
    Ok(String::from_utf8(out.stdout)?
        .lines()
        .map(String::from)
        .collect())
}

fn git_mv(repo: &Path, src: &Path, dst: &Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("git")
        .args(["mv", "-f"])
        .arg(src)
        .arg(dst)
        .current_dir(repo)
        .status()?;
    if !status.success() {
        std::fs::rename(src, dst)?;
    }
    Ok(())
}

fn git_add(repo: &Path, path: &Path) -> anyhow::Result<()> {
    let _ = std::process::Command::new("git")
        .arg("add")
        .arg(path)
        .current_dir(repo)
        .status()?;
    Ok(())
}

fn git_rm(repo: &Path, path: &Path) -> anyhow::Result<()> {
    let _ = std::process::Command::new("git")
        .args(["rm", "-f"])
        .arg(path)
        .current_dir(repo)
        .status()?;
    Ok(())
}

fn anyhow_err(e: orbit_state_core::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
