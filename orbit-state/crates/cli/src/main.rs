//! orbit CLI — files-canonical agent substrate.
//!
//! Architectural shape (per ac-05): the CLI parses argv into a typed
//! [`VerbRequest`], hands it to [`orbit_state_core::execute`], and renders
//! the response. The MCP server uses the same dispatch fn — the parity
//! guarantee falls out of "both surfaces serialise the same `VerbResponse`
//! through the same envelope helper."
//!
//! Output modes:
//! - default: human-readable text (TSV-like)
//! - `--json`: the wire envelope (`{"data":...,"ok":true}` / `{"error":...,"ok":false}`)
//!
//! The `--json` output is byte-identical to the envelope MCP wraps in its
//! `tools/call` response — that's the parity contract.
//!
//! ac-21 link preservation: `link_sanity_check` is called once at startup so
//! the linker keeps the rusqlite dependency even when the invoked verb path
//! doesn't touch SQLite. Once write verbs land (ac-06+) and exercise the
//! index, this call becomes redundant and can be removed.

use clap::{Parser, Subcommand};
use orbit_state_core::layout::OrbitLayout;
use orbit_state_core::Error as OrbitError;
use orbit_state_core::{
    canonicalise_all, envelope_err_string, envelope_ok_string, execute, CanonicaliseReport,
    CardListArgs, CardSearchArgs, CardShowArgs, CardShowResult, CardSpecsArgs, CardSpecsResult,
    CardTreeArgs, CardTreeEdge, CardTreeResult, ChoiceListArgs, ChoiceListResult, OverviewArgs,
    OverviewResult,
    ChoiceSearchArgs, ChoiceShowArgs, ChoiceShowResult, MemoryListArgs, MemoryListResult,
    MemoryRememberArgs, MemoryRememberResult, MemorySearchArgs, SessionPrimeArgs,
    SessionPrimeResult, SpecCloseArgs, SpecCloseResult, SpecCreateArgs, SpecCreateResult,
    SpecListArgs, SpecListResult, SpecNoteArgs, SpecNoteResult, SpecShowArgs, SpecShowResult,
    SpecUpdateArgs, SpecUpdateResult, TaskClaimArgs, TaskDoneArgs, TaskEventResult, TaskListArgs,
    TaskListResult, TaskOpenArgs, TaskOpenResult, TaskReadyArgs, TaskShowArgs, TaskShowResult,
    TaskUpdateArgs, VerbRequest, VerbResponse,
};
use std::path::PathBuf;
use std::process::ExitCode;

/// orbit — files-canonical agent substrate.
#[derive(Debug, Parser)]
#[command(name = "orbit", version, about)]
struct Cli {
    /// Path to the repo root (defaults to the current directory). The
    /// `.orbit/` folder is resolved relative to this.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    /// Emit the wire envelope as JSON instead of human-readable output.
    /// In `--json` mode the bytes are byte-identical to MCP's `tools/call`
    /// response payload — this is what the parity test compares.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Spec verbs (list, show, create, ...).
    Spec {
        #[command(subcommand)]
        action: SpecAction,
    },
    /// Task verbs (open, list, show, ready, claim, update, done).
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Memory verbs (remember, list, search).
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Card verbs (show, list, search) — read-only.
    Card {
        #[command(subcommand)]
        action: CardAction,
    },
    /// Choice verbs (show, list, search) — read-only.
    Choice {
        #[command(subcommand)]
        action: ChoiceAction,
    },
    /// Session priming context (open specs + recent memories).
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Single-screen project synthesis — open specs, cards-by-maturity,
    /// recent memories, most-connected card, and orphan cards. Bounded
    /// output regardless of project age.
    Overview {
        #[arg(long)]
        memory_cap: Option<usize>,
    },
    /// Substrate hygiene check — round-trip every canonical file (ac-16) and
    /// rebuild the index from files (ac-17). Exits non-zero on any drift.
    /// CI invokes this once per commit as the merge gate.
    Verify,
    /// Rewrite every canonical YAML through the canonical writer, fixing
    /// byte-drift in place. Use after hand-editing a card or choice when
    /// `orbit verify` reports `not_byte_identical`.
    Canonicalise {
        /// Parse and reserialise without writing — preview what would change.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// Prime an agent session — bounded output: open specs + up to K memories.
    Prime {
        /// Override the default memory cap (K=10).
        #[arg(long)]
        memory_cap: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
enum MemoryAction {
    /// Upsert a memory entry.
    Remember {
        key: String,
        body: String,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// List all memories.
    List,
    /// Search memories (substring, case-insensitive, body + labels).
    Search { query: String },
}

#[derive(Debug, Subcommand)]
enum CardAction {
    Show { slug: String },
    List {
        #[arg(long)]
        maturity: Option<String>,
    },
    Search { query: String },
    /// Render the local subgraph from a card (outgoing + incoming
    /// `relations:` edges). Default depth is 2.
    Tree {
        slug: String,
        #[arg(long, default_value_t = 2)]
        depth: u32,
    },
    /// List specs advancing a card, with bidirectional link health.
    /// Surfaces drift where the card's `specs:` and the spec's `cards:`
    /// arrays disagree.
    Specs { slug: String },
}

#[derive(Debug, Subcommand)]
enum ChoiceAction {
    Show { id: String },
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Search { query: String },
}

#[derive(Debug, Subcommand)]
enum TaskAction {
    /// Open a new task under a spec.
    Open {
        spec_id: String,
        body: String,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// List tasks (current state per task_id).
    List {
        #[arg(long)]
        spec_id: Option<String>,
        #[arg(long)]
        state: Option<String>,
    },
    /// Show one task with its full event history.
    Show {
        spec_id: String,
        task_id: String,
    },
    /// List claimable (open, no claim) tasks.
    Ready {
        #[arg(long)]
        spec_id: Option<String>,
    },
    /// Claim an open task.
    Claim {
        spec_id: String,
        task_id: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Append an update note to a task.
    Update {
        spec_id: String,
        task_id: String,
        body: String,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Mark a task done.
    Done {
        spec_id: String,
        task_id: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        timestamp: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SpecAction {
    /// List specs in `.orbit/specs/`, sorted by id.
    List {
        /// Filter by status (`open` or `closed`).
        #[arg(long)]
        status: Option<String>,
    },
    /// Show a single spec by id.
    Show {
        /// Spec identifier (e.g. `2026-05-07-orbit-state-v0.1` or `0001`).
        id: String,
    },
    /// Append a timestamped note to a spec.
    Note {
        /// Spec identifier.
        id: String,
        /// Note body. Use `-` to read from stdin (not yet implemented).
        body: String,
        /// Free-text labels (repeatable).
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Override the substrate timestamp. Primarily for migration tools
        /// porting historical timestamps; production callers omit this.
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Create a new spec at `.orbit/specs/<id>.yaml`.
    Create {
        /// Spec identifier (slug-shaped; no path separators).
        id: String,
        /// One-sentence statement of what shipping this spec achieves.
        goal: String,
        /// Cards this spec advances (repeatable).
        #[arg(long = "card")]
        cards: Vec<String>,
        /// Free-text labels (repeatable).
        #[arg(long = "label")]
        labels: Vec<String>,
    },
    /// Update fields on an existing spec (status changes go via `close`).
    Update {
        id: String,
        /// New goal sentence (omit to keep current).
        #[arg(long)]
        goal: Option<String>,
        /// Replace card list. Pass with no values to clear.
        #[arg(long = "cards", num_args = 0..)]
        cards: Option<Vec<String>>,
        /// Replace label list. Pass with no values to clear.
        #[arg(long = "labels", num_args = 0..)]
        labels: Option<Vec<String>>,
        /// Mark the named AC as checked (e.g. `ac-05`). Reads the current
        /// spec, flips the AC's `checked` flag to true, and writes the
        /// full acceptance_criteria list back via the canonical writer.
        /// Errors if the AC is missing or already checked.
        #[arg(long = "ac-check")]
        ac_check: Option<String>,
        /// Mark the named AC as unchecked (e.g. `ac-05`). Mirror of
        /// `--ac-check` — flips a checked AC back to unchecked.
        #[arg(long = "ac-uncheck")]
        ac_uncheck: Option<String>,
    },
    /// Close a spec; transactionally appends to linked cards' `specs` arrays.
    Close {
        id: String,
    },
    /// One-shot migration from flat sidecar layout to per-spec folders per
    /// choice 0021. For each `.orbit/specs/<id>.yaml`, creates `<id>/` and
    /// moves the yaml to `<id>/spec.yaml`. Sidecars matching `<id>.<suffix>`
    /// are folded into `<id>/<suffix>` with the leading dot stripped.
    /// Idempotent — safe to re-run.
    MigrateLayout {
        /// Print what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    // ac-21 link preservation: ensure the linker keeps rusqlite/SQLite even
    // if the invoked verb path doesn't touch the index.
    if let Err(e) = orbit_state_core::link_sanity_check() {
        eprintln!("orbit: unavailable: link sanity check failed: {e}");
        return ExitCode::FAILURE;
    }

    let cli = Cli::parse();
    let root = match cli.root.clone() {
        Some(p) => p,
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("orbit: unavailable: cannot resolve cwd: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let layout = OrbitLayout::at(&root);

    // `verify` and `canonicalise` are hygiene/admin commands, not verbs — they
    // don't go through execute(). Handle them directly so their output shape
    // (per-file path listings, exit code = drift/failure presence) stays
    // separate from the verb envelope.
    if matches!(cli.command, Command::Verify) {
        return run_verify(&layout, cli.json);
    }
    if let Command::Canonicalise { dry_run } = cli.command {
        return run_canonicalise(&layout, dry_run, cli.json);
    }
    if let Command::Spec {
        action: SpecAction::MigrateLayout { dry_run },
    } = cli.command
    {
        return run_migrate_layout(&layout, dry_run, cli.json);
    }

    let request = match build_request(&layout, &cli.command) {
        Ok(r) => r,
        Err(err) => {
            if cli.json {
                println!("{}", envelope_err_string(&err));
            } else {
                eprintln!("{err}");
            }
            return ExitCode::FAILURE;
        }
    };

    match execute(&layout, &request) {
        Ok(response) => {
            if cli.json {
                match envelope_ok_string(&response) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                render_human(&response);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            if cli.json {
                println!("{}", envelope_err_string(&err));
            } else {
                eprintln!("{err}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Run the substrate hygiene check (ac-16 + ac-17). Exits 0 on clean, 1 on
/// any drift. JSON mode emits a single line `{"ok": true|false, "round_trip_failures": [...], "index_drift": [...]}`
/// for CI consumption; human mode emits one line per failure plus a summary.
fn run_verify(layout: &OrbitLayout, json: bool) -> ExitCode {
    let outcome = match orbit_state_core::verify_all(layout) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("orbit verify: unavailable: {e}");
            return ExitCode::FAILURE;
        }
    };

    if json {
        // Hand-rolled JSON to avoid pulling serde_json into the binary just
        // for this one path — keeps the verify subcommand independent of the
        // verb envelope's serialisation stack.
        let mut out = String::from("{\"ok\":");
        out.push_str(if outcome.has_failures() { "false" } else { "true" });
        out.push_str(",\"round_trip_failures\":[");
        for (i, f) in outcome.round_trip_failures.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let kind = match &f.kind {
                orbit_state_core::RoundTripFailureKind::ParseFailed(msg) => {
                    format!("parse_failed: {msg}")
                }
                orbit_state_core::RoundTripFailureKind::NotByteIdentical => {
                    "not_byte_identical".into()
                }
            };
            out.push_str(&format!(
                "{{\"path\":{:?},\"kind\":{:?}}}",
                f.path.display().to_string(),
                kind
            ));
        }
        out.push_str("],\"index_drift\":[");
        for (i, d) in outcome.index_drift.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{d:?}"));
        }
        out.push_str("]}");
        println!("{out}");
    } else if outcome.has_failures() {
        eprintln!("orbit verify: drift detected");
        for f in &outcome.round_trip_failures {
            let kind = match &f.kind {
                orbit_state_core::RoundTripFailureKind::ParseFailed(msg) => {
                    format!("parse failed: {msg}")
                }
                orbit_state_core::RoundTripFailureKind::NotByteIdentical => {
                    "not byte-identical (run `orbit canonicalise` to fix in place)".into()
                }
            };
            eprintln!("  {} — {kind}", f.path.display());
        }
        for d in &outcome.index_drift {
            eprintln!("  index: {d}");
        }
    } else {
        println!("orbit verify: clean");
    }

    if outcome.has_failures() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Walk every canonical YAML and rewrite drifted files through the canonical
/// writer. Mirrors `run_verify`'s output shape: human mode prints a one-line
/// summary plus per-failure paths; JSON mode emits a single envelope-shaped
/// line for tooling. Exits non-zero only on parse failures — drift fixed in
/// place is success.
fn run_canonicalise(layout: &OrbitLayout, dry_run: bool, json: bool) -> ExitCode {
    let report: CanonicaliseReport = canonicalise_all(layout, dry_run);

    if json {
        let mut out = String::from("{\"ok\":");
        out.push_str(if report.has_failures() { "false" } else { "true" });
        out.push_str(",\"dry_run\":");
        out.push_str(if dry_run { "true" } else { "false" });
        out.push_str(&format!(
            ",\"rewrote\":{},\"unchanged\":{},\"parse_failed\":[",
            report.rewrote, report.unchanged
        ));
        for (i, (path, msg)) in report.parse_failed.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"path\":{:?},\"error\":{:?}}}",
                path.display().to_string(),
                msg
            ));
        }
        out.push_str("]}");
        println!("{out}");
    } else {
        let verb = if dry_run { "would rewrite" } else { "rewrote" };
        println!(
            "orbit canonicalise: {verb} {} file(s), {} unchanged, {} parse-failed",
            report.rewrote,
            report.unchanged,
            report.parse_failed.len()
        );
        for (path, msg) in &report.parse_failed {
            eprintln!("  parse failed: {} — {msg}", path.display());
        }
    }

    if report.has_failures() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// One-shot per-spec-folder migration per choice 0021. Like canonicalise,
/// this is a fs-level operation that doesn't fit the verb envelope cleanly
/// — handled here directly so its per-move output stays separate.
fn run_migrate_layout(layout: &OrbitLayout, dry_run: bool, json: bool) -> ExitCode {
    let report = orbit_state_core::migrate_spec_layout(layout, dry_run);

    if json {
        let mut out = String::from("{\"ok\":");
        out.push_str(if report.errors.is_empty() {
            "true"
        } else {
            "false"
        });
        out.push_str(",\"dry_run\":");
        out.push_str(if dry_run { "true" } else { "false" });
        out.push_str(&format!(
            ",\"migrated\":{},\"already_folder\":{},\"moves\":{}",
            report.migrated.len(),
            report.already_folder.len(),
            report.moves.len()
        ));
        out.push_str(",\"errors\":[");
        for (i, (path, msg)) in report.errors.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"path\":{:?},\"error\":{:?}}}",
                path.display().to_string(),
                msg
            ));
        }
        out.push_str("]}");
        println!("{out}");
    } else {
        let verb = if dry_run { "would migrate" } else { "migrated" };
        println!(
            "orbit spec migrate-layout: {verb} {} spec(s), {} already in folder shape, {} planned move(s)",
            report.migrated.len(),
            report.already_folder.len(),
            report.moves.len(),
        );
        for spec_id in &report.migrated {
            println!("  {} {spec_id}", if dry_run { "would migrate" } else { "migrated" });
        }
        for (path, msg) in &report.errors {
            eprintln!("  error: {} — {msg}", path.display());
        }
    }

    if report.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Translate the parsed argv into a [`VerbRequest`]. Mostly pure — the only
/// I/O happens for `spec update --ac-check / --ac-uncheck`, which must read
/// the current spec to compute the new acceptance_criteria list. The parity
/// layer's "two independent parsers, same dispatch" property still holds:
/// the AC mutation lives entirely on the CLI side and emits a normal
/// SpecUpdate request that MCP could equivalently produce.
fn build_request(layout: &OrbitLayout, command: &Command) -> Result<VerbRequest, OrbitError> {
    Ok(match command {
        Command::Spec { action } => match action {
            SpecAction::List { status } => VerbRequest::SpecList(SpecListArgs {
                status: status.clone(),
            }),
            SpecAction::Show { id } => VerbRequest::SpecShow(SpecShowArgs { id: id.clone() }),
            SpecAction::Note {
                id,
                body,
                labels,
                timestamp,
            } => VerbRequest::SpecNote(SpecNoteArgs {
                id: id.clone(),
                body: body.clone(),
                labels: labels.clone(),
                timestamp: timestamp.clone(),
            }),
            SpecAction::Create {
                id,
                goal,
                cards,
                labels,
            } => VerbRequest::SpecCreate(SpecCreateArgs {
                id: id.clone(),
                goal: goal.clone(),
                cards: cards.clone(),
                labels: labels.clone(),
                acceptance_criteria: vec![],
            }),
            SpecAction::Update {
                id,
                goal,
                cards,
                labels,
                ac_check,
                ac_uncheck,
            } => {
                let acceptance_criteria = match (ac_check.as_deref(), ac_uncheck.as_deref()) {
                    (None, None) => None,
                    (Some(_), Some(_)) => {
                        return Err(OrbitError::malformed(
                            "spec.update",
                            "--ac-check and --ac-uncheck are mutually exclusive",
                        ));
                    }
                    (target, uncheck_target) => {
                        // Read current spec, flip the named AC, return the full list.
                        let resp = execute(layout, &VerbRequest::SpecShow(SpecShowArgs {
                            id: id.clone(),
                        }))?;
                        let VerbResponse::SpecShow(show) = resp else {
                            return Err(OrbitError::malformed(
                                "spec.update",
                                "spec.show returned unexpected response",
                            ));
                        };
                        let mut acs = show.spec.acceptance_criteria.clone();
                        let (ac_id, want_checked) = match (target, uncheck_target) {
                            (Some(a), None) => (a, true),
                            (None, Some(a)) => (a, false),
                            _ => unreachable!(),
                        };
                        let pos = acs.iter().position(|c| c.id == ac_id).ok_or_else(|| {
                            OrbitError::not_found(
                                "spec.update",
                                format!("AC {ac_id} not found on spec {id}"),
                            )
                        })?;
                        if acs[pos].checked == want_checked {
                            let state = if want_checked { "checked" } else { "unchecked" };
                            return Err(OrbitError::conflict(
                                "spec.update",
                                format!("AC {ac_id} is already {state}"),
                            ));
                        }
                        acs[pos].checked = want_checked;
                        Some(acs)
                    }
                };
                VerbRequest::SpecUpdate(SpecUpdateArgs {
                    id: id.clone(),
                    goal: goal.clone(),
                    cards: cards.clone(),
                    labels: labels.clone(),
                    acceptance_criteria,
                })
            }
            SpecAction::Close { id } => VerbRequest::SpecClose(SpecCloseArgs { id: id.clone() }),
            SpecAction::MigrateLayout { .. } => unreachable!(
                "spec migrate-layout is short-circuited in main() before reaching build_request"
            ),
        },
        Command::Task { action } => match action {
            TaskAction::Open {
                spec_id,
                body,
                labels,
                task_id,
                timestamp,
            } => VerbRequest::TaskOpen(TaskOpenArgs {
                spec_id: spec_id.clone(),
                body: body.clone(),
                labels: labels.clone(),
                task_id: task_id.clone(),
                timestamp: timestamp.clone(),
            }),
            TaskAction::List { spec_id, state } => VerbRequest::TaskList(TaskListArgs {
                spec_id: spec_id.clone(),
                state: state.clone(),
            }),
            TaskAction::Show { spec_id, task_id } => VerbRequest::TaskShow(TaskShowArgs {
                spec_id: spec_id.clone(),
                task_id: task_id.clone(),
            }),
            TaskAction::Ready { spec_id } => VerbRequest::TaskReady(TaskReadyArgs {
                spec_id: spec_id.clone(),
            }),
            TaskAction::Claim {
                spec_id,
                task_id,
                body,
                labels,
                timestamp,
            } => VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: spec_id.clone(),
                task_id: task_id.clone(),
                body: body.clone(),
                labels: labels.clone(),
                timestamp: timestamp.clone(),
            }),
            TaskAction::Update {
                spec_id,
                task_id,
                body,
                labels,
                timestamp,
            } => VerbRequest::TaskUpdate(TaskUpdateArgs {
                spec_id: spec_id.clone(),
                task_id: task_id.clone(),
                body: body.clone(),
                labels: labels.clone(),
                timestamp: timestamp.clone(),
            }),
            TaskAction::Done {
                spec_id,
                task_id,
                body,
                labels,
                timestamp,
            } => VerbRequest::TaskDone(TaskDoneArgs {
                spec_id: spec_id.clone(),
                task_id: task_id.clone(),
                body: body.clone(),
                labels: labels.clone(),
                timestamp: timestamp.clone(),
            }),
        },
        Command::Memory { action } => match action {
            MemoryAction::Remember {
                key,
                body,
                labels,
                timestamp,
            } => VerbRequest::MemoryRemember(MemoryRememberArgs {
                key: key.clone(),
                body: body.clone(),
                labels: labels.clone(),
                timestamp: timestamp.clone(),
            }),
            MemoryAction::List => VerbRequest::MemoryList(MemoryListArgs::default()),
            MemoryAction::Search { query } => VerbRequest::MemorySearch(MemorySearchArgs {
                query: query.clone(),
            }),
        },
        Command::Card { action } => match action {
            CardAction::Show { slug } => VerbRequest::CardShow(CardShowArgs { slug: slug.clone() }),
            CardAction::List { maturity } => VerbRequest::CardList(CardListArgs {
                maturity: maturity.clone(),
            }),
            CardAction::Search { query } => VerbRequest::CardSearch(CardSearchArgs {
                query: query.clone(),
            }),
            CardAction::Tree { slug, depth } => VerbRequest::CardTree(CardTreeArgs {
                slug: slug.clone(),
                depth: *depth,
            }),
            CardAction::Specs { slug } => VerbRequest::CardSpecs(CardSpecsArgs {
                slug: slug.clone(),
            }),
        },
        Command::Choice { action } => match action {
            ChoiceAction::Show { id } => VerbRequest::ChoiceShow(ChoiceShowArgs { id: id.clone() }),
            ChoiceAction::List { status } => VerbRequest::ChoiceList(ChoiceListArgs {
                status: status.clone(),
            }),
            ChoiceAction::Search { query } => VerbRequest::ChoiceSearch(ChoiceSearchArgs {
                query: query.clone(),
            }),
        },
        Command::Session { action } => match action {
            SessionAction::Prime { memory_cap } => VerbRequest::SessionPrime(SessionPrimeArgs {
                memory_cap: *memory_cap,
            }),
        },
        Command::Overview { memory_cap } => VerbRequest::Overview(OverviewArgs {
            memory_cap: *memory_cap,
        }),
        Command::Verify => unreachable!(
            "Command::Verify is short-circuited in main() before reaching build_request"
        ),
        Command::Canonicalise { .. } => unreachable!(
            "Command::Canonicalise is short-circuited in main() before reaching build_request"
        ),
    })
}

/// Human-readable rendering. Best-effort, not stable for parsing — agents
/// should use `--json`.
fn render_human(response: &VerbResponse) {
    match response {
        VerbResponse::SpecList(result) => render_spec_list(result),
        VerbResponse::SpecShow(result) => render_spec_show(result),
        VerbResponse::SpecNote(result) => render_spec_note(result),
        VerbResponse::SpecCreate(result) => render_spec_create(result),
        VerbResponse::SpecUpdate(result) => render_spec_update(result),
        VerbResponse::SpecClose(result) => render_spec_close(result),
        VerbResponse::TaskOpen(result) => render_task_open(result),
        VerbResponse::TaskList(result) | VerbResponse::TaskReady(result) => {
            render_task_list(result)
        }
        VerbResponse::TaskShow(result) => render_task_show(result),
        VerbResponse::TaskClaim(result)
        | VerbResponse::TaskUpdate(result)
        | VerbResponse::TaskDone(result) => render_task_event(result),
        VerbResponse::MemoryRemember(result) => render_memory_remember(result),
        VerbResponse::MemoryList(result) | VerbResponse::MemorySearch(result) => {
            render_memory_list(result)
        }
        VerbResponse::CardShow(result) => render_card_show(result),
        VerbResponse::CardList(result) | VerbResponse::CardSearch(result) => {
            render_card_list(result)
        }
        VerbResponse::CardTree(result) => render_card_tree(result),
        VerbResponse::CardSpecs(result) => render_card_specs(result),
        VerbResponse::Overview(result) => render_overview(result),
        VerbResponse::ChoiceShow(result) => render_choice_show(result),
        VerbResponse::ChoiceList(result) | VerbResponse::ChoiceSearch(result) => {
            render_choice_list(result)
        }
        VerbResponse::SessionPrime(result) => render_session_prime(result),
    }
}

fn render_session_prime(result: &SessionPrimeResult) {
    println!("session.prime — bound: {} items", result.item_bound);
    println!();
    if !result.open_specs.is_empty() {
        println!("Open specs ({}):", result.open_specs.len());
        for s in &result.open_specs {
            println!("  {}: {}", s.id, s.goal);
        }
    }
    if !result.memories.is_empty() {
        println!();
        println!("Recent memories ({}):", result.memories.len());
        for m in &result.memories {
            println!("  {}: {}", m.key, first_line(&m.body));
        }
    }
}

fn render_memory_remember(result: &MemoryRememberResult) {
    println!("remembered: {} ({})", result.memory.key, result.memory.timestamp);
}

fn render_memory_list(result: &MemoryListResult) {
    if result.memories.is_empty() {
        println!("(no memories)");
        return;
    }
    for m in &result.memories {
        println!("{}\t{}", m.key, first_line(&m.body));
    }
}

fn render_card_show(result: &CardShowResult) {
    println!("slug:     {}", result.slug);
    println!("feature:  {}", result.card.feature);
    println!("goal:     {}", result.card.goal);
    println!("maturity: {:?}", result.card.maturity);
    if !result.card.specs.is_empty() {
        println!("specs:    {}", result.card.specs.join(", "));
    }
}

fn render_card_tree(result: &CardTreeResult) {
    println!("{} (depth {})", result.root, result.depth);
    if !result.tree.feature.is_empty() {
        println!("  {}", result.tree.feature);
    }
    if result.tree.outgoing.is_empty() && result.tree.incoming.is_empty() {
        println!();
        println!("(no relations)");
        return;
    }
    if !result.tree.outgoing.is_empty() {
        println!();
        println!("outgoing:");
        for edge in &result.tree.outgoing {
            render_tree_edge(edge, "→", 0);
        }
    }
    if !result.tree.incoming.is_empty() {
        println!();
        println!("incoming:");
        for edge in &result.tree.incoming {
            render_tree_edge(edge, "←", 0);
        }
    }
}

fn render_tree_edge(edge: &CardTreeEdge, arrow: &str, indent: usize) {
    let pad = "  ".repeat(indent + 1);
    let truncated = if edge.target.truncated { " …" } else { "" };
    println!(
        "{pad}{arrow} {} {}{truncated}",
        edge.kind, edge.target.slug
    );
    if !edge.target.feature.is_empty() {
        println!("{}  {}", "  ".repeat(indent + 2), edge.target.feature);
    }
    for child in &edge.target.outgoing {
        render_tree_edge(child, "→", indent + 1);
    }
    for child in &edge.target.incoming {
        render_tree_edge(child, "←", indent + 1);
    }
}

fn render_overview(result: &OverviewResult) {
    println!("Open specs: {}", result.open_spec_count);
    for id in &result.recent_open_spec_ids {
        println!("  {id}");
    }
    if result.spec_overflow > 0 {
        println!("  +{} more", result.spec_overflow);
    }
    println!();
    println!(
        "Cards by maturity: planned={}, emerging={}, established={}",
        result.cards_by_maturity.planned,
        result.cards_by_maturity.emerging,
        result.cards_by_maturity.established,
    );
    if let Some(mc) = &result.most_connected_card {
        println!();
        println!(
            "Most-connected card: {} (degree {}) — {}",
            mc.slug, mc.degree, mc.feature
        );
    }
    if !result.orphans.is_empty() {
        println!();
        println!("Orphans ({}):", result.orphans.len() + result.orphan_overflow);
        for slug in &result.orphans {
            println!("  {slug}");
        }
        if result.orphan_overflow > 0 {
            println!("  +{} more", result.orphan_overflow);
        }
    }
    if !result.memories.is_empty() {
        println!();
        println!("Recent memories ({}):", result.memories.len());
        for m in &result.memories {
            println!("  {}: {}", m.key, first_line(&m.body));
        }
    }
}

fn render_card_specs(result: &CardSpecsResult) {
    println!("card: {}", result.root);
    if result.specs.is_empty() {
        println!("(no linked specs)");
        return;
    }
    println!();
    for entry in &result.specs {
        let marker = match (entry.listed_on_card, entry.back_referenced_by_spec) {
            (true, true) => "✓",
            (true, false) => "→",  // card claims spec, spec doesn't back-ref
            (false, true) => "←",  // spec claims card, card doesn't list
            (false, false) => "?", // shouldn't happen in normal flow
        };
        println!(
            "  {} {}\t{}\t{}",
            marker, entry.spec_id, entry.status, entry.spec_path
        );
        if !entry.listed_on_card {
            println!("      drift: spec back-references card, but card.specs[] does not list this spec");
        }
        if !entry.back_referenced_by_spec && entry.status != "missing" && entry.status != "parse-failed" {
            println!("      drift: card lists this spec, but spec.cards[] does not back-reference the card");
        }
        if entry.status == "missing" {
            println!("      drift: card lists this spec, but the spec file is missing on disk");
        }
        if entry.status == "parse-failed" {
            println!("      drift: spec file failed to parse — can't verify back-reference");
        }
    }
}

fn render_card_list(result: &orbit_state_core::CardListResult) {
    if result.cards.is_empty() {
        println!("(no cards)");
        return;
    }
    for c in &result.cards {
        println!("{}\t{}\t{}", c.slug, c.maturity, c.feature);
    }
}

fn render_choice_show(result: &ChoiceShowResult) {
    println!("id:           {}", result.choice.id);
    println!("title:        {}", result.choice.title);
    println!("status:       {:?}", result.choice.status);
    println!("date_created: {}", result.choice.date_created);
    println!();
    println!("{}", result.choice.body);
}

fn render_choice_list(result: &ChoiceListResult) {
    if result.choices.is_empty() {
        println!("(no choices)");
        return;
    }
    for c in &result.choices {
        println!("{}\t{}\t{}", c.id, c.status, c.title);
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

fn render_task_open(result: &TaskOpenResult) {
    println!(
        "opened task {} on {}: {}",
        result.task_id,
        result.event.spec_id,
        result.event.body.as_deref().unwrap_or("")
    );
}

fn render_task_list(result: &TaskListResult) {
    if result.tasks.is_empty() {
        println!("(no tasks)");
        return;
    }
    for t in &result.tasks {
        println!("{}\t{}\t{}\t{}", t.spec_id, t.task_id, t.state, t.body.as_deref().unwrap_or(""));
    }
}

fn render_task_show(result: &TaskShowResult) {
    println!("task:    {}", result.state.task_id);
    println!("spec:    {}", result.state.spec_id);
    println!("state:   {}", result.state.state);
    println!("events:  {}", result.state.event_count);
    for ev in &result.events {
        println!(
            "  {} [{}] {}",
            ev.timestamp,
            event_kind_label(&ev.event),
            ev.body.as_deref().unwrap_or("")
        );
    }
}

fn render_task_event(result: &TaskEventResult) {
    println!(
        "{} task {} ({})",
        event_kind_label(&result.event.event),
        result.event.task_id,
        result.event.timestamp
    );
}

fn event_kind_label(kind: &orbit_state_core::schema::TaskEventKind) -> &'static str {
    use orbit_state_core::schema::TaskEventKind::*;
    match kind {
        Open => "open",
        Claim => "claim",
        Update => "update",
        Done => "done",
    }
}

fn render_spec_note(result: &SpecNoteResult) {
    println!(
        "noted on {}: {} ({})",
        result.note.spec_id, result.note.body, result.note.timestamp
    );
}

fn render_spec_create(result: &SpecCreateResult) {
    println!("created spec {}: {}", result.spec.id, result.spec.goal);
}

fn render_spec_update(result: &SpecUpdateResult) {
    println!("updated spec {}: {}", result.spec.id, result.spec.goal);
}

fn render_spec_close(result: &SpecCloseResult) {
    println!("closed spec {}", result.spec.id);
    if !result.cards_updated.is_empty() {
        println!("cards updated: {}", result.cards_updated.join(", "));
    }
}

fn render_spec_list(result: &SpecListResult) {
    if result.specs.is_empty() {
        println!("(no specs)");
        return;
    }
    // Tab-separated for cheap eyeballing. id, status, goal.
    for s in &result.specs {
        println!("{}\t{}\t{}", s.id, s.status, s.goal);
    }
}

fn render_spec_show(result: &SpecShowResult) {
    let s = &result.spec;
    let status = match s.status {
        orbit_state_core::schema::SpecStatus::Open => "open",
        orbit_state_core::schema::SpecStatus::Closed => "closed",
    };
    println!("id:     {}", s.id);
    println!("status: {status}");
    println!("goal:   {}", s.goal);
    if !s.cards.is_empty() {
        println!("cards:  {}", s.cards.join(", "));
    }
    if !s.labels.is_empty() {
        println!("labels: {}", s.labels.join(", "));
    }
    if !s.acceptance_criteria.is_empty() {
        println!("acceptance:");
        for ac in &s.acceptance_criteria {
            let check = if ac.checked { "x" } else { " " };
            let gate = if ac.gate { " [gate]" } else { "" };
            println!("  [{check}] {}{gate}: {}", ac.id, ac.description);
        }
    }
}
