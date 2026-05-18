//! Verb dispatch surface — single entry point shared by CLI and MCP.
//!
//! Per ac-05: "MCP server and CLI both call same Rust core — state-mutation
//! parity (canonical files + state.db byte-identical), error format
//! `<verb>: <category>: <sentence>`."
//!
//! This module defines:
//! - [`VerbRequest`]   — typed input taxonomy (one variant per verb).
//! - [`VerbResponse`]  — typed output taxonomy (one variant per verb).
//! - [`execute`]       — the single dispatch fn both surfaces call.
//! - [`envelope_ok`] / [`envelope_err`] — wire envelope helpers.
//!
//! Adding a verb is a closed-form change: extend the two enums with matching
//! variants and add a private impl fn dispatched from [`execute`]. Both
//! surfaces (CLI argv parser, MCP JSON-RPC handler) construct `VerbRequest`
//! independently, then call [`execute`] — that's where the parity contract
//! lives. The wire envelope is shared so byte-equal payloads fall out for
//! free as long as both surfaces serialise the same `VerbResponse` with the
//! same helper.
//!
//! v0.1 surface: `spec.list` only. Subsequent ACs (ac-06..11) add the rest.

use crate::atomic::{append_jsonl_line, write_atomic};
use crate::canonical::{parse_json_line, parse_yaml, serialise_json_line, serialise_yaml};
use crate::error::{Error, Result};
use crate::layout::OrbitLayout;
use crate::locks;
use crate::schema::{
    AcceptanceCriterion, Card, Choice, InvocationOutcome, Memory, NoteEvent, Session,
    SkillInvocation, Spec, SpecStatus, TaskEvent, TaskEventKind,
};
use crate::session::{read_session_card, read_session_id};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

// ============================================================================
// Decision log — captured here so it travels with the code.
// ============================================================================
//
// Wire shape vs on-disk shape: the response wraps `schema::Spec` directly
// for now (`SpecShowResult { spec: Spec }`). On-disk and wire are isomorphic
// at v0.1. If they diverge later (e.g. wire wants resolved derived fields
// like aggregated note count), the wrapper struct gives us the seam to
// project without breaking the wire contract.

// ============================================================================
// Request / Response taxonomy
// ============================================================================

/// Typed verb request. Tagged on the wire as `{"verb": "<name>", "args": {...}}`
/// so the MCP `tools/call` translation is trivial:
///
/// ```text
/// MCP {name: "spec.list", arguments: {...}} → {"verb": "spec.list", "args": {...}} → VerbRequest
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verb", content = "args")]
pub enum VerbRequest {
    #[serde(rename = "spec.list")]
    SpecList(SpecListArgs),
    #[serde(rename = "spec.show")]
    SpecShow(SpecShowArgs),
    #[serde(rename = "spec.note")]
    SpecNote(SpecNoteArgs),
    #[serde(rename = "spec.create")]
    SpecCreate(SpecCreateArgs),
    #[serde(rename = "spec.update")]
    SpecUpdate(SpecUpdateArgs),
    #[serde(rename = "spec.close")]
    SpecClose(SpecCloseArgs),
    #[serde(rename = "task.open")]
    TaskOpen(TaskOpenArgs),
    #[serde(rename = "task.list")]
    TaskList(TaskListArgs),
    #[serde(rename = "task.show")]
    TaskShow(TaskShowArgs),
    #[serde(rename = "task.ready")]
    TaskReady(TaskReadyArgs),
    #[serde(rename = "task.claim")]
    TaskClaim(TaskClaimArgs),
    #[serde(rename = "task.update")]
    TaskUpdate(TaskUpdateArgs),
    #[serde(rename = "task.done")]
    TaskDone(TaskDoneArgs),
    #[serde(rename = "memory.remember")]
    MemoryRemember(MemoryRememberArgs),
    #[serde(rename = "memory.list")]
    MemoryList(MemoryListArgs),
    #[serde(rename = "memory.search")]
    MemorySearch(MemorySearchArgs),
    #[serde(rename = "card.show")]
    CardShow(CardShowArgs),
    #[serde(rename = "card.list")]
    CardList(CardListArgs),
    #[serde(rename = "card.search")]
    CardSearch(CardSearchArgs),
    #[serde(rename = "card.tree")]
    CardTree(CardTreeArgs),
    #[serde(rename = "card.specs")]
    CardSpecs(CardSpecsArgs),
    #[serde(rename = "overview")]
    Overview(OverviewArgs),
    #[serde(rename = "graph")]
    Graph(GraphArgs),
    #[serde(rename = "audit.drift")]
    AuditDrift(AuditDriftArgs),
    #[serde(rename = "audit.topology")]
    AuditTopology(AuditTopologyArgs),
    #[serde(rename = "choice.show")]
    ChoiceShow(ChoiceShowArgs),
    #[serde(rename = "choice.list")]
    ChoiceList(ChoiceListArgs),
    #[serde(rename = "choice.search")]
    ChoiceSearch(ChoiceSearchArgs),
    #[serde(rename = "session.prime")]
    SessionPrime(SessionPrimeArgs),
    #[serde(rename = "session.start")]
    SessionStart(SessionStartArgs),
    #[serde(rename = "session.distill")]
    SessionDistill(SessionDistillArgs),
    #[serde(rename = "session.set-card")]
    SessionSetCard(SessionSetCardArgs),
    #[serde(rename = "session.handover")]
    SessionHandover(SessionHandoverArgs),
    #[serde(rename = "skill.record-invocation")]
    SkillRecordInvocation(SkillRecordInvocationArgs),
    #[serde(rename = "skill.recurrence")]
    SkillRecurrence(SkillRecurrenceArgs),
}

/// Args for `spec.list`. Optional `status` filter; further filters land later.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecListArgs {
    /// Restrict to specs in this status. Must be `"open"` or `"closed"` if
    /// provided. Empty string and other values are rejected as malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Args for `spec.show` — locate the spec by id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecShowArgs {
    pub id: String,
}

/// Args for `spec.create` — write a new spec file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecCreateArgs {
    pub id: String,
    pub goal: String,
    /// Cards this spec advances. Empty list is legal but unusual.
    #[serde(default)]
    pub cards: Vec<String>,
    /// Free-text labels (e.g. `spec`, `experimental`).
    #[serde(default)]
    pub labels: Vec<String>,
    /// Initial acceptance criteria — usually empty at creation; populated
    /// via spec.update once the spec is designed.
    #[serde(default)]
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
}

/// Args for `spec.update` — modify fields on an existing spec. Only the
/// fields included in the args are applied; omitted fields keep prior
/// values. Status changes go through `spec.close` (which has transactional
/// card-linkage logic), not here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecUpdateArgs {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<Vec<AcceptanceCriterion>>,
}

/// Args for `spec.close` — transition status to `closed` and append the
/// spec's path to every linked card's `specs` array atomically.
///
/// `force` bypasses the unchecked-AC pre-flight added by spec
/// 2026-05-13-spec-close-ac-preflight (ac-02 / ac-03). It does not bypass
/// the unfinished-tasks guard or the already-closed guard.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecCloseArgs {
    pub id: String,
    /// When true, close even if non-time-gated ACs remain unchecked.
    /// The bypassed AC ids surface in `SpecCloseResult.forced_unchecked`
    /// so the audit trail is preserved in the structured response.
    #[serde(default)]
    pub force: bool,
}

// ----------------------------------------------------------------------------
// Task verb args (ac-07)
// ----------------------------------------------------------------------------

/// Args for `task.open` — append an Open event creating a new task under
/// `<spec_id>.tasks.jsonl`. Substrate generates `task_id` if not supplied;
/// callers supply one for migrations or tests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskOpenArgs {
    pub spec_id: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Args for `task.list` — list tasks (current state per task_id) for one
/// spec, or all specs if `spec_id` is None.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskListArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_id: Option<String>,
    /// Filter by current state (`open`, `claim`, `update`, `done`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Args for `task.show` — show one task with its full event history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskShowArgs {
    pub spec_id: String,
    pub task_id: String,
}

/// Args for `task.ready` — list tasks whose last event is Open (claimable).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskReadyArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_id: Option<String>,
}

/// Args for `task.claim` — append a Claim event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskClaimArgs {
    pub spec_id: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Args for `task.update` — append an Update event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskUpdateArgs {
    pub spec_id: String,
    pub task_id: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Args for `task.done` — append a Done event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskDoneArgs {
    pub spec_id: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

// ----------------------------------------------------------------------------
// Memory / card / choice verb args (ac-08, ac-09, ac-10)
// ----------------------------------------------------------------------------

/// Args for `memory.remember` — upsert a memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryRememberArgs {
    pub key: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Suppress the topology-label nudge even when the labels list
    /// includes `topology`. Per spec 2026-05-18-topology-substrate-wires
    /// ac-04. Defaults to false; mirrors the `--no-edit` / `--no-verify`
    /// naming convention.
    #[serde(default)]
    pub no_nudge: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryListArgs {}

/// Args for `memory.search` — substring (case-insensitive) over body + labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchArgs {
    pub query: String,
}

/// Args for `card.show`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CardShowArgs {
    pub slug: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CardListArgs {
    /// Filter by maturity (`planned`, `emerging`, `established`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maturity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CardSearchArgs {
    pub query: String,
}

/// Args for `card.tree` — render the local subgraph from a card.
///
/// `depth` defaults to 2 (one hop in each direction expanded) and may be 0
/// (returns just the root with no edges). The graph is cycle-safe: a slug
/// already seen on the current expansion path is rendered as a truncated
/// node so the structure doesn't recurse indefinitely.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CardTreeArgs {
    pub slug: String,
    #[serde(default = "default_card_tree_depth")]
    pub depth: u32,
}

fn default_card_tree_depth() -> u32 {
    2
}

/// Args for `card.specs` — list specs that advance a card, with bidirectional
/// link health.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CardSpecsArgs {
    pub slug: String,
}

/// Args for `overview` — single-screen project synthesis.
///
/// All output is bounded. The optional `memory_cap` mirrors `session.prime`
/// (default K=10) and applies uniformly to memories, the recent-open-spec
/// list, and the orphan list so the verb stays single-screen as the project
/// ages.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OverviewArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_cap: Option<usize>,
}

/// Args for `graph` — render the cards/specs graph to mermaid or graphviz.
///
/// The unscoped default render is intentionally permitted to exceed
/// single-screen — it serves the share-or-paste use case, not the synthesis
/// use case (the bounded contract applies to `overview`, not here).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphArgs {
    /// Scope the render to one card and its neighbourhood. When set, the
    /// graph is the union of nodes within `depth` hops of this card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// Depth in hops from `card`. Default 2; only meaningful with `card`.
    #[serde(default = "default_graph_depth")]
    pub depth: u32,
    /// Output format. Default mermaid.
    #[serde(default)]
    pub format: GraphFormat,
}

fn default_graph_depth() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GraphFormat {
    #[default]
    Mermaid,
    Graphviz,
}

/// Args for `audit.drift` — permissive YAML scan that surfaces top-level
/// fields absent from the canonical schema. No flags at v0.1; the verb
/// walks the full substrate.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuditDriftArgs {}

/// Args for `audit.topology` — walks the topology doc named by
/// `.orbit/config.yaml`'s `docs.topology` key and reports drift across
/// three categories (stale_pointer, missing_entry, shape_drift). No
/// flags at v0.1. Per spec 2026-05-18-documentation-topology ac-06.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuditTopologyArgs {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceShowArgs {
    pub id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceListArgs {
    /// Filter by status (`proposed`, `accepted`, `rejected`, `deprecated`, `superseded`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceSearchArgs {
    pub query: String,
}

/// Args for `session.prime` — agent session priming context.
///
/// Per ac-11: bounded output formula `f(N specs, M memories) ≤ 40 +
/// 2*open_specs + min(M,10)`. The K=10 memory cap is enforced here;
/// the per-open-spec bound is structural (each spec contributes one
/// summary to the output, regardless of how heavy it is).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionPrimeArgs {
    /// Override the default memory cap (K=10). Tests use this to verify
    /// the bound is enforced; production callers omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_cap: Option<usize>,
}

// ----------------------------------------------------------------------------
// Session / skill verb args (spec 2026-05-15-agent-learning-loop)
// ----------------------------------------------------------------------------

/// Args for `session.start` — write a session id to `.orbit/.session-id`.
///
/// When `id` is supplied (typically by test fixtures or replay scenarios) it
/// is used verbatim. Otherwise a UUIDv4 is generated. Re-running with no `id`
/// overwrites with a new UUID — the intended "fresh session" semantics.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionStartArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Args for `session.distill` — write or update `.orbit/sessions/<id>.yaml`.
///
/// `session_id` precedence: arg > `ORBIT_SESSION_ID` env > `.orbit/.session-id`.
/// `distillate` is the agent's end-of-session reflection (free text).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionDistillArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub distillate: String,
    /// Optional card slug scoping the distilled session. Resolution
    /// precedence (per spec 2026-05-16-session-handover ac-03): explicit
    /// arg first, else `.orbit/.session-card` fallback, else None. The
    /// id is NOT validated at distill time — validation lives at
    /// `session.set-card` time so the hot path stays cheap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Args for `session.set-card` — validate a card id and write the canonical
/// slug to `.orbit/.session-card` so the next `session.distill` (typically
/// the Stop hook) scopes the session to that card.
///
/// See spec 2026-05-16-session-handover ac-04.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionSetCardArgs {
    pub card_id: String,
}

/// Args for `session.handover` — return the most-recent matching Session.
/// Both fields optional: no `card_id` means "latest across all cards";
/// no `since` means "no lower bound". When the sessions directory is
/// absent or no Session matches, the result envelope carries
/// `handover: null` (NOT an error — same shape as `skill.recurrence`).
///
/// See spec 2026-05-16-session-handover ac-06.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionHandoverArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Args for `skill.record-invocation` — append one row to the skill's stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillRecordInvocationArgs {
    pub skill_id: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Args for `skill.recurrence` — read per-outcome counts for one skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillRecurrenceArgs {
    pub skill_id: String,
    /// RFC 3339 cutoff — only rows with `timestamp >= since` are counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Args for `spec.note` — append a timestamped note to a spec.
///
/// The `timestamp` arg is the documented test/migration seam. Production
/// callers omit it and the substrate stamps RFC 3339 UTC at append time.
/// Migration tools (Migration B in the spec — "bd notes → spec.note events")
/// pre-supply the original bd-recorded timestamp so historical ordering
/// survives the cutover.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecNoteArgs {
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Typed verb response. One variant per verb, mirroring [`VerbRequest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "verb", content = "result")]
pub enum VerbResponse {
    #[serde(rename = "spec.list")]
    SpecList(SpecListResult),
    #[serde(rename = "spec.show")]
    SpecShow(SpecShowResult),
    #[serde(rename = "spec.note")]
    SpecNote(SpecNoteResult),
    #[serde(rename = "spec.create")]
    SpecCreate(SpecCreateResult),
    #[serde(rename = "spec.update")]
    SpecUpdate(SpecUpdateResult),
    #[serde(rename = "spec.close")]
    SpecClose(SpecCloseResult),
    #[serde(rename = "task.open")]
    TaskOpen(TaskOpenResult),
    #[serde(rename = "task.list")]
    TaskList(TaskListResult),
    #[serde(rename = "task.show")]
    TaskShow(TaskShowResult),
    #[serde(rename = "task.ready")]
    TaskReady(TaskListResult),
    #[serde(rename = "task.claim")]
    TaskClaim(TaskEventResult),
    #[serde(rename = "task.update")]
    TaskUpdate(TaskEventResult),
    #[serde(rename = "task.done")]
    TaskDone(TaskEventResult),
    #[serde(rename = "memory.remember")]
    MemoryRemember(MemoryRememberResult),
    #[serde(rename = "memory.list")]
    MemoryList(MemoryListResult),
    #[serde(rename = "memory.search")]
    MemorySearch(MemoryListResult),
    #[serde(rename = "card.show")]
    CardShow(CardShowResult),
    #[serde(rename = "card.list")]
    CardList(CardListResult),
    #[serde(rename = "card.search")]
    CardSearch(CardListResult),
    #[serde(rename = "card.tree")]
    CardTree(CardTreeResult),
    #[serde(rename = "card.specs")]
    CardSpecs(CardSpecsResult),
    #[serde(rename = "overview")]
    Overview(OverviewResult),
    #[serde(rename = "graph")]
    Graph(GraphResult),
    #[serde(rename = "audit.drift")]
    AuditDrift(AuditDriftResult),
    #[serde(rename = "audit.topology")]
    AuditTopology(AuditTopologyResult),
    #[serde(rename = "choice.show")]
    ChoiceShow(ChoiceShowResult),
    #[serde(rename = "choice.list")]
    ChoiceList(ChoiceListResult),
    #[serde(rename = "choice.search")]
    ChoiceSearch(ChoiceListResult),
    #[serde(rename = "session.prime")]
    SessionPrime(SessionPrimeResult),
    #[serde(rename = "session.start")]
    SessionStart(SessionStartResult),
    #[serde(rename = "session.distill")]
    SessionDistill(SessionDistillResult),
    #[serde(rename = "session.set-card")]
    SessionSetCard(SessionSetCardResult),
    #[serde(rename = "session.handover")]
    SessionHandover(SessionHandoverResult),
    #[serde(rename = "skill.record-invocation")]
    SkillRecordInvocation(SkillRecordInvocationResult),
    #[serde(rename = "skill.recurrence")]
    SkillRecurrence(SkillRecurrenceResult),
}

/// Result for `spec.list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecListResult {
    pub specs: Vec<SpecSummary>,
}

/// Result for `spec.show`. Wraps the on-disk Spec; future fields (resolved
/// note count, derived task counts) extend the wrapper without breaking the
/// envelope contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecShowResult {
    pub spec: Spec,
}

/// Result for `spec.note` — echoes the appended event so callers can confirm
/// the substrate-stamped timestamp without re-reading the stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecNoteResult {
    pub note: NoteEvent,
}

/// Result for `spec.create` — echoes the on-disk spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecCreateResult {
    pub spec: Spec,
}

/// Result for `spec.update` — returns the post-update spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecUpdateResult {
    pub spec: Spec,
}

/// Result for `spec.close` — returns the closed spec plus a list of cards
/// whose `specs` array was extended.
///
/// `forced_unchecked` lists ACs that were bypassed via the `force` flag
/// (per spec 2026-05-13-spec-close-ac-preflight ac-03); empty when no
/// bypass occurred. `deferrable_open` lists ACs of deferrable kind
/// (`Ops`/`Observation` per `AcType::blocks_close()`) that remained
/// unchecked at close (spec 2026-05-16-ac-taxonomy ac-02); empty when
/// no deferrable ACs remained open. Both fields use
/// `skip_serializing_if = "Vec::is_empty"` so happy-path responses
/// remain byte-identical to the pre-change shape.
///
/// Note: this struct intentionally does NOT carry `deny_unknown_fields`,
/// preserving forward-additive read compatibility for callers that
/// cache an older response shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecCloseResult {
    pub spec: Spec,
    pub cards_updated: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forced_unchecked: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferrable_open: Vec<String>,
    /// Topology drift entries for subsystems the closing spec text touched.
    /// Word-boundary match (regex `\b<regex::escape(subsystem)>\b`,
    /// case-insensitive) of subsystem names ≥ 5 characters against the
    /// concatenation of `spec.yaml + interview.md + design-note.md`
    /// (each sidecar included when present). Non-blocking — closure
    /// proceeds with exit 0; this field is informational. Empty (and
    /// `skip_serializing_if`-omitted) when not configured or when no
    /// matches exist. Per spec 2026-05-18-topology-substrate-wires ac-03.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topology_warnings: Vec<TopologyDriftEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRememberResult {
    pub memory: Memory,
    /// Advisory nudge populated when the stored memory carried the
    /// canonical `topology` label and the caller did not pass
    /// `--no-nudge`. Non-blocking — the memory still stored. Per spec
    /// 2026-05-18-topology-substrate-wires ac-04.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nudge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryListResult {
    pub memories: Vec<Memory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardShowResult {
    pub slug: String,
    pub card: Card,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardListResult {
    pub cards: Vec<CardSummary>,
}

/// Projection of a card for list/search views.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardSummary {
    pub slug: String,
    pub feature: String,
    pub goal: String,
    pub maturity: String,
}

/// Result for `card.tree` — local subgraph from a root card.
///
/// The `tree` node is the resolved root; its `outgoing` and `incoming`
/// vectors carry the immediate edges (one hop). Each edge's `target` is
/// itself a `CardTreeNode`, recursing up to the configured depth. At the
/// depth boundary or on a revisited slug, `target.truncated = true` and
/// its `outgoing` / `incoming` vectors are empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardTreeResult {
    pub root: String,
    pub depth: u32,
    pub tree: CardTreeNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardTreeNode {
    pub slug: String,
    pub feature: String,
    pub outgoing: Vec<CardTreeEdge>,
    pub incoming: Vec<CardTreeEdge>,
    /// True when this node was reached at the depth boundary or on a
    /// cycle revisit — its edges are intentionally elided.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardTreeEdge {
    pub kind: String,
    pub reason: String,
    pub target: CardTreeNode,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Result for `card.specs` — every spec that's linked to a card by either
/// direction (card → spec via `card.specs[]`, or spec → card via
/// `spec.cards[]`). Each entry names whether both directions agree; one-way
/// references surface as drift.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardSpecsResult {
    pub root: String,
    pub specs: Vec<CardSpecsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardSpecsEntry {
    pub spec_id: String,
    pub spec_path: String,
    /// True if the card's `specs:` array lists this spec.
    pub listed_on_card: bool,
    /// True if the spec's `cards:` array back-references this card.
    pub back_referenced_by_spec: bool,
    /// `open`, `closed`, `missing`, or `parse-failed` — gives the caller
    /// enough context to triage drift without re-reading the spec.
    pub status: String,
}

/// Result for `overview` — single-screen project synthesis. All vectors are
/// bounded by `memory_cap` (default K=10); overflow counters expose how
/// much was elided so the caller can scroll the substrate manually if it
/// matters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverviewResult {
    pub open_spec_count: usize,
    /// Up to K=10 most-recent open spec ids (by id, which is date-prefixed).
    pub recent_open_spec_ids: Vec<String>,
    /// Number of open specs not surfaced because they fell past the cap.
    pub spec_overflow: usize,
    pub cards_by_maturity: CardMaturityCounts,
    pub memories: Vec<Memory>,
    /// Card with the highest degree (outgoing + incoming `relations:` count;
    /// `specs:` entries do NOT contribute). Ties broken by lowest numeric id.
    /// `None` when no card has any relations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_connected_card: Option<MostConnectedCard>,
    /// Cards with `specs: []` AND zero incoming `relations:`. Capped at
    /// K=10; `orphan_overflow` counts the rest.
    pub orphans: Vec<String>,
    pub orphan_overflow: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CardMaturityCounts {
    pub planned: usize,
    pub emerging: usize,
    pub established: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MostConnectedCard {
    pub slug: String,
    pub feature: String,
    pub degree: usize,
}

/// Result for `graph` — the rendered text plus the format it's in. The
/// caller pastes `text` into a markdown block or graphviz tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphResult {
    pub format: String,
    pub text: String,
}

/// Result for `audit.drift` — one entry per unknown top-level field across
/// all walked files. Empty `drift` means the substrate is clean against the
/// canonical schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditDriftResult {
    pub drift: Vec<DriftEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DriftEntry {
    pub path: String,
    pub kind: String,
    pub field: String,
    pub disposition: String,
}

/// Result for `audit.topology` — three states are possible: (a) topology
/// capability not configured (`configured: false`, empty drift), (b)
/// configured and clean (`configured: true`, empty drift), (c) configured
/// with drift (`configured: true`, non-empty drift). Exit code is 0 for
/// all three; consumers discriminate via the envelope, never via `$?`.
/// Per spec 2026-05-18-documentation-topology ac-06.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditTopologyResult {
    /// True when `.orbit/config.yaml` exists AND `docs.topology` is set.
    /// False when either is missing — the topology capability is opt-in.
    pub configured: bool,
    /// Drift entries, one per detected issue. Empty when configured + clean
    /// AND when not configured.
    pub topology_drift: Vec<TopologyDriftEntry>,
}

/// A single drift entry from `audit.topology`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyDriftEntry {
    /// The subsystem name (for stale_pointer / shape_drift) or the
    /// detected codebase directory (for missing_entry).
    pub subsystem: String,
    /// One of: `stale_pointer`, `missing_entry`, `shape_drift`.
    pub drift_kind: String,
    /// Optional detail — the offending path for stale_pointer, the
    /// missing anchor for shape_drift, or empty for missing_entry.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceShowResult {
    pub choice: Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceListResult {
    pub choices: Vec<ChoiceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub date_created: String,
}

/// Result for `session.prime` — agent priming context. Per ac-11:
/// `f(N specs, M memories) ≤ 40 + 2*open_specs + min(M,10)`.
///
/// The bound is "items in the response", not bytes/tokens — agents can size
/// their context separately. Items here are open spec summaries + memory
/// references.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPrimeResult {
    pub open_specs: Vec<SpecSummary>,
    pub memories: Vec<Memory>,
    /// Most-recent Session across all cards (no card filter at prime —
    /// per-card lookup is via `orbit session handover --card <id>`). The
    /// agent reads this before any other action when it's Some. See
    /// spec 2026-05-16-session-handover ac-07.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover: Option<HandoverSummary>,
    /// Hard upper bound on items: 40 + 2*open_specs + min(memory_cap, 10),
    /// plus +1 when `handover` is Some — so clients know the field is in
    /// the bound (otherwise it's invisible to them).
    pub item_bound: usize,
    /// Next-step suggestion. Per tree-views ac-07 this references `orbit
    /// overview` so a fresh session reaches the synthesis layer in one
    /// step — the load-bearing wire from card 0033's surfacing scenario.
    /// When `handover` is Some the prefix sentinel from ac-07 of spec
    /// 2026-05-16-session-handover (`"Read the handover above before any
    /// other action. "`) is joined onto the front so the next agent reads
    /// the handover before the overview.
    pub next_step: String,
    /// Topology drift entries surfaced at session start. `Some` whenever
    /// the topology capability is configured (`audit_topology(...).configured == true`,
    /// i.e. `.orbit/config.yaml` exists AND `docs.topology` is set) —
    /// `Some(vec![])` for the configured + clean case, `Some(non-empty)`
    /// when drift is present. `None` (key omitted via
    /// `skip_serializing_if`) when the topology capability is not
    /// configured. Per spec 2026-05-18-topology-substrate-wires ac-02.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology_drift: Option<Vec<TopologyDriftEntry>>,
}

/// Result for `session.start` — echoes the session id written to disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStartResult {
    pub session_id: String,
    pub path: String,
}

/// Result for `session.distill` — echoes the post-write Session entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDistillResult {
    pub session: Session,
}

/// Result for `session.set-card` — echoes the canonical resolved slug
/// and the path the substrate wrote. See spec 2026-05-16-session-handover
/// ac-04 for the validation + atomic-write contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSetCardResult {
    pub card_id: String,
    pub path: String,
}

/// Per-card session summary surfaced by `session.handover` and embedded
/// in the `session.prime` envelope (ac-07). Subset of `Session` carrying
/// just the orientation-relevant fields — the full entity is on disk for
/// callers who want it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoverSummary {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub distillate: String,
}

/// Result for `session.handover` — the most-recent matching Session, or
/// `None` when no Session matches. See spec 2026-05-16-session-handover
/// ac-06 for the no-match-is-not-an-error contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHandoverResult {
    pub handover: Option<HandoverSummary>,
}

/// Result for `skill.record-invocation` — echoes the appended row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRecordInvocationResult {
    pub invocation: SkillInvocation,
}

/// One invocation entry returned by `skill.recurrence`. `correction` is
/// omitted from the wire when the original record had none.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecurrenceInvocation {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
}

/// One outcome bucket — count + the entries that contributed to it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecurrenceBucket {
    pub count: usize,
    pub invocations: Vec<RecurrenceInvocation>,
}

/// Per-outcome breakdown for `skill.recurrence`. Every variant key is always
/// present (even with count 0) so agents can index without first checking
/// for missing keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecurrenceByOutcome {
    pub worked: RecurrenceBucket,
    pub partial: RecurrenceBucket,
    #[serde(rename = "didnt-apply")]
    pub didnt_apply: RecurrenceBucket,
    pub incorrect: RecurrenceBucket,
}

/// Result for `skill.recurrence` — per-outcome counts + invocation entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRecurrenceResult {
    pub skill_id: String,
    pub by_outcome: RecurrenceByOutcome,
    pub total: usize,
}

/// Reduced view of a task — its current state derived from the last event
/// for its task_id, plus the event history count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskState {
    pub task_id: String,
    pub spec_id: String,
    /// Current state — `open`, `claim`, `update`, or `done`.
    pub state: String,
    /// Body from the last event that carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Labels carried on the last event (not aggregated).
    #[serde(default)]
    pub labels: Vec<String>,
    /// Timestamp of the last event.
    pub timestamp: String,
    /// Number of events in this task's history.
    pub event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskOpenResult {
    pub event: TaskEvent,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListResult {
    pub tasks: Vec<TaskState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskShowResult {
    pub state: TaskState,
    pub events: Vec<TaskEvent>,
}

/// Result for the three Claim/Update/Done verbs — each appends one event
/// and echoes it for confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventResult {
    pub event: TaskEvent,
}

/// Projection of a spec for list views — id, goal, status, plus the cards it
/// advances and any labels. Excludes ACs and other heavy fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecSummary {
    pub id: String,
    pub goal: String,
    pub status: String,
    #[serde(default)]
    pub cards: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

// ============================================================================
// Dispatch
// ============================================================================

/// Dispatch a verb against the layout. The single entry point both CLI and
/// MCP call — the architectural guarantee from ac-05 lives here.
pub fn execute(layout: &OrbitLayout, request: &VerbRequest) -> Result<VerbResponse> {
    match request {
        VerbRequest::SpecList(args) => spec_list(layout, args).map(VerbResponse::SpecList),
        VerbRequest::SpecShow(args) => spec_show(layout, args).map(VerbResponse::SpecShow),
        VerbRequest::SpecNote(args) => spec_note(layout, args).map(VerbResponse::SpecNote),
        VerbRequest::SpecCreate(args) => spec_create(layout, args).map(VerbResponse::SpecCreate),
        VerbRequest::SpecUpdate(args) => spec_update(layout, args).map(VerbResponse::SpecUpdate),
        VerbRequest::SpecClose(args) => spec_close(layout, args).map(VerbResponse::SpecClose),
        VerbRequest::TaskOpen(args) => task_open(layout, args).map(VerbResponse::TaskOpen),
        VerbRequest::TaskList(args) => task_list(layout, args).map(VerbResponse::TaskList),
        VerbRequest::TaskShow(args) => task_show(layout, args).map(VerbResponse::TaskShow),
        VerbRequest::TaskReady(args) => task_ready(layout, args).map(VerbResponse::TaskReady),
        VerbRequest::TaskClaim(args) => task_claim(layout, args).map(VerbResponse::TaskClaim),
        VerbRequest::TaskUpdate(args) => task_update(layout, args).map(VerbResponse::TaskUpdate),
        VerbRequest::TaskDone(args) => task_done(layout, args).map(VerbResponse::TaskDone),
        VerbRequest::MemoryRemember(args) => {
            memory_remember(layout, args).map(VerbResponse::MemoryRemember)
        }
        VerbRequest::MemoryList(args) => memory_list(layout, args).map(VerbResponse::MemoryList),
        VerbRequest::MemorySearch(args) => {
            memory_search(layout, args).map(VerbResponse::MemorySearch)
        }
        VerbRequest::CardShow(args) => card_show(layout, args).map(VerbResponse::CardShow),
        VerbRequest::CardList(args) => card_list(layout, args).map(VerbResponse::CardList),
        VerbRequest::CardSearch(args) => card_search(layout, args).map(VerbResponse::CardSearch),
        VerbRequest::CardTree(args) => card_tree(layout, args).map(VerbResponse::CardTree),
        VerbRequest::CardSpecs(args) => card_specs(layout, args).map(VerbResponse::CardSpecs),
        VerbRequest::Overview(args) => overview(layout, args).map(VerbResponse::Overview),
        VerbRequest::Graph(args) => graph(layout, args).map(VerbResponse::Graph),
        VerbRequest::AuditDrift(args) => audit_drift(layout, args).map(VerbResponse::AuditDrift),
        VerbRequest::AuditTopology(args) => {
            audit_topology(layout, args).map(VerbResponse::AuditTopology)
        }
        VerbRequest::ChoiceShow(args) => choice_show(layout, args).map(VerbResponse::ChoiceShow),
        VerbRequest::ChoiceList(args) => choice_list(layout, args).map(VerbResponse::ChoiceList),
        VerbRequest::ChoiceSearch(args) => {
            choice_search(layout, args).map(VerbResponse::ChoiceSearch)
        }
        VerbRequest::SessionPrime(args) => {
            session_prime(layout, args).map(VerbResponse::SessionPrime)
        }
        VerbRequest::SessionStart(args) => {
            session_start(layout, args).map(VerbResponse::SessionStart)
        }
        VerbRequest::SessionDistill(args) => {
            session_distill(layout, args).map(VerbResponse::SessionDistill)
        }
        VerbRequest::SessionSetCard(args) => {
            session_set_card(layout, args).map(VerbResponse::SessionSetCard)
        }
        VerbRequest::SessionHandover(args) => {
            session_handover(layout, args).map(VerbResponse::SessionHandover)
        }
        VerbRequest::SkillRecordInvocation(args) => {
            skill_record_invocation(layout, args).map(VerbResponse::SkillRecordInvocation)
        }
        VerbRequest::SkillRecurrence(args) => {
            skill_recurrence(layout, args).map(VerbResponse::SkillRecurrence)
        }
    }
}

// ============================================================================
// Verb implementations
// ============================================================================

/// `spec.list` — enumerate spec files under `.orbit/specs/`, sorted by id.
///
/// Reads files directly (not the index). Reading from files is correct and
/// deterministic; once the index proves out for write paths, read verbs can
/// switch to index-backed for performance. ac-05 does not require index reads.
fn spec_list(layout: &OrbitLayout, args: &SpecListArgs) -> Result<SpecListResult> {
    const VERB: &str = "spec.list";

    if let Some(s) = args.status.as_deref() {
        if !matches!(s, "open" | "closed") {
            return Err(Error::malformed(
                VERB,
                format!("status must be 'open' or 'closed', got '{s}'"),
            ));
        }
    }

    let files = layout
        .list_spec_files()
        .map_err(|e| Error::unavailable(VERB, format!("list specs dir: {e}")))?;

    let mut specs = Vec::with_capacity(files.len());
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| {
            Error::unavailable(VERB, format!("read {}: {e}", path.display()))
        })?;
        let spec: Spec = parse_yaml(&text).map_err(|mut e| {
            // The canonical layer tags errors with verb="canonical"; re-tag to
            // the calling verb so the on-wire error format is correct.
            e.verb = VERB.into();
            e
        })?;
        let status = match spec.status {
            SpecStatus::Open => "open",
            SpecStatus::Closed => "closed",
        };
        if let Some(filter) = args.status.as_deref() {
            if status != filter {
                continue;
            }
        }
        specs.push(SpecSummary {
            id: spec.id,
            goal: spec.goal,
            status: status.into(),
            cards: spec.cards,
            labels: spec.labels,
        });
    }

    specs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(SpecListResult { specs })
}

/// `spec.note` — append a note event to a spec's notes JSONL stream.
///
/// Locking: acquires the spec's lock so concurrent appends serialise. The
/// raw write itself is POSIX-O_APPEND atomic, but the lock guarantees a
/// well-defined append order across multiple writers.
fn spec_note(layout: &OrbitLayout, args: &SpecNoteArgs) -> Result<SpecNoteResult> {
    const VERB: &str = "spec.note";

    if args.id.is_empty() {
        return Err(Error::malformed(VERB, "id must not be empty"));
    }
    if args.id.contains('/') || args.id.contains('\\') || args.id.contains("..") {
        return Err(Error::malformed(
            VERB,
            format!("id must not contain path separators or '..': '{}'", args.id),
        ));
    }
    if args.body.is_empty() {
        return Err(Error::malformed(VERB, "body must not be empty"));
    }

    // Spec must exist before we can attach a note to it.
    let spec_path = layout.spec_file(&args.id);
    if !spec_path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no spec at {}", spec_path.display()),
        ));
    }

    let timestamp = match &args.timestamp {
        Some(t) => t.clone(),
        None => current_rfc3339_utc().map_err(|e| {
            Error::unavailable(VERB, format!("substrate timestamp generation failed: {e}"))
        })?,
    };

    let event = NoteEvent {
        spec_id: args.id.clone(),
        body: args.body.clone(),
        labels: args.labels.clone(),
        timestamp,
    };

    // Acquire the spec lock for the append. Reads of the same stream don't
    // need this — see ac-03's "reads do not require lock acquisition" rule.
    let lock_key = format!("spec-{}", args.id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    // serialise_json_line guarantees a trailing newline, which append_jsonl_line
    // requires.
    let line = serialise_json_line(&event).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    let stream_path = layout.notes_stream(&args.id);
    append_jsonl_line(&stream_path, &line).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SpecNoteResult { note: event })
}

/// `spec.create` — write a new spec.yaml file.
///
/// Conflict if a spec with that id already exists. Lock is acquired so two
/// concurrent creates can't race.
fn spec_create(layout: &OrbitLayout, args: &SpecCreateArgs) -> Result<SpecCreateResult> {
    const VERB: &str = "spec.create";

    validate_spec_id(VERB, &args.id)?;
    if args.goal.is_empty() {
        return Err(Error::malformed(VERB, "goal must not be empty"));
    }

    let lock_key = format!("spec-{}", args.id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    let path = layout.spec_file(&args.id);
    if path.exists() {
        return Err(Error::conflict(
            VERB,
            format!("spec already exists at {}", path.display()),
        ));
    }
    layout
        .ensure_dirs()
        .map_err(|e| Error::unavailable(VERB, format!("ensure dirs: {e}")))?;
    layout
        .ensure_spec_dir(&args.id)
        .map_err(|e| Error::unavailable(VERB, format!("ensure spec dir: {e}")))?;

    let spec = Spec {
        id: args.id.clone(),
        goal: args.goal.clone(),
        cards: args.cards.clone(),
        status: SpecStatus::Open,
        labels: args.labels.clone(),
        acceptance_criteria: args.acceptance_criteria.clone(),
    };
    let yaml = serialise_yaml(&spec).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    write_atomic(&path, yaml.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SpecCreateResult { spec })
}

/// `spec.update` — modify fields on an existing spec. Status changes are
/// not allowed here; `spec.close` owns that transition.
fn spec_update(layout: &OrbitLayout, args: &SpecUpdateArgs) -> Result<SpecUpdateResult> {
    const VERB: &str = "spec.update";

    validate_spec_id(VERB, &args.id)?;

    let lock_key = format!("spec-{}", args.id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    let path = layout.spec_file(&args.id);
    if !path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no spec at {}", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", path.display())))?;
    let mut spec: Spec = parse_yaml(&text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    // Apply field-by-field. Empty-goal still rejected (validation, not
    // omission).
    if let Some(goal) = &args.goal {
        if goal.is_empty() {
            return Err(Error::malformed(VERB, "goal must not be empty"));
        }
        spec.goal = goal.clone();
    }
    if let Some(cards) = &args.cards {
        spec.cards = cards.clone();
    }
    if let Some(labels) = &args.labels {
        spec.labels = labels.clone();
    }
    if let Some(acs) = &args.acceptance_criteria {
        spec.acceptance_criteria = acs.clone();
    }

    let yaml = serialise_yaml(&spec).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    write_atomic(&path, yaml.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SpecUpdateResult { spec })
}

/// `spec.close` — flip status to `closed` and transactionally append the
/// spec's path to every linked card's `specs` array.
///
/// Per ac-06: "transactional: either all linked cards update or none do,
/// with the spec remaining open if any update fails." Implementation:
///
/// 1. Acquire spec lock.
/// 2. Read spec; verify status == open.
/// 3. Read each linked card; build the proposed updated card.
///    Validate each (parse round-trip) BEFORE writing anything.
/// 4. Write each updated card atomically. On any failure mid-batch, roll
///    back the cards already written (using the pre-image we cached).
/// 5. If all card writes succeeded, write the closed spec.
/// 6. If the spec write fails after card writes succeeded, roll back cards
///    too — the spec remaining "open" with cards updated is an inconsistent
///    state and we'd rather pay the rollback cost than leave drift.
///
/// `cards_updated` in the result names the cards whose `specs` array now
/// contains this spec's relative path.
fn spec_close(layout: &OrbitLayout, args: &SpecCloseArgs) -> Result<SpecCloseResult> {
    const VERB: &str = "spec.close";

    validate_spec_id(VERB, &args.id)?;

    let lock_key = format!("spec-{}", args.id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    let spec_path = layout.spec_file(&args.id);
    if !spec_path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no spec at {}", spec_path.display()),
        ));
    }
    let spec_text = std::fs::read_to_string(&spec_path)
        .map_err(|e| Error::unavailable(VERB, format!("read spec: {e}")))?;
    let mut spec: Spec = parse_yaml(&spec_text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    if spec.status == SpecStatus::Closed {
        return Err(Error::conflict(VERB, format!("spec '{}' already closed", spec.id)));
    }

    // AC pre-flight (spec 2026-05-13-spec-close-ac-preflight, ac-02 / ac-04;
    // generalised by spec 2026-05-16-ac-taxonomy ac-02). The spec's
    // acceptance_criteria are already in memory from the parse above, so
    // checking them is essentially free — we do this BEFORE the
    // unfinished-tasks check (which requires task-stream IO) so the cheaper
    // guard fails fast. The unfinished-tasks guard below is unchanged in
    // behaviour (ac-06 of the precursor spec).
    //
    // Blocking set: ACs that are unchecked AND of blocking kind
    // (Code/Config/Doc per AcType::blocks_close()). Unchecked deferrable-
    // kind ACs (Ops/Observation) are reported in the result's
    // `deferrable_open` field but do not block close.
    let unchecked_blocking: Vec<&AcceptanceCriterion> = spec
        .acceptance_criteria
        .iter()
        .filter(|ac| !ac.checked && ac.ac_type.blocks_close())
        .collect();
    let deferrable_open: Vec<String> = spec
        .acceptance_criteria
        .iter()
        .filter(|ac| !ac.checked && !ac.ac_type.blocks_close())
        .map(|ac| ac.id.clone())
        .collect();
    if !unchecked_blocking.is_empty() && !args.force {
        let ids: Vec<&str> = unchecked_blocking.iter().map(|ac| ac.id.as_str()).collect();
        let gate_ids: Vec<&str> = unchecked_blocking
            .iter()
            .filter(|ac| ac.gate)
            .map(|ac| ac.id.as_str())
            .collect();
        let gate_suffix = if gate_ids.is_empty() {
            String::new()
        } else {
            format!(" (gate: {})", gate_ids.join(", "))
        };
        return Err(Error::conflict(
            VERB,
            format!(
                "{} unchecked blocking AC(s) in spec '{}': {}{}",
                ids.len(),
                spec.id,
                ids.join(", "),
                gate_suffix,
            ),
        ));
    }
    let forced_unchecked: Vec<String> = if args.force {
        unchecked_blocking.iter().map(|ac| ac.id.clone()).collect()
    } else {
        Vec::new()
    };

    // Per ac-06: spec.close requires every child task to be in state `done`.
    // Read the task stream once; reduce per task; reject if any non-done.
    let task_events = read_task_events(layout, &spec.id).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    let mut by_id: BTreeMap<String, Vec<TaskEvent>> = BTreeMap::new();
    for ev in task_events {
        by_id.entry(ev.task_id.clone()).or_default().push(ev);
    }
    let unfinished: Vec<String> = by_id
        .iter()
        .filter_map(|(id, evs)| {
            evs.last().and_then(|last| {
                if matches!(last.event, TaskEventKind::Done) {
                    None
                } else {
                    Some(id.clone())
                }
            })
        })
        .collect();
    if !unfinished.is_empty() {
        return Err(Error::conflict(
            VERB,
            format!(
                "{} unfinished task(s) under spec '{}': {}",
                unfinished.len(),
                spec.id,
                unfinished.join(", ")
            ),
        ));
    }

    // Reference inserted into each linked card's `specs` array. We use the
    // spec id with the `.orbit/specs/` prefix so the reference stays
    // stable regardless of where the workspace is rooted. Folder-shape
    // layout (choice 0021): `.orbit/specs/<id>/spec.yaml`.
    let spec_ref = format!(".orbit/specs/{}/spec.yaml", spec.id);

    // Phase 1: read every linked card and compute the proposed update.
    // We deliberately collect everything into memory before writing
    // ANYTHING, so a malformed card surfaces before any side effects.
    let mut card_updates: Vec<CardUpdate> = Vec::with_capacity(spec.cards.len());
    for card_slug in &spec.cards {
        validate_card_slug(VERB, card_slug)?;
        let card_path = layout.card_file(card_slug);
        if !card_path.exists() {
            return Err(Error::not_found(
                VERB,
                format!("linked card not found: {} ({})", card_slug, card_path.display()),
            ));
        }
        let pre_image = std::fs::read_to_string(&card_path)
            .map_err(|e| Error::unavailable(VERB, format!("read card {card_slug}: {e}")))?;
        let mut card: crate::schema::Card = parse_yaml(&pre_image).map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;
        // Idempotent: if the spec ref is already present, do nothing for
        // this card (helps if a previous spec.close partially completed).
        let needs_write = !card.specs.contains(&spec_ref);
        if needs_write {
            card.specs.push(spec_ref.clone());
        }
        let post_image = serialise_yaml(&card).map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;
        card_updates.push(CardUpdate {
            slug: card_slug.clone(),
            path: card_path,
            pre_image,
            post_image,
            written: false,
            needs_write,
        });
    }

    // Phase 2: write every card. On any failure, roll back the ones we
    // already wrote.
    for upd in card_updates.iter_mut() {
        if !upd.needs_write {
            continue;
        }
        if let Err(e) = write_atomic(&upd.path, upd.post_image.as_bytes()) {
            rollback_cards(&card_updates);
            let mut tagged = e;
            tagged.verb = VERB.into();
            return Err(tagged);
        }
        upd.written = true;
    }

    // Phase 3: write the closed spec. If this fails, roll back cards.
    spec.status = SpecStatus::Closed;
    let new_yaml = match serialise_yaml(&spec) {
        Ok(y) => y,
        Err(mut e) => {
            rollback_cards(&card_updates);
            e.verb = VERB.into();
            return Err(e);
        }
    };
    if let Err(e) = write_atomic(&spec_path, new_yaml.as_bytes()) {
        rollback_cards(&card_updates);
        let mut tagged = e;
        tagged.verb = VERB.into();
        return Err(tagged);
    }

    let cards_updated: Vec<String> = card_updates
        .iter()
        .filter(|u| u.needs_write)
        .map(|u| u.slug.clone())
        .collect();

    // Topology warnings surface (spec 2026-05-18-topology-substrate-wires
    // ac-03). Concatenate the spec's substantive sidecars
    // (spec.yaml + interview.md + design-note.md, each when present),
    // and word-boundary-match each topology-doc subsystem name against
    // the concatenation. Subsystem names < 5 characters are excluded to
    // suppress false-positives on short common tokens. Names are passed
    // through regex::escape before \b...\b interpolation so
    // metacharacters (dots, hyphens, slashes) match literally. Best
    // effort: a malformed config or unreadable sidecar yields no
    // warnings rather than failing the close.
    let topology_warnings = compute_topology_warnings(layout, &spec.id);

    Ok(SpecCloseResult {
        spec,
        cards_updated,
        forced_unchecked,
        deferrable_open,
        topology_warnings,
    })
}

/// Per ac-03: subsystem-name word-boundary scan across the spec's
/// substantive sidecars. Returns empty when the topology capability is
/// not configured or when no matches exist. Errors swallowed (this is
/// an advisory surface, not a blocking gate).
fn compute_topology_warnings(layout: &OrbitLayout, spec_id: &str) -> Vec<TopologyDriftEntry> {
    let subsystems = load_topology_subsystem_names(layout);
    if subsystems.is_empty() {
        return Vec::new();
    }

    let spec_dir = layout.spec_dir(spec_id);
    let mut text = String::new();
    for sidecar in &["spec.yaml", "interview.md", "design-note.md"] {
        let path = spec_dir.join(sidecar);
        if let Ok(body) = std::fs::read_to_string(&path) {
            text.push_str(&body);
            text.push('\n');
        }
    }
    if text.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<TopologyDriftEntry> = Vec::new();
    for subsystem in subsystems {
        // Length filter — suppress false-positives on short common tokens
        // (memo, spec, ac, ...).
        if subsystem.chars().count() < 5 {
            continue;
        }
        // regex::escape before \b...\b interpolation — subsystem names
        // may contain metacharacters (dots, hyphens, slashes). Case-
        // insensitive via the (?i) inline flag.
        let pattern = format!(r"(?i)\b{}\b", regex::escape(&subsystem));
        let re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if re.is_match(&text) {
            out.push(TopologyDriftEntry {
                subsystem,
                drift_kind: "spec_touch".into(),
                detail: String::new(),
            });
        }
    }
    out
}

/// In-memory record of one card's pre/post image during spec.close.
struct CardUpdate {
    slug: String,
    path: std::path::PathBuf,
    pre_image: String,
    post_image: String,
    written: bool,
    needs_write: bool,
}

/// Restore every card we'd already written back to its pre-image. Best-
/// effort — failures here are logged via the surface error but don't change
/// the outer return value.
fn rollback_cards(updates: &[CardUpdate]) {
    for upd in updates {
        if upd.written {
            // Best-effort restore. Failures here are logged via stderr
            // because they imply a partially-corrupted state we couldn't
            // fully clean up; the caller's error already names the
            // original failure.
            if let Err(e) = write_atomic(&upd.path, upd.pre_image.as_bytes()) {
                eprintln!(
                    "spec.close: rollback failed for card {}: {e} — manual recovery required",
                    upd.slug
                );
            }
        }
    }
}

/// Reject empty IDs, path traversal, and separators. Used by every verb
/// that takes a spec id.
fn validate_spec_id(verb: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::malformed(verb, "id must not be empty"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("id must not contain path separators or '..': '{}'", id),
        ));
    }
    Ok(())
}

/// Same protections for card slugs (cards live in `.orbit/cards/<slug>.yaml`).
fn validate_card_slug(verb: &str, slug: &str) -> Result<()> {
    if slug.is_empty() {
        return Err(Error::malformed(verb, "card slug must not be empty"));
    }
    if slug.contains('/') || slug.contains('\\') || slug.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("card slug must not contain path separators or '..': '{slug}'"),
        ));
    }
    Ok(())
}

/// Per choice 0022: cards and choices accept bare-NNNN as a CLI shorthand.
/// `8` and `0008` both resolve to the unique file in `dir` whose filename
/// starts with `0008-`. Returns `Ok(Some(slug))` on unique match, `Ok(None)`
/// when the query isn't bare-numeric (caller falls back to literal lookup),
/// and an error on zero or multiple matches.
fn resolve_numeric_slug(verb: &str, dir: &Path, query: &str) -> Result<Option<String>> {
    if query.is_empty() || !query.chars().all(|c| c.is_ascii_digit()) || query.len() > 4 {
        return Ok(None);
    }
    let n: u32 = query
        .parse()
        .map_err(|e| Error::malformed(verb, format!("parse `{query}`: {e}")))?;
    let padded = format!("{n:04}-");
    let mut matches: Vec<String> = Vec::new();
    let read = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return Ok(None),
    };
    for entry in read.flatten() {
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if !name.ends_with(".yaml") {
            continue;
        }
        if name.starts_with(&padded) {
            matches.push(name.trim_end_matches(".yaml").to_string());
        }
    }
    match matches.len() {
        0 => Err(Error::not_found(
            verb,
            format!("no entry matching `{padded}*` in {}", dir.display()),
        )),
        1 => Ok(Some(matches.pop().unwrap())),
        _ => {
            matches.sort();
            Err(Error::malformed(
                verb,
                format!(
                    "ambiguous: `{query}` matches {} entries: {}",
                    matches.len(),
                    matches.join(", ")
                ),
            ))
        }
    }
}

// ============================================================================
// Task verbs (ac-07) — append-only JSONL events with last-event-wins state
// reduction. Per ac-07: "Tasks are append-only JSONL events. State =
// last event for that task_id."
// ============================================================================

/// `task.open` — append an Open event creating a new task. Generates a
/// task_id if the caller doesn't supply one.
fn task_open(layout: &OrbitLayout, args: &TaskOpenArgs) -> Result<TaskOpenResult> {
    const VERB: &str = "task.open";
    validate_spec_id(VERB, &args.spec_id)?;
    if args.body.is_empty() {
        return Err(Error::malformed(VERB, "body must not be empty"));
    }

    let spec_path = layout.spec_file(&args.spec_id);
    if !spec_path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no spec at {}", spec_path.display()),
        ));
    }

    let task_id = match &args.task_id {
        Some(id) => {
            validate_task_id(VERB, id)?;
            id.clone()
        }
        None => generate_task_id().map_err(|e| {
            Error::unavailable(VERB, format!("generate task_id: {e}"))
        })?,
    };
    let timestamp = stamp_or(VERB, &args.timestamp)?;

    // Conflict if a task with this id already has events. Reading events for
    // the spec is cheap; the JSONL file is small in v0.1.
    let existing = read_task_events(layout, &args.spec_id)?;
    if existing.iter().any(|e| e.task_id == task_id) {
        return Err(Error::conflict(
            VERB,
            format!("task '{task_id}' already exists in spec '{}'", args.spec_id),
        ));
    }

    let event = TaskEvent {
        task_id: task_id.clone(),
        spec_id: args.spec_id.clone(),
        event: TaskEventKind::Open,
        body: Some(args.body.clone()),
        labels: args.labels.clone(),
        timestamp,
    };
    append_task_event(VERB, layout, &args.spec_id, &event)?;
    Ok(TaskOpenResult { event, task_id })
}

/// `task.list` — current state per task, optionally filtered by state.
fn task_list(layout: &OrbitLayout, args: &TaskListArgs) -> Result<TaskListResult> {
    const VERB: &str = "task.list";
    if let Some(s) = args.state.as_deref() {
        if !matches!(s, "open" | "claim" | "update" | "done") {
            return Err(Error::malformed(
                VERB,
                format!("state must be one of open|claim|update|done, got '{s}'"),
            ));
        }
    }

    let states = collect_task_states(layout, args.spec_id.as_deref(), VERB)?;
    let filtered: Vec<TaskState> = states
        .into_iter()
        .filter(|s| match args.state.as_deref() {
            Some(want) => s.state == want,
            None => true,
        })
        .collect();
    Ok(TaskListResult { tasks: filtered })
}

/// `task.ready` — equivalent to `task.list --state open`.
fn task_ready(layout: &OrbitLayout, args: &TaskReadyArgs) -> Result<TaskListResult> {
    const VERB: &str = "task.ready";
    let states = collect_task_states(layout, args.spec_id.as_deref(), VERB)?;
    Ok(TaskListResult {
        tasks: states.into_iter().filter(|s| s.state == "open").collect(),
    })
}

/// `task.show` — full event history + reduced state for one task.
fn task_show(layout: &OrbitLayout, args: &TaskShowArgs) -> Result<TaskShowResult> {
    const VERB: &str = "task.show";
    validate_spec_id(VERB, &args.spec_id)?;
    validate_task_id(VERB, &args.task_id)?;

    let events = read_task_events(layout, &args.spec_id).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    let task_events: Vec<TaskEvent> = events
        .into_iter()
        .filter(|e| e.task_id == args.task_id)
        .collect();
    if task_events.is_empty() {
        return Err(Error::not_found(
            VERB,
            format!("no task '{}' in spec '{}'", args.task_id, args.spec_id),
        ));
    }
    let state = reduce_task_events(&task_events).expect("non-empty events have a last");
    Ok(TaskShowResult {
        state,
        events: task_events,
    })
}

fn task_claim(layout: &OrbitLayout, args: &TaskClaimArgs) -> Result<TaskEventResult> {
    append_task_lifecycle_event(
        "task.claim",
        layout,
        &args.spec_id,
        &args.task_id,
        TaskEventKind::Claim,
        args.body.clone(),
        args.labels.clone(),
        args.timestamp.clone(),
        |prev_state| {
            // Claim only legal from Open.
            if prev_state != "open" {
                return Err(Error::conflict(
                    "task.claim",
                    format!("task in state '{prev_state}' cannot be claimed; only 'open' tasks are claimable"),
                ));
            }
            Ok(())
        },
    )
}

fn task_update(layout: &OrbitLayout, args: &TaskUpdateArgs) -> Result<TaskEventResult> {
    if args.body.is_empty() {
        return Err(Error::malformed("task.update", "body must not be empty"));
    }
    append_task_lifecycle_event(
        "task.update",
        layout,
        &args.spec_id,
        &args.task_id,
        TaskEventKind::Update,
        Some(args.body.clone()),
        args.labels.clone(),
        args.timestamp.clone(),
        |prev_state| {
            if prev_state == "done" {
                return Err(Error::conflict(
                    "task.update",
                    "task already done; updates are not appended after done",
                ));
            }
            Ok(())
        },
    )
}

fn task_done(layout: &OrbitLayout, args: &TaskDoneArgs) -> Result<TaskEventResult> {
    append_task_lifecycle_event(
        "task.done",
        layout,
        &args.spec_id,
        &args.task_id,
        TaskEventKind::Done,
        args.body.clone(),
        args.labels.clone(),
        args.timestamp.clone(),
        |prev_state| {
            if prev_state == "done" {
                return Err(Error::conflict(
                    "task.done",
                    "task already done",
                ));
            }
            Ok(())
        },
    )
}

/// Shared lifecycle-event append for claim / update / done. Validates the
/// task exists, the prior state allows the transition (via `validate`), then
/// appends the event under the spec lock.
#[allow(clippy::too_many_arguments)]
fn append_task_lifecycle_event(
    verb: &'static str,
    layout: &OrbitLayout,
    spec_id: &str,
    task_id: &str,
    kind: TaskEventKind,
    body: Option<String>,
    labels: Vec<String>,
    timestamp_arg: Option<String>,
    validate: impl FnOnce(&str) -> Result<()>,
) -> Result<TaskEventResult> {
    validate_spec_id(verb, spec_id)?;
    validate_task_id(verb, task_id)?;

    let lock_key = format!("spec-{spec_id}");
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = verb.into();
        e
    })?;

    let events = read_task_events(layout, spec_id).map_err(|mut e| {
        e.verb = verb.into();
        e
    })?;
    let task_events: Vec<&TaskEvent> = events.iter().filter(|e| e.task_id == task_id).collect();
    if task_events.is_empty() {
        return Err(Error::not_found(
            verb,
            format!("no task '{task_id}' in spec '{spec_id}'"),
        ));
    }
    let prev_state = task_event_kind_str(task_events.last().unwrap().event);
    validate(prev_state)?;

    let timestamp = stamp_or(verb, &timestamp_arg)?;
    let event = TaskEvent {
        task_id: task_id.into(),
        spec_id: spec_id.into(),
        event: kind,
        body,
        labels,
        timestamp,
    };
    append_task_event(verb, layout, spec_id, &event)?;
    Ok(TaskEventResult { event })
}

// --- Task helpers ----------------------------------------------------------

/// Read every event in `<spec_id>.tasks.jsonl` in order.
fn read_task_events(layout: &OrbitLayout, spec_id: &str) -> Result<Vec<TaskEvent>> {
    let path = layout.task_stream(spec_id);
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::unavailable("task.read", format!("read {}: {e}", path.display()))
    })?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: TaskEvent = parse_json_line(line).map_err(|mut e| {
            e.verb = "task.read".into();
            e.message = format!("{} (line {})", e.message, i + 1);
            e
        })?;
        out.push(event);
    }
    Ok(out)
}

/// Append a TaskEvent to `<spec_id>.tasks.jsonl`. Caller must hold the spec
/// lock for logical consistency; `append_jsonl_line` provides the byte-level
/// append atomicity.
fn append_task_event(
    verb: &'static str,
    layout: &OrbitLayout,
    spec_id: &str,
    event: &TaskEvent,
) -> Result<()> {
    let line = serialise_json_line(event).map_err(|mut e| {
        e.verb = verb.into();
        e
    })?;
    let path = layout.task_stream(spec_id);
    append_jsonl_line(&path, &line).map_err(|mut e| {
        e.verb = verb.into();
        e
    })
}

/// Reduce an ordered list of events for ONE task to its current state.
fn reduce_task_events(events: &[TaskEvent]) -> Option<TaskState> {
    let last = events.last()?;
    Some(TaskState {
        task_id: last.task_id.clone(),
        spec_id: last.spec_id.clone(),
        state: task_event_kind_str(last.event).into(),
        body: last.body.clone(),
        labels: last.labels.clone(),
        timestamp: last.timestamp.clone(),
        event_count: events.len(),
    })
}

/// Walk every (or one) spec's task stream and reduce each task to its
/// current state. Used by task.list and task.ready.
fn collect_task_states(
    layout: &OrbitLayout,
    spec_id: Option<&str>,
    verb: &'static str,
) -> Result<Vec<TaskState>> {
    let spec_files = match spec_id {
        Some(id) => {
            validate_spec_id(verb, id)?;
            let p = layout.spec_file(id);
            if !p.exists() {
                return Err(Error::not_found(
                    verb,
                    format!("no spec at {}", p.display()),
                ));
            }
            vec![id.to_string()]
        }
        None => {
            // List all spec files; derive ids from their parent folder names
            // — list_spec_files returns `<id>/spec.yaml` paths under choice
            // 0021's folder layout.
            let files = layout
                .list_spec_files()
                .map_err(|e| Error::unavailable(verb, format!("list specs: {e}")))?;
            files
                .iter()
                .filter_map(|p| {
                    p.parent()
                        .and_then(|d| d.file_name())
                        .and_then(|s| s.to_str())
                        .map(String::from)
                })
                .collect()
        }
    };

    let mut all_states = Vec::new();
    for spec_id in spec_files {
        let events = read_task_events(layout, &spec_id).map_err(|mut e| {
            e.verb = verb.into();
            e
        })?;
        // Group events by task_id, preserving order via BTreeMap (deterministic).
        let mut by_id: BTreeMap<String, Vec<TaskEvent>> = BTreeMap::new();
        for ev in events {
            by_id.entry(ev.task_id.clone()).or_default().push(ev);
        }
        for (_, evs) in by_id {
            if let Some(s) = reduce_task_events(&evs) {
                all_states.push(s);
            }
        }
    }

    // Sort for deterministic output.
    all_states.sort_by(|a, b| a.spec_id.cmp(&b.spec_id).then(a.task_id.cmp(&b.task_id)));
    Ok(all_states)
}

fn task_event_kind_str(kind: TaskEventKind) -> &'static str {
    match kind {
        TaskEventKind::Open => "open",
        TaskEventKind::Claim => "claim",
        TaskEventKind::Update => "update",
        TaskEventKind::Done => "done",
    }
}

fn validate_task_id(verb: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::malformed(verb, "task_id must not be empty"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("task_id must not contain path separators or '..': '{id}'"),
        ));
    }
    Ok(())
}

/// Generate a task_id of the shape `t-<8hex><8hex>` using process pid + nanos.
/// Deterministic per process+time, human-readable, no new deps. Collision
/// risk within a single process is bounded by clock resolution; v0.1's
/// single-machine constraint makes this safe.
fn generate_task_id() -> std::result::Result<String, std::time::SystemTimeError> {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    Ok(format!("t-{pid:08x}{nanos:016x}"))
}

/// Use the supplied timestamp if any; otherwise stamp with substrate clock.
fn stamp_or(verb: &str, supplied: &Option<String>) -> Result<String> {
    match supplied {
        Some(t) => Ok(t.clone()),
        None => current_rfc3339_utc()
            .map_err(|e| Error::unavailable(verb, format!("substrate timestamp: {e}"))),
    }
}

// ============================================================================
// Memory verbs (ac-08) — substrate-written entities; cross-session/cross-machine via git.
// ============================================================================

/// Canonical topology-label nudge text — emitted on the
/// `MemoryRememberResult.nudge` field when the stored memory carries
/// the `topology` label and `--no-nudge` is not set. Per spec
/// 2026-05-18-topology-substrate-wires ac-04.
pub const TOPOLOGY_NUDGE: &str = "consider /orb:topology — labelled memories often correspond to subsystems that should be added or updated in the topology doc";

fn memory_remember(layout: &OrbitLayout, args: &MemoryRememberArgs) -> Result<MemoryRememberResult> {
    const VERB: &str = "memory.remember";
    validate_memory_key(VERB, &args.key)?;
    if args.body.is_empty() {
        return Err(Error::malformed(VERB, "body must not be empty"));
    }

    let lock_key = format!("memory-{}", args.key);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    layout
        .ensure_dirs()
        .map_err(|e| Error::unavailable(VERB, format!("ensure dirs: {e}")))?;

    let timestamp = stamp_or(VERB, &args.timestamp)?;
    let memory = Memory {
        key: args.key.clone(),
        body: args.body.clone(),
        timestamp,
        labels: args.labels.clone(),
    };
    let yaml = serialise_yaml(&memory).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    write_atomic(layout.memory_file(&args.key), yaml.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    // Topology-label nudge (ac-04). Fires only when the labels list
    // contains the canonical `topology` label AND the caller did not
    // pass `--no-nudge`. Non-blocking — the memory has already stored.
    let nudge = if !args.no_nudge && args.labels.iter().any(|l| l == "topology") {
        Some(TOPOLOGY_NUDGE.to_string())
    } else {
        None
    };

    Ok(MemoryRememberResult { memory, nudge })
}

fn memory_list(layout: &OrbitLayout, _args: &MemoryListArgs) -> Result<MemoryListResult> {
    const VERB: &str = "memory.list";
    Ok(MemoryListResult {
        memories: read_all_memories(layout, VERB)?,
    })
}

fn memory_search(layout: &OrbitLayout, args: &MemorySearchArgs) -> Result<MemoryListResult> {
    const VERB: &str = "memory.search";
    if args.query.is_empty() {
        return Err(Error::malformed(VERB, "query must not be empty"));
    }
    let needle = args.query.to_lowercase();
    let all = read_all_memories(layout, VERB)?;
    let matched: Vec<Memory> = all
        .into_iter()
        .filter(|m| {
            m.body.to_lowercase().contains(&needle)
                || m.labels.iter().any(|l| l.to_lowercase().contains(&needle))
        })
        .collect();
    Ok(MemoryListResult { memories: matched })
}

fn read_all_memories(layout: &OrbitLayout, verb: &'static str) -> Result<Vec<Memory>> {
    let files = layout
        .list_memory_files()
        .map_err(|e| Error::unavailable(verb, format!("list memories: {e}")))?;
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::unavailable(verb, format!("read {}: {e}", path.display())))?;
        let m: Memory = parse_yaml(&text).map_err(|mut e| {
            e.verb = verb.into();
            e
        })?;
        out.push(m);
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

fn validate_memory_key(verb: &str, key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(Error::malformed(verb, "key must not be empty"));
    }
    if key.contains('/') || key.contains('\\') || key.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("key must not contain path separators or '..': '{key}'"),
        ));
    }
    Ok(())
}

// ============================================================================
// Card verbs (ac-09) — read-only; the only substrate-driven card write is
// the `specs` array append from spec.close, handled there.
// ============================================================================

fn card_show(layout: &OrbitLayout, args: &CardShowArgs) -> Result<CardShowResult> {
    const VERB: &str = "card.show";
    validate_card_slug(VERB, &args.slug)?;
    let resolved = resolve_numeric_slug(VERB, &layout.cards_dir(), &args.slug)?
        .unwrap_or_else(|| args.slug.clone());
    let path = layout.card_file(&resolved);
    if !path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no card at {}", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", path.display())))?;
    let card: Card = parse_yaml(&text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    Ok(CardShowResult {
        slug: resolved,
        card,
    })
}

fn card_list(layout: &OrbitLayout, args: &CardListArgs) -> Result<CardListResult> {
    const VERB: &str = "card.list";
    if let Some(m) = args.maturity.as_deref() {
        if !matches!(m, "planned" | "emerging" | "established") {
            return Err(Error::malformed(
                VERB,
                format!("maturity must be planned|emerging|established, got '{m}'"),
            ));
        }
    }
    let summaries = collect_card_summaries(layout, VERB)?;
    let filtered = match &args.maturity {
        Some(m) => summaries.into_iter().filter(|s| s.maturity == *m).collect(),
        None => summaries,
    };
    Ok(CardListResult { cards: filtered })
}

fn card_search(layout: &OrbitLayout, args: &CardSearchArgs) -> Result<CardListResult> {
    const VERB: &str = "card.search";
    if args.query.is_empty() {
        return Err(Error::malformed(VERB, "query must not be empty"));
    }
    let needle = args.query.to_lowercase();
    let summaries = collect_card_summaries(layout, VERB)?;
    let matched: Vec<CardSummary> = summaries
        .into_iter()
        .filter(|s| {
            s.feature.to_lowercase().contains(&needle)
                || s.goal.to_lowercase().contains(&needle)
                || s.slug.to_lowercase().contains(&needle)
        })
        .collect();
    Ok(CardListResult { cards: matched })
}

fn card_tree(layout: &OrbitLayout, args: &CardTreeArgs) -> Result<CardTreeResult> {
    const VERB: &str = "card.tree";
    validate_card_slug(VERB, &args.slug)?;

    // Resolve the root slug — same prefix-match semantics as card.show.
    let resolved = resolve_numeric_slug(VERB, &layout.cards_dir(), &args.slug)?
        .unwrap_or_else(|| args.slug.clone());
    let root_path = layout.card_file(&resolved);
    if !root_path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no card at {}", root_path.display()),
        ));
    }

    // Load every card once into a slug→Card map, then build forward and
    // reverse edge indexes. Walking cards once keeps the cost linear in
    // card count regardless of tree depth.
    let cards = load_all_cards(layout, VERB)?;
    if !cards.contains_key(&resolved) {
        // Path existed but parse failed earlier — shouldn't happen, but
        // guard against silent divergence between fs and parsed view.
        return Err(Error::not_found(
            VERB,
            format!("card {resolved} not present in loaded card set"),
        ));
    }

    let forward = build_forward_edges(&cards);
    let reverse = build_reverse_edges(&cards);

    let mut visited = std::collections::HashSet::new();
    let tree = expand_card_node(&resolved, &cards, &forward, &reverse, args.depth, &mut visited);

    Ok(CardTreeResult {
        root: resolved,
        depth: args.depth,
        tree,
    })
}

/// Load every card under `.orbit/cards/` into a `slug -> Card` map. Used by
/// `card.tree` to build forward and reverse edge indexes in one pass.
fn load_all_cards(
    layout: &OrbitLayout,
    verb: &'static str,
) -> Result<BTreeMap<String, Card>> {
    let files = layout
        .list_card_files()
        .map_err(|e| Error::unavailable(verb, format!("list cards: {e}")))?;
    let mut out = BTreeMap::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::unavailable(verb, format!("read {}: {e}", path.display())))?;
        let card: Card = parse_yaml(&text).map_err(|mut e| {
            e.verb = verb.into();
            e
        })?;
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                Error::malformed(verb, format!("card path has no stem: {}", path.display()))
            })?
            .to_string();
        out.insert(slug, card);
    }
    Ok(out)
}

/// Forward edges per card: `slug -> Vec<(target_slug, kind, reason)>`.
fn build_forward_edges(
    cards: &BTreeMap<String, Card>,
) -> BTreeMap<String, Vec<(String, String, String)>> {
    let mut out: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for (slug, card) in cards {
        let edges = card
            .relations
            .iter()
            .map(|r| (r.card.clone(), relation_kind_str(&r.kind).into(), r.reason.clone()))
            .collect();
        out.insert(slug.clone(), edges);
    }
    out
}

/// Reverse edges: for each target slug, the list of (source_slug, kind,
/// reason) pointing to it. Resolved against the card set's slug
/// vocabulary; edges to unknown slugs are kept verbatim so the tree
/// surfaces dangling references rather than silently dropping them.
fn build_reverse_edges(
    cards: &BTreeMap<String, Card>,
) -> BTreeMap<String, Vec<(String, String, String)>> {
    let mut out: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for (source, card) in cards {
        for relation in &card.relations {
            out.entry(relation.card.clone()).or_default().push((
                source.clone(),
                relation_kind_str(&relation.kind).into(),
                relation.reason.clone(),
            ));
        }
    }
    out
}

fn relation_kind_str(kind: &crate::schema::RelationKind) -> &'static str {
    use crate::schema::RelationKind;
    match kind {
        RelationKind::DependsOn => "depends-on",
        RelationKind::Feeds => "feeds",
        RelationKind::Supersedes => "supersedes",
        RelationKind::SupersededBy => "superseded-by",
    }
}

/// Recursively expand a node up to `depth` hops. Cycle-safe: re-visiting a
/// slug already on the current expansion path produces a truncated leaf.
fn expand_card_node(
    slug: &str,
    cards: &BTreeMap<String, Card>,
    forward: &BTreeMap<String, Vec<(String, String, String)>>,
    reverse: &BTreeMap<String, Vec<(String, String, String)>>,
    depth: u32,
    visited: &mut std::collections::HashSet<String>,
) -> CardTreeNode {
    let feature = cards
        .get(slug)
        .map(|c| c.feature.clone())
        .unwrap_or_default();

    // Already visited → return a truncated leaf to break the cycle without
    // duplicating downstream edges. The caller still sees the slug and
    // feature; the structure is bounded.
    if visited.contains(slug) {
        return CardTreeNode {
            slug: slug.to_string(),
            feature,
            outgoing: Vec::new(),
            incoming: Vec::new(),
            truncated: true,
        };
    }
    // Depth boundary → leaf node with the slug only, no edges expanded.
    if depth == 0 {
        return CardTreeNode {
            slug: slug.to_string(),
            feature,
            outgoing: Vec::new(),
            incoming: Vec::new(),
            truncated: true,
        };
    }

    visited.insert(slug.to_string());

    let outgoing = forward
        .get(slug)
        .map(|edges| {
            edges
                .iter()
                .map(|(target_slug, kind, reason)| CardTreeEdge {
                    kind: kind.clone(),
                    reason: reason.clone(),
                    target: expand_card_node(target_slug, cards, forward, reverse, depth - 1, visited),
                })
                .collect()
        })
        .unwrap_or_default();

    let incoming = reverse
        .get(slug)
        .map(|edges| {
            edges
                .iter()
                .map(|(source_slug, kind, reason)| CardTreeEdge {
                    kind: kind.clone(),
                    reason: reason.clone(),
                    target: expand_card_node(source_slug, cards, forward, reverse, depth - 1, visited),
                })
                .collect()
        })
        .unwrap_or_default();

    visited.remove(slug);

    CardTreeNode {
        slug: slug.to_string(),
        feature,
        outgoing,
        incoming,
        truncated: false,
    }
}

fn card_specs(layout: &OrbitLayout, args: &CardSpecsArgs) -> Result<CardSpecsResult> {
    const VERB: &str = "card.specs";
    validate_card_slug(VERB, &args.slug)?;
    let resolved = resolve_numeric_slug(VERB, &layout.cards_dir(), &args.slug)?
        .unwrap_or_else(|| args.slug.clone());
    let card_path = layout.card_file(&resolved);
    if !card_path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no card at {}", card_path.display()),
        ));
    }
    let card_text = std::fs::read_to_string(&card_path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", card_path.display())))?;
    let card: Card = parse_yaml(&card_text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    // Map of all known specs on disk: id -> (path, cards array, status).
    // Built once; consulted for forward dereferences and the reverse scan.
    let spec_files = layout
        .list_spec_files()
        .map_err(|e| Error::unavailable(VERB, format!("list specs: {e}")))?;
    let mut specs_on_disk: BTreeMap<String, (String, Vec<String>, String, bool)> = BTreeMap::new();
    for path in spec_files {
        // Per choice 0021 the per-spec folder is `<id>/spec.yaml`. The spec
        // id is the parent directory name.
        let spec_id = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                Error::malformed(
                    VERB,
                    format!("spec path has no parent folder name: {}", path.display()),
                )
            })?
            .to_string();
        let path_str = relativise_spec_path(&path, &layout.root);
        match std::fs::read_to_string(&path)
            .map_err(|e| (e, "read".to_string()))
            .and_then(|t| parse_yaml::<Spec>(&t).map_err(|e| (std::io::Error::other(e.to_string()), "parse".to_string())))
        {
            Ok(spec) => {
                let status = match spec.status {
                    SpecStatus::Open => "open",
                    SpecStatus::Closed => "closed",
                };
                specs_on_disk.insert(spec_id, (path_str, spec.cards, status.to_string(), true));
            }
            Err((_, stage)) => {
                let status = if stage == "read" { "missing" } else { "parse-failed" };
                specs_on_disk.insert(spec_id, (path_str, Vec::new(), status.to_string(), false));
            }
        }
    }

    let mut entries: BTreeMap<String, CardSpecsEntry> = BTreeMap::new();

    // Forward direction: every path the card lists in card.specs[].
    for listed_path in &card.specs {
        let spec_id = spec_id_from_listed_path(listed_path);
        let (path_for_entry, back_ref, status) = match specs_on_disk.get(&spec_id) {
            Some((path, cards, status, parsed)) => {
                let back = *parsed && cards.iter().any(|c| c == &resolved);
                (path.clone(), back, status.clone())
            }
            None => (listed_path.clone(), false, "missing".to_string()),
        };
        entries.insert(
            spec_id.clone(),
            CardSpecsEntry {
                spec_id,
                spec_path: path_for_entry,
                listed_on_card: true,
                back_referenced_by_spec: back_ref,
                status,
            },
        );
    }

    // Reverse direction: every on-disk spec whose cards[] references this
    // card but which isn't already in the entries map (or which is, but with
    // listed_on_card=true already — we only need to upsert the back-ref
    // flag).
    for (spec_id, (path, cards, status, parsed)) in &specs_on_disk {
        if !*parsed {
            continue;
        }
        if cards.iter().any(|c| c == &resolved) {
            entries
                .entry(spec_id.clone())
                .and_modify(|e| {
                    e.back_referenced_by_spec = true;
                })
                .or_insert_with(|| CardSpecsEntry {
                    spec_id: spec_id.clone(),
                    spec_path: path.clone(),
                    listed_on_card: false,
                    back_referenced_by_spec: true,
                    status: status.clone(),
                });
        }
    }

    Ok(CardSpecsResult {
        root: resolved,
        specs: entries.into_values().collect(),
    })
}

/// Render a spec path as a relative `.orbit/specs/<id>/spec.yaml` string for
/// display alongside the human-written form in `card.specs[]`. Falls back to
/// the absolute path if it can't be relativised (e.g. the layout root isn't
/// a prefix — only happens in tests with unusual fixtures).
fn relativise_spec_path(path: &Path, orbit_root: &Path) -> String {
    // orbit_root is the `.orbit/` dir; we want output prefixed `.orbit/...`
    let parent = orbit_root.parent().unwrap_or(orbit_root);
    if let Ok(rel) = path.strip_prefix(parent) {
        return rel.to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

fn overview(layout: &OrbitLayout, args: &OverviewArgs) -> Result<OverviewResult> {
    const VERB: &str = "overview";
    const DEFAULT_CAP: usize = 10;
    let cap = args.memory_cap.unwrap_or(DEFAULT_CAP);

    // Open specs — reuse spec.list, then filter and cap.
    let SpecListResult { specs: all_specs } = spec_list(layout, &SpecListArgs::default())
        .map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;
    let mut open_ids: Vec<String> = all_specs
        .into_iter()
        .filter(|s| s.status == "open")
        .map(|s| s.id)
        .collect();
    open_ids.sort(); // chronological since ids are date-prefixed
    let open_spec_count = open_ids.len();
    let recent_open_spec_ids: Vec<String> = if open_spec_count > cap {
        open_ids.into_iter().rev().take(cap).collect::<Vec<_>>().into_iter().rev().collect()
    } else {
        open_ids
    };
    let spec_overflow = open_spec_count.saturating_sub(recent_open_spec_ids.len());

    // Cards — single pass for maturity counts + degree + orphan detection.
    let cards = load_all_cards(layout, VERB)?;
    let mut maturity = CardMaturityCounts {
        planned: 0,
        emerging: 0,
        established: 0,
    };
    for card in cards.values() {
        match card.maturity {
            crate::schema::CardMaturity::Planned => maturity.planned += 1,
            crate::schema::CardMaturity::Emerging => maturity.emerging += 1,
            crate::schema::CardMaturity::Established => maturity.established += 1,
        }
    }

    // Reverse-edge index — for both "most-connected" (incoming edges count
    // toward degree) and "orphans" (zero incoming relations).
    let reverse = build_reverse_edges(&cards);

    let mut most_connected: Option<MostConnectedCard> = None;
    let mut best_degree: usize = 0;
    for (slug, card) in &cards {
        let outgoing = card.relations.len();
        let incoming = reverse.get(slug).map(Vec::len).unwrap_or(0);
        let degree = outgoing + incoming;
        if degree == 0 {
            continue;
        }
        // New leader if strictly greater, or equal with a lower numeric id.
        let take = degree > best_degree
            || (degree == best_degree
                && most_connected
                    .as_ref()
                    .is_some_and(|c| numeric_prefix(slug) < numeric_prefix(&c.slug)));
        if take || most_connected.is_none() {
            most_connected = Some(MostConnectedCard {
                slug: slug.clone(),
                feature: card.feature.clone(),
                degree,
            });
            best_degree = degree;
        }
    }

    // Orphans — cards with specs: [] AND no incoming relations from other
    // cards. BTreeMap iteration is sorted, so output is deterministic.
    let mut orphans_all: Vec<String> = cards
        .iter()
        .filter(|(slug, card)| {
            card.specs.is_empty() && reverse.get(*slug).map_or(true, Vec::is_empty)
        })
        .map(|(slug, _)| slug.clone())
        .collect();
    let orphan_total = orphans_all.len();
    let orphan_overflow = orphan_total.saturating_sub(cap);
    orphans_all.truncate(cap);

    // Memories — same shape as session.prime: by timestamp DESC, capped.
    let mut memories = read_all_memories(layout, VERB)?;
    memories.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    memories.truncate(cap);

    Ok(OverviewResult {
        open_spec_count,
        recent_open_spec_ids,
        spec_overflow,
        cards_by_maturity: maturity,
        memories,
        most_connected_card: most_connected,
        orphans: orphans_all,
        orphan_overflow,
    })
}

fn audit_drift(layout: &OrbitLayout, _args: &AuditDriftArgs) -> Result<AuditDriftResult> {
    const VERB: &str = "audit.drift";
    const DEFAULT_DISPOSITION: &str = "quarantine";

    let mut drift: Vec<DriftEntry> = Vec::new();

    // Helper: scan one file as untyped YAML, diff its top-level keys
    // against the known field set. parse-failed files surface as a single
    // drift entry with a special field name so callers see the file at all.
    let scan = |path: &Path, kind: &str, known: &[&str], out: &mut Vec<DriftEntry>| -> Result<()> {
        let display_path = relativise_spec_path(path, &layout.root);
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                return Err(Error::unavailable(
                    VERB,
                    format!("read {}: {e}", path.display()),
                ));
            }
        };
        let value: serde_yaml::Value = match serde_yaml::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                out.push(DriftEntry {
                    path: display_path,
                    kind: kind.to_string(),
                    field: format!("<parse-failed: {e}>"),
                    disposition: DEFAULT_DISPOSITION.to_string(),
                });
                return Ok(());
            }
        };
        let mapping = match value.as_mapping() {
            Some(m) => m,
            None => {
                out.push(DriftEntry {
                    path: display_path,
                    kind: kind.to_string(),
                    field: "<root-not-mapping>".into(),
                    disposition: DEFAULT_DISPOSITION.to_string(),
                });
                return Ok(());
            }
        };
        for (key, _) in mapping {
            let key_str = match key.as_str() {
                Some(s) => s,
                None => continue,
            };
            if !known.contains(&key_str) {
                out.push(DriftEntry {
                    path: display_path.clone(),
                    kind: kind.to_string(),
                    field: key_str.to_string(),
                    disposition: DEFAULT_DISPOSITION.to_string(),
                });
            }
        }
        Ok(())
    };

    for path in layout
        .list_card_files()
        .map_err(|e| Error::unavailable(VERB, format!("list cards: {e}")))?
    {
        scan(&path, "card", Card::FIELDS, &mut drift)?;
    }
    for path in layout
        .list_spec_files()
        .map_err(|e| Error::unavailable(VERB, format!("list specs: {e}")))?
    {
        scan(&path, "spec", Spec::FIELDS, &mut drift)?;
    }
    for path in layout
        .list_choice_files()
        .map_err(|e| Error::unavailable(VERB, format!("list choices: {e}")))?
    {
        scan(&path, "choice", Choice::FIELDS, &mut drift)?;
    }
    for path in layout
        .list_memory_files()
        .map_err(|e| Error::unavailable(VERB, format!("list memories: {e}")))?
    {
        scan(&path, "memory", Memory::FIELDS, &mut drift)?;
    }

    Ok(AuditDriftResult { drift })
}

// ----- audit.topology (spec 2026-05-18-documentation-topology ac-06) -----

/// The five anchors a topology entry must carry. Order is normative —
/// `/orb:topology`'s scaffolder writes them in this order. The audit's
/// `shape_drift` detector requires all five with these exact labels.
const TOPOLOGY_ANCHORS: &[&str] = &["code", "decision", "operational", "tests", "what"];

/// Subdirectories under the repo root that the missing_entry heuristic
/// scans. Top-level dirs under these are candidate subsystems.
const TOPOLOGY_SUBSYSTEM_ROOTS: &[&str] = &["src", "crates"];

/// One parsed entry from the topology doc.
#[derive(Debug, Clone)]
struct TopologyEntry {
    subsystem: String,
    anchors: std::collections::BTreeMap<String, String>,
}

/// Parse the topology doc. Looks for `## <subsystem>` headers; for each,
/// captures `- <anchor>: <value>` bullet lines that follow until the next
/// `## ` header. Unrecognised lines between bullets are tolerated.
fn parse_topology_doc(text: &str) -> Vec<TopologyEntry> {
    let mut entries: Vec<TopologyEntry> = Vec::new();
    let mut current: Option<TopologyEntry> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if let Some(subsystem) = line.strip_prefix("## ") {
            if let Some(prev) = current.take() {
                entries.push(prev);
            }
            current = Some(TopologyEntry {
                subsystem: subsystem.trim().to_string(),
                anchors: std::collections::BTreeMap::new(),
            });
            continue;
        }
        if let Some(entry) = current.as_mut() {
            // Bullet shape: `- <anchor>: <value>` with optional leading whitespace.
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("- ") {
                if let Some((anchor, value)) = rest.split_once(':') {
                    let anchor_lc = anchor.trim().to_lowercase();
                    if TOPOLOGY_ANCHORS.contains(&anchor_lc.as_str()) {
                        entry
                            .anchors
                            .insert(anchor_lc, value.trim().to_string());
                    }
                }
            }
        }
    }
    if let Some(prev) = current.take() {
        entries.push(prev);
    }
    entries
}

/// Detect top-level subsystem directories under `src/` / `crates/` from
/// the repo root. Returns a sorted, deduplicated list of directory names.
fn detect_subsystem_dirs(repo_root: &Path) -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for root in TOPOLOGY_SUBSYSTEM_ROOTS {
        let dir = repo_root.join(root);
        if !dir.is_dir() {
            continue;
        }
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    // Skip hidden / build dirs.
                    if name.starts_with('.') || name == "target" || name == "node_modules" {
                        continue;
                    }
                    names.insert(name.to_string());
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Load the parsed topology entries from the doc named by `docs.topology`.
/// Returns an empty vec when the topology capability is not configured or
/// when the topology doc is missing — callers that need the configured /
/// not-configured distinction should call `audit_topology` instead.
///
/// Per spec 2026-05-18-topology-substrate-wires ac-03 — shared helper used
/// by `spec_close`'s topology_warnings heuristic. The audit_topology
/// function consumes the same parse internally but returns drift entries,
/// not the source entries, so it isn't reusable here without exposing the
/// internal shape.
fn load_topology_subsystem_names(layout: &OrbitLayout) -> Vec<String> {
    let config_path = layout.config_file();
    if !config_path.exists() {
        return Vec::new();
    }
    let config_text = match std::fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let config: crate::schema::Config = match serde_yaml::from_str(&config_text) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let topology_rel = match config.docs.as_ref().and_then(|d| d.topology.as_ref()) {
        Some(p) => p.clone(),
        None => return Vec::new(),
    };
    let repo_root = layout.root.parent().unwrap_or(&layout.root);
    let topology_path = repo_root.join(&topology_rel);
    let topology_text = match std::fs::read_to_string(&topology_path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    parse_topology_doc(&topology_text)
        .into_iter()
        .map(|e| e.subsystem)
        .collect()
}

fn audit_topology(
    layout: &OrbitLayout,
    _args: &AuditTopologyArgs,
) -> Result<AuditTopologyResult> {
    const VERB: &str = "audit.topology";

    // 1. Read .orbit/config.yaml. Absence → not configured.
    let config_path = layout.config_file();
    if !config_path.exists() {
        return Ok(AuditTopologyResult {
            configured: false,
            topology_drift: Vec::new(),
        });
    }
    let config_text = std::fs::read_to_string(&config_path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", config_path.display())))?;
    let config: crate::schema::Config = serde_yaml::from_str(&config_text)
        .map_err(|e| Error::malformed(VERB, format!("parse config.yaml: {e}")))?;

    let topology_rel = match config.docs.as_ref().and_then(|d| d.topology.as_ref()) {
        Some(p) => p.clone(),
        None => {
            return Ok(AuditTopologyResult {
                configured: false,
                topology_drift: Vec::new(),
            });
        }
    };

    // 2. Resolve the topology doc path against the REPO root (config
    //    paths are repo-relative, not .orbit-relative).
    let repo_root = layout.root.parent().unwrap_or(&layout.root);
    let topology_path = repo_root.join(&topology_rel);

    let mut drift: Vec<TopologyDriftEntry> = Vec::new();

    // 3. If the topology doc itself doesn't exist, that's a single
    //    stale_pointer drift entry on the config pointer.
    if !topology_path.exists() {
        drift.push(TopologyDriftEntry {
            subsystem: String::new(),
            drift_kind: "stale_pointer".into(),
            detail: format!("docs.topology points to nonexistent {topology_rel}"),
        });
        return Ok(AuditTopologyResult {
            configured: true,
            topology_drift: drift,
        });
    }

    let topology_text = std::fs::read_to_string(&topology_path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", topology_path.display())))?;
    let entries = parse_topology_doc(&topology_text);

    // 4. For each entry: shape_drift (missing anchors) and stale_pointer
    //    (anchor values that name files which don't exist).
    for entry in &entries {
        // shape_drift: missing anchors
        for &anchor in TOPOLOGY_ANCHORS {
            if !entry.anchors.contains_key(anchor) {
                drift.push(TopologyDriftEntry {
                    subsystem: entry.subsystem.clone(),
                    drift_kind: "shape_drift".into(),
                    detail: format!("missing anchor: {anchor}"),
                });
            }
        }
        // stale_pointer: anchor values that look like paths and don't exist.
        // Skip `what:` (it's prose, not a path).
        for (anchor, value) in &entry.anchors {
            if anchor == "what" {
                continue;
            }
            // Skip empty / hand-waved values.
            let v = value.trim();
            if v.is_empty() || v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("n/a") {
                continue;
            }
            let candidate = repo_root.join(v);
            if !candidate.exists() {
                drift.push(TopologyDriftEntry {
                    subsystem: entry.subsystem.clone(),
                    drift_kind: "stale_pointer".into(),
                    detail: format!("{anchor}: {v}"),
                });
            }
        }
    }

    // 5. missing_entry: subsystems detected in the codebase with no entry.
    let documented: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.subsystem.to_lowercase())
        .collect();
    for subsystem in detect_subsystem_dirs(repo_root) {
        if !documented.contains(&subsystem.to_lowercase()) {
            drift.push(TopologyDriftEntry {
                subsystem,
                drift_kind: "missing_entry".into(),
                detail: String::new(),
            });
        }
    }

    Ok(AuditTopologyResult {
        configured: true,
        topology_drift: drift,
    })
}

fn graph(layout: &OrbitLayout, args: &GraphArgs) -> Result<GraphResult> {
    const VERB: &str = "graph";

    let cards = load_all_cards(layout, VERB)?;
    let forward = build_forward_edges(&cards);

    // Decide which cards to include.
    let scope: BTreeMap<String, &Card> = match &args.card {
        Some(query) => {
            validate_card_slug(VERB, query)?;
            let resolved = resolve_numeric_slug(VERB, &layout.cards_dir(), query)?
                .unwrap_or_else(|| query.clone());
            if !cards.contains_key(&resolved) {
                return Err(Error::not_found(
                    VERB,
                    format!("no card at {}", layout.card_file(&resolved).display()),
                ));
            }
            let reverse = build_reverse_edges(&cards);
            let included = bfs_card_neighbourhood(&resolved, &forward, &reverse, args.depth);
            cards
                .iter()
                .filter(|(slug, _)| included.contains(*slug))
                .map(|(s, c)| (s.clone(), c))
                .collect()
        }
        None => cards.iter().map(|(s, c)| (s.clone(), c)).collect(),
    };

    // Card → spec edges come from card.specs[]. Specs become nodes only
    // when at least one in-scope card lists them.
    let mut spec_nodes: BTreeMap<String, String> = BTreeMap::new();
    let mut card_spec_edges: Vec<(String, String)> = Vec::new();
    for (slug, card) in &scope {
        for spec_path in &card.specs {
            let spec_id = spec_id_from_listed_path(spec_path);
            spec_nodes.entry(spec_id.clone()).or_insert_with(|| spec_path.clone());
            card_spec_edges.push((slug.clone(), spec_id));
        }
    }

    let text = match args.format {
        GraphFormat::Mermaid => render_mermaid(&scope, &spec_nodes, &card_spec_edges),
        GraphFormat::Graphviz => render_graphviz(&scope, &spec_nodes, &card_spec_edges),
    };
    let format = match args.format {
        GraphFormat::Mermaid => "mermaid",
        GraphFormat::Graphviz => "graphviz",
    };
    Ok(GraphResult {
        format: format.to_string(),
        text,
    })
}

/// BFS from `root` over forward + reverse edges to gather the set of cards
/// reachable within `depth` hops in either direction. Bounded by
/// HashSet-of-visited; ignores edges to slugs absent from the loaded card
/// set (dangling references).
fn bfs_card_neighbourhood(
    root: &str,
    forward: &BTreeMap<String, Vec<(String, String, String)>>,
    reverse: &BTreeMap<String, Vec<(String, String, String)>>,
    depth: u32,
) -> std::collections::HashSet<String> {
    let mut included = std::collections::HashSet::new();
    let mut frontier: Vec<String> = vec![root.to_string()];
    included.insert(root.to_string());
    for _ in 0..depth {
        let mut next: Vec<String> = Vec::new();
        for slug in &frontier {
            if let Some(edges) = forward.get(slug) {
                for (target, _, _) in edges {
                    if included.insert(target.clone()) {
                        next.push(target.clone());
                    }
                }
            }
            if let Some(edges) = reverse.get(slug) {
                for (source, _, _) in edges {
                    if included.insert(source.clone()) {
                        next.push(source.clone());
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    included
}

/// Sanitise a slug for use as a mermaid node id (alphanumeric + underscore).
fn mermaid_id(prefix: char, slug: &str) -> String {
    let body: String = slug
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{prefix}_{body}")
}

fn render_mermaid(
    cards: &BTreeMap<String, &Card>,
    spec_nodes: &BTreeMap<String, String>,
    card_spec_edges: &[(String, String)],
) -> String {
    let mut out = String::from("graph LR\n");
    // Card nodes.
    for (slug, card) in cards {
        let id = mermaid_id('c', slug);
        let label = format!("{slug}: {}", card.feature);
        out.push_str(&format!("  {id}[\"{label}\"]\n", id = id, label = label.replace('"', "'")));
    }
    // Spec nodes.
    for spec_id in spec_nodes.keys() {
        let id = mermaid_id('s', spec_id);
        out.push_str(&format!("  {id}([\"{spec_id}\"])\n"));
    }
    // Card → card edges (only when both endpoints are in scope).
    for (slug, card) in cards {
        let from = mermaid_id('c', slug);
        for relation in &card.relations {
            if !cards.contains_key(&relation.card) {
                continue;
            }
            let to = mermaid_id('c', &relation.card);
            let label = relation_kind_str(&relation.kind);
            out.push_str(&format!("  {from} -->|{label}| {to}\n"));
        }
    }
    // Card → spec edges.
    for (card_slug, spec_id) in card_spec_edges {
        let from = mermaid_id('c', card_slug);
        let to = mermaid_id('s', spec_id);
        out.push_str(&format!("  {from} -.-> {to}\n"));
    }
    out
}

fn render_graphviz(
    cards: &BTreeMap<String, &Card>,
    spec_nodes: &BTreeMap<String, String>,
    card_spec_edges: &[(String, String)],
) -> String {
    let mut out = String::from("digraph orbit {\n  rankdir=LR;\n");
    for (slug, card) in cards {
        let label = format!("{slug}\\n{}", card.feature).replace('"', "\\\"");
        out.push_str(&format!("  \"{slug}\" [label=\"{label}\", shape=box];\n"));
    }
    for spec_id in spec_nodes.keys() {
        out.push_str(&format!("  \"{spec_id}\" [shape=ellipse];\n"));
    }
    for (slug, card) in cards {
        for relation in &card.relations {
            if !cards.contains_key(&relation.card) {
                continue;
            }
            let label = relation_kind_str(&relation.kind);
            out.push_str(&format!(
                "  \"{slug}\" -> \"{target}\" [label=\"{label}\"];\n",
                target = relation.card
            ));
        }
    }
    for (card_slug, spec_id) in card_spec_edges {
        out.push_str(&format!(
            "  \"{card_slug}\" -> \"{spec_id}\" [style=dashed];\n"
        ));
    }
    out.push_str("}\n");
    out
}

/// Parse the leading numeric prefix of a card slug (e.g. `"0033"` from
/// `"0033-see-the-tree"`). Used as the tie-break for `most-connected card`
/// per ac-03's pinned rule. Returns u32::MAX when no prefix is found so
/// non-numeric slugs sort last (and lose all ties).
fn numeric_prefix(slug: &str) -> u32 {
    let take: String = slug.chars().take_while(|c| c.is_ascii_digit()).collect();
    take.parse().unwrap_or(u32::MAX)
}

/// Extract the spec id from a path string as it appears in `card.specs[]`.
/// The canonical shape is `.orbit/specs/<id>/spec.yaml` (per choice 0021),
/// but legacy values may still appear as `.orbit/specs/<id>.yaml`. Both
/// resolve to `<id>`.
fn spec_id_from_listed_path(listed: &str) -> String {
    // Trim trailing `/spec.yaml` if present.
    let trimmed = listed.trim_end_matches("/spec.yaml");
    let trimmed = trimmed.trim_end_matches(".yaml");
    trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn collect_card_summaries(layout: &OrbitLayout, verb: &'static str) -> Result<Vec<CardSummary>> {
    let files = layout
        .list_card_files()
        .map_err(|e| Error::unavailable(verb, format!("list cards: {e}")))?;
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::unavailable(verb, format!("read {}: {e}", path.display())))?;
        let card: Card = parse_yaml(&text).map_err(|mut e| {
            e.verb = verb.into();
            e
        })?;
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::malformed(verb, format!("card path has no stem: {}", path.display())))?
            .to_string();
        let maturity = match card.maturity {
            crate::schema::CardMaturity::Planned => "planned",
            crate::schema::CardMaturity::Emerging => "emerging",
            crate::schema::CardMaturity::Established => "established",
        };
        out.push(CardSummary {
            slug,
            feature: card.feature,
            goal: card.goal,
            maturity: maturity.into(),
        });
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(out)
}

// ============================================================================
// Choice verbs (ac-10) — read-only; choices are human-written, CI-validated.
// ============================================================================

fn choice_show(layout: &OrbitLayout, args: &ChoiceShowArgs) -> Result<ChoiceShowResult> {
    const VERB: &str = "choice.show";
    if args.id.is_empty() {
        return Err(Error::malformed(VERB, "id must not be empty"));
    }
    if args.id.contains('/') || args.id.contains('\\') || args.id.contains("..") {
        return Err(Error::malformed(
            VERB,
            format!("id must not contain path separators or '..': '{}'", args.id),
        ));
    }
    let resolved = resolve_numeric_slug(VERB, &layout.choices_dir(), &args.id)?
        .unwrap_or_else(|| args.id.clone());
    let path = layout.choice_file(&resolved);
    if !path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no choice at {}", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", path.display())))?;
    let choice: Choice = parse_yaml(&text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    Ok(ChoiceShowResult { choice })
}

fn choice_list(layout: &OrbitLayout, args: &ChoiceListArgs) -> Result<ChoiceListResult> {
    const VERB: &str = "choice.list";
    if let Some(s) = args.status.as_deref() {
        if !matches!(s, "proposed" | "accepted" | "rejected" | "deprecated" | "superseded") {
            return Err(Error::malformed(
                VERB,
                format!(
                    "status must be proposed|accepted|rejected|deprecated|superseded, got '{s}'"
                ),
            ));
        }
    }
    let summaries = collect_choice_summaries(layout, VERB)?;
    let filtered = match &args.status {
        Some(s) => summaries.into_iter().filter(|c| c.status == *s).collect(),
        None => summaries,
    };
    Ok(ChoiceListResult { choices: filtered })
}

fn choice_search(layout: &OrbitLayout, args: &ChoiceSearchArgs) -> Result<ChoiceListResult> {
    const VERB: &str = "choice.search";
    if args.query.is_empty() {
        return Err(Error::malformed(VERB, "query must not be empty"));
    }
    let needle = args.query.to_lowercase();
    // Search hits title or body, so we must read full Choice (not just summary).
    let files = layout
        .list_choice_files()
        .map_err(|e| Error::unavailable(VERB, format!("list choices: {e}")))?;
    let mut matched = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::unavailable(VERB, format!("read {}: {e}", path.display())))?;
        let choice: Choice = parse_yaml(&text).map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;
        if choice.title.to_lowercase().contains(&needle)
            || choice.body.to_lowercase().contains(&needle)
        {
            matched.push(ChoiceSummary {
                id: choice.id,
                title: choice.title,
                status: choice_status_str(&choice.status).into(),
                date_created: choice.date_created,
            });
        }
    }
    matched.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ChoiceListResult { choices: matched })
}

fn collect_choice_summaries(
    layout: &OrbitLayout,
    verb: &'static str,
) -> Result<Vec<ChoiceSummary>> {
    let files = layout
        .list_choice_files()
        .map_err(|e| Error::unavailable(verb, format!("list choices: {e}")))?;
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::unavailable(verb, format!("read {}: {e}", path.display())))?;
        let choice: Choice = parse_yaml(&text).map_err(|mut e| {
            e.verb = verb.into();
            e
        })?;
        out.push(ChoiceSummary {
            id: choice.id,
            title: choice.title,
            status: choice_status_str(&choice.status).into(),
            date_created: choice.date_created,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn choice_status_str(s: &crate::schema::ChoiceStatus) -> &'static str {
    use crate::schema::ChoiceStatus::*;
    match s {
        Proposed => "proposed",
        Accepted => "accepted",
        Rejected => "rejected",
        Deprecated => "deprecated",
        Superseded => "superseded",
    }
}

// ============================================================================
// Session verb (ac-11)
// ============================================================================

/// `session.prime` — agent priming context with bounded output.
///
/// Returns:
/// - All open specs (summaries — id/goal/status/cards/labels)
/// - Up to K memories (default K=10), most recent first
/// - The item bound formula's value, for caller diagnostics
fn session_prime(layout: &OrbitLayout, args: &SessionPrimeArgs) -> Result<SessionPrimeResult> {
    const VERB: &str = "session.prime";
    const DEFAULT_MEMORY_CAP: usize = 10;
    let cap = args.memory_cap.unwrap_or(DEFAULT_MEMORY_CAP);

    // Open specs.
    let SpecListResult { specs: all_specs } =
        spec_list(layout, &SpecListArgs::default())?;
    let open_specs: Vec<SpecSummary> = all_specs
        .into_iter()
        .filter(|s| s.status == "open")
        .collect();

    // Per spec 2026-05-15-agent-learning-loop ac-06: when at least one open
    // spec has a non-empty `labels` field, sort memories first by label-
    // overlap with open-spec labels (descending), then by timestamp DESC,
    // then truncate to cap. When no open spec has labels, sort by timestamp
    // alone (the previous behaviour).
    let open_spec_labels: BTreeSet<String> = open_specs
        .iter()
        .flat_map(|s| s.labels.iter().cloned())
        .collect();
    let use_overlap_sort = !open_spec_labels.is_empty();

    let mut memories = read_all_memories(layout, VERB)?;
    if use_overlap_sort {
        memories.sort_by(|a, b| {
            let a_overlap = a.labels.iter().filter(|l| open_spec_labels.contains(*l)).count();
            let b_overlap = b.labels.iter().filter(|l| open_spec_labels.contains(*l)).count();
            // Higher overlap first; tie-break on timestamp DESC.
            b_overlap.cmp(&a_overlap).then_with(|| b.timestamp.cmp(&a.timestamp))
        });
    } else {
        memories.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    }
    let effective = cap.min(memories.len());
    memories.truncate(effective);

    // Spec 2026-05-16-session-handover ac-07: surface the most-recent
    // Session globally (no card filter at prime — per-card lookup is via
    // `orbit session handover --card <id>`).
    let handover = session_handover(layout, &SessionHandoverArgs::default())?.handover;

    let mut item_bound = 40 + 2 * open_specs.len() + cap.min(DEFAULT_MEMORY_CAP);
    if handover.is_some() {
        item_bound += 1;
    }

    const HANDOVER_PREFIX: &str = "Read the handover above before any other action. ";
    let base_next_step = "Run `orbit overview` for a single-screen project synthesis (open specs, cards-by-maturity, recent memories, most-connected card, orphans).";
    let next_step = if handover.is_some() {
        format!("{HANDOVER_PREFIX}{base_next_step}")
    } else {
        base_next_step.to_string()
    };

    // Topology drift surface (spec 2026-05-18-topology-substrate-wires
    // ac-02). The audit returns `configured: false` when `.orbit/config.yaml`
    // is absent OR when `docs.topology` is unset — both cases collapse to
    // None on the envelope side (skip-on-default). When configured, Some
    // is populated even on the clean path (empty vec → empty array in
    // the envelope, which is the agreed shape distinguishing
    // configured-clean from not-configured).
    let topology_audit = audit_topology(layout, &AuditTopologyArgs::default())?;
    let topology_drift = if topology_audit.configured {
        Some(topology_audit.topology_drift)
    } else {
        None
    };

    Ok(SessionPrimeResult {
        open_specs,
        memories,
        handover,
        item_bound,
        next_step,
        topology_drift,
    })
}

/// `session.start` — generate a session id (UUIDv4 by default) and write it
/// to `.orbit/.session-id` atomically. When `id` is supplied (test fixtures,
/// replay scenarios) it is used verbatim instead.
fn session_start(layout: &OrbitLayout, args: &SessionStartArgs) -> Result<SessionStartResult> {
    const VERB: &str = "session.start";

    let session_id = match &args.id {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(Error::malformed(VERB, "id must not be empty when supplied"));
            }
            if trimmed.contains('\n') || trimmed.contains('\r') {
                return Err(Error::malformed(
                    VERB,
                    "id must not contain newline characters",
                ));
            }
            trimmed.to_string()
        }
        None => uuid::Uuid::new_v4().to_string(),
    };

    layout
        .ensure_dirs()
        .map_err(|e| Error::unavailable(VERB, format!("ensure dirs: {e}")))?;

    let path = layout.session_id_file();
    let mut contents = session_id.clone();
    contents.push('\n');
    write_atomic(&path, contents.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SessionStartResult {
        session_id,
        path: path.display().to_string(),
    })
}

/// `session.distill` — write or update `.orbit/sessions/<id>.yaml` keyed by
/// session id. Idempotent: re-running on the same id preserves `started_at`
/// and advances `ended_at`.
fn session_distill(
    layout: &OrbitLayout,
    args: &SessionDistillArgs,
) -> Result<SessionDistillResult> {
    const VERB: &str = "session.distill";

    let session_id = match args.session_id.as_deref() {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(Error::malformed(VERB, "session_id must not be empty"));
            }
            trimmed.to_string()
        }
        None => read_session_id(layout, VERB)?,
    };

    validate_session_id(VERB, &session_id)?;

    if args.distillate.is_empty() {
        return Err(Error::malformed(VERB, "distillate must not be empty"));
    }

    let lock_key = format!("session-{}", session_id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    std::fs::create_dir_all(layout.sessions_dir())
        .map_err(|e| Error::unavailable(VERB, format!("ensure sessions dir: {e}")))?;

    let now = current_rfc3339_utc().map_err(|e| {
        Error::unavailable(VERB, format!("substrate timestamp generation failed: {e}"))
    })?;
    let path = layout.session_file(&session_id);

    let started_at = if path.exists() {
        let text = std::fs::read_to_string(&path).map_err(|e| {
            Error::unavailable(VERB, format!("read {}: {e}", path.display()))
        })?;
        let existing: Session = parse_yaml(&text).map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;
        existing.started_at
    } else {
        now.clone()
    };

    // Spec 2026-05-16-session-handover ac-03: card_id resolution precedence
    // is explicit arg first, else `.orbit/.session-card` fallback, else None.
    // No validation here — validation lives at `session.set-card` time so the
    // hot path stays cheap. Idempotent latest-write-wins matches the rest of
    // the distill contract.
    let card_id = match args.card_id.as_deref() {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                read_session_card(layout, VERB)?
            } else {
                Some(trimmed.to_string())
            }
        }
        None => read_session_card(layout, VERB)?,
    };

    let session = Session {
        id: session_id,
        started_at,
        ended_at: Some(now),
        distillate: args.distillate.clone(),
        card_id,
        labels: args.labels.clone(),
    };
    let yaml = serialise_yaml(&session).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    write_atomic(&path, yaml.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SessionDistillResult { session })
}

/// `session.set-card` — validate `args.card_id` against the card-lookup
/// prefix-match helper, then write the resolved canonical slug atomically
/// to `.orbit/.session-card`. On unknown card, returns `Error::not_found`
/// and writes nothing. See spec 2026-05-16-session-handover ac-04.
fn session_set_card(
    layout: &OrbitLayout,
    args: &SessionSetCardArgs,
) -> Result<SessionSetCardResult> {
    const VERB: &str = "session.set-card";

    let raw = args.card_id.trim();
    if raw.is_empty() {
        return Err(Error::malformed(VERB, "card_id must not be empty"));
    }
    validate_card_slug(VERB, raw)?;

    // Resolve the slug. resolve_numeric_slug handles bare/padded numeric;
    // a full slug like "0036-session-handover" requires the literal-file
    // existence check below.
    let resolved = match resolve_numeric_slug(VERB, &layout.cards_dir(), raw)? {
        Some(slug) => slug,
        None => raw.to_string(),
    };
    let path = layout.card_file(&resolved);
    if !path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no card matching `{raw}` (looked for {})", path.display()),
        ));
    }

    layout.ensure_dirs().map_err(|e| {
        Error::unavailable(VERB, format!("ensure dirs: {e}"))
    })?;

    let card_path = layout.session_card_file();
    let mut contents = resolved.clone();
    contents.push('\n');
    write_atomic(&card_path, contents.as_bytes()).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SessionSetCardResult {
        card_id: resolved,
        path: card_path.display().to_string(),
    })
}

/// `session.handover` — walk `.orbit/sessions/*.yaml`, filter by `card_id`
/// (when provided) and `started_at >= since` (when provided), and return
/// the session with the maximum `started_at`. Returns `handover: None`
/// when no match — querying for an unrecorded card is a legitimate question
/// per the `skill.recurrence` precedent. See spec 2026-05-16-session-handover
/// ac-06.
fn session_handover(
    layout: &OrbitLayout,
    args: &SessionHandoverArgs,
) -> Result<SessionHandoverResult> {
    const VERB: &str = "session.handover";

    // Resolve a positional/long card id via the same prefix-match helper as
    // session.set-card so the operator can write `--card 36` or `--card 0036`
    // or the full slug.
    let resolved_card = match args.card_id.as_deref() {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                validate_card_slug(VERB, trimmed)?;
                let slug =
                    match resolve_numeric_slug(VERB, &layout.cards_dir(), trimmed)? {
                        Some(s) => s,
                        None => trimmed.to_string(),
                    };
                let card_path = layout.card_file(&slug);
                if !card_path.exists() {
                    return Err(Error::not_found(
                        VERB,
                        format!(
                            "no card matching `{trimmed}` (looked for {})",
                            card_path.display()
                        ),
                    ));
                }
                Some(slug)
            }
        }
        None => None,
    };

    let since = args.since.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let dir = layout.sessions_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionHandoverResult { handover: None });
        }
        Err(e) => {
            return Err(Error::unavailable(
                VERB,
                format!("read {}: {e}", dir.display()),
            ));
        }
    };

    let mut best: Option<Session> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| {
            Error::unavailable(VERB, format!("read {}: {e}", path.display()))
        })?;
        let session: Session = parse_yaml(&text).map_err(|mut e| {
            e.verb = VERB.into();
            e
        })?;

        if let Some(card) = &resolved_card {
            match &session.card_id {
                Some(c) if c == card => {}
                _ => continue,
            }
        }
        if let Some(s) = since {
            if session.started_at.as_str() < s {
                continue;
            }
        }
        match &best {
            Some(b) if b.started_at >= session.started_at => {}
            _ => best = Some(session),
        }
    }

    let handover = best.map(|s| HandoverSummary {
        session_id: s.id,
        card_id: s.card_id,
        started_at: s.started_at,
        ended_at: s.ended_at,
        distillate: s.distillate,
    });
    Ok(SessionHandoverResult { handover })
}

/// `skill.record-invocation` — append one row to
/// `.orbit/skills/<skill_id>.invocations.jsonl`.
fn skill_record_invocation(
    layout: &OrbitLayout,
    args: &SkillRecordInvocationArgs,
) -> Result<SkillRecordInvocationResult> {
    const VERB: &str = "skill.record-invocation";

    validate_skill_id(VERB, &args.skill_id)?;

    let outcome = parse_invocation_outcome(VERB, &args.outcome)?;

    let session_id = match args.session_id.as_deref() {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(Error::malformed(VERB, "session_id must not be empty"));
            }
            trimmed.to_string()
        }
        None => read_session_id(layout, VERB)?,
    };

    let correction = match args.correction.as_deref() {
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s.to_string()),
        None => None,
    };

    let timestamp = match &args.timestamp {
        Some(t) => t.clone(),
        None => current_rfc3339_utc().map_err(|e| {
            Error::unavailable(VERB, format!("substrate timestamp generation failed: {e}"))
        })?,
    };

    let invocation = SkillInvocation {
        skill_id: args.skill_id.clone(),
        session_id,
        outcome,
        correction,
        timestamp,
    };

    let lock_key = format!("skill-{}", args.skill_id);
    let _guard = locks::acquire_default(layout, &lock_key).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    std::fs::create_dir_all(layout.skills_dir())
        .map_err(|e| Error::unavailable(VERB, format!("ensure skills dir: {e}")))?;

    let line = serialise_json_line(&invocation).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    let path = layout.skill_invocations_file(&args.skill_id);
    append_jsonl_line(&path, &line).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;

    Ok(SkillRecordInvocationResult { invocation })
}

/// `skill.recurrence` — read the per-skill invocation stream and bucket rows
/// by outcome. Returns an empty-shape response when the file is absent.
fn skill_recurrence(
    layout: &OrbitLayout,
    args: &SkillRecurrenceArgs,
) -> Result<SkillRecurrenceResult> {
    const VERB: &str = "skill.recurrence";

    validate_skill_id(VERB, &args.skill_id)?;

    let path = layout.skill_invocations_file(&args.skill_id);
    let mut by_outcome = RecurrenceByOutcome::default();
    let mut total = 0usize;

    if path.exists() {
        let text = std::fs::read_to_string(&path).map_err(|e| {
            Error::unavailable(VERB, format!("read {}: {e}", path.display()))
        })?;
        for (lineno, raw) in text.lines().enumerate() {
            if raw.is_empty() {
                continue;
            }
            let invocation: SkillInvocation = parse_json_line(raw).map_err(|mut e| {
                e.verb = VERB.into();
                e.message = format!("{} (line {})", e.message, lineno + 1);
                e
            })?;
            if let Some(cutoff) = args.since.as_deref() {
                if invocation.timestamp.as_str() < cutoff {
                    continue;
                }
            }
            total += 1;
            let bucket = match invocation.outcome {
                InvocationOutcome::Worked => &mut by_outcome.worked,
                InvocationOutcome::Partial => &mut by_outcome.partial,
                InvocationOutcome::DidntApply => &mut by_outcome.didnt_apply,
                InvocationOutcome::Incorrect => &mut by_outcome.incorrect,
            };
            bucket.count += 1;
            bucket.invocations.push(RecurrenceInvocation {
                timestamp: invocation.timestamp,
                correction: invocation.correction,
            });
        }
    }

    Ok(SkillRecurrenceResult {
        skill_id: args.skill_id.clone(),
        by_outcome,
        total,
    })
}

fn parse_invocation_outcome(verb: &str, raw: &str) -> Result<InvocationOutcome> {
    match raw {
        "worked" => Ok(InvocationOutcome::Worked),
        "partial" => Ok(InvocationOutcome::Partial),
        "didnt-apply" => Ok(InvocationOutcome::DidntApply),
        "incorrect" => Ok(InvocationOutcome::Incorrect),
        other => Err(Error::malformed(
            verb,
            format!(
                "outcome must be one of 'worked', 'partial', 'didnt-apply', 'incorrect'; got '{other}'"
            ),
        )),
    }
}

fn validate_skill_id(verb: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::malformed(verb, "skill_id must not be empty"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("skill_id must not contain path separators or '..': '{id}'"),
        ));
    }
    Ok(())
}

fn validate_session_id(verb: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::malformed(verb, "session_id must not be empty"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(Error::malformed(
            verb,
            format!("session_id must not contain path separators or '..': '{id}'"),
        ));
    }
    Ok(())
}

/// Generate an RFC 3339 UTC timestamp. The substrate's default clock for
/// any verb that needs to stamp an event.
fn current_rfc3339_utc() -> std::result::Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

/// `spec.show` — read the spec at `<id>.yaml`, parse, return.
///
/// NotFound when the file doesn't exist; Malformed if it parses badly.
fn spec_show(layout: &OrbitLayout, args: &SpecShowArgs) -> Result<SpecShowResult> {
    const VERB: &str = "spec.show";

    if args.id.is_empty() {
        return Err(Error::malformed(VERB, "id must not be empty"));
    }
    // Defensive: reject ids that contain path separators. Spec ids are slug-
    // shaped and the layout already enforces .yaml extension; a `..` or `/`
    // would let a caller read arbitrary YAML files in the workspace.
    if args.id.contains('/') || args.id.contains('\\') || args.id.contains("..") {
        return Err(Error::malformed(
            VERB,
            format!("id must not contain path separators or '..': '{}'", args.id),
        ));
    }

    let path = layout.spec_file(&args.id);
    if !path.exists() {
        return Err(Error::not_found(
            VERB,
            format!("no spec at {}", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::unavailable(VERB, format!("read {}: {e}", path.display()))
    })?;
    let spec: Spec = parse_yaml(&text).map_err(|mut e| {
        e.verb = VERB.into();
        e
    })?;
    Ok(SpecShowResult { spec })
}

// ============================================================================
// Wire envelope
// ============================================================================
//
// Both CLI (`--json` mode) and MCP (`tools/call` response payload) emit the
// same envelope shape so byte-equal output falls out for free:
//
//   ok  : {"data":<verb-response>,"ok":true}
//   err : {"error":{"category":"<cat>","message":"<msg>","verb":"<verb>"},"ok":false}
//
// serde_json sorts object keys alphabetically by default, so the exact byte
// layout is deterministic across both surfaces. Inner struct fields preserve
// declaration order via the Serialize derive.

/// Build the OK envelope as a JSON [`Value`]. Callers stringify via
/// [`serde_json::to_string`] when they want bytes.
pub fn envelope_ok<T: Serialize>(data: &T) -> Value {
    json!({ "ok": true, "data": data })
}

/// Build the error envelope as a JSON [`Value`].
pub fn envelope_err(err: &Error) -> Value {
    json!({
        "ok": false,
        "error": {
            "verb": err.verb,
            "category": err.category.as_str(),
            "message": err.message,
        }
    })
}

/// Convenience: stringify the OK envelope. Returns the canonical wire bytes
/// as a UTF-8 string. Infallible for any `T: Serialize` whose serialise is
/// itself infallible (the envelope wrapper introduces no new failure modes).
pub fn envelope_ok_string<T: Serialize>(data: &T) -> Result<String> {
    serde_json::to_string(&envelope_ok(data)).map_err(|e| {
        Error::malformed("envelope", format!("serialise ok envelope: {e}")).with_source(e)
    })
}

/// Convenience: stringify the error envelope. Cannot fail in practice —
/// errors are simple owned strings + an enum.
pub fn envelope_err_string(err: &Error) -> String {
    // unwrap-justified: envelope_err produces only owned strings + a fixed
    // shape; serde_json::to_string on a Value cannot fail for these inputs.
    serde_json::to_string(&envelope_err(err)).expect("error envelope serialisation is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::serialise_yaml;
    use crate::error::Category;
    use crate::schema::AcType;
    use crate::schema::Spec;
    use tempfile::tempdir;

    fn write_spec(layout: &OrbitLayout, id: &str, goal: &str, status: SpecStatus) {
        let spec = Spec {
            id: id.into(),
            goal: goal.into(),
            cards: vec![],
            status,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir(id).unwrap();
        std::fs::write(layout.spec_file(id), serialise_yaml(&spec).unwrap()).unwrap();
    }

    fn unwrap_spec_list(resp: VerbResponse) -> SpecListResult {
        match resp {
            VerbResponse::SpecList(r) => r,
            other => panic!("expected SpecList variant, got {other:?}"),
        }
    }

    #[test]
    fn spec_list_returns_empty_when_no_specs() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let resp = execute(&layout, &VerbRequest::SpecList(SpecListArgs::default())).unwrap();
        assert!(unwrap_spec_list(resp).specs.is_empty());
    }

    #[test]
    fn spec_list_returns_specs_sorted_by_id() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0002", "second", SpecStatus::Open);
        write_spec(&layout, "0001", "first", SpecStatus::Open);

        let resp = execute(&layout, &VerbRequest::SpecList(SpecListArgs::default())).unwrap();
        let r = unwrap_spec_list(resp);
        let ids: Vec<_> = r.specs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["0001", "0002"]);
    }

    #[test]
    fn spec_list_status_filter_open_excludes_closed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "first", SpecStatus::Open);
        write_spec(&layout, "0002", "second", SpecStatus::Closed);

        let args = SpecListArgs { status: Some("open".into()) };
        let resp = execute(&layout, &VerbRequest::SpecList(args)).unwrap();
        let r = unwrap_spec_list(resp);
        assert_eq!(r.specs.len(), 1);
        assert_eq!(r.specs[0].id, "0001");
    }

    #[test]
    fn spec_list_invalid_status_filter_is_malformed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let args = SpecListArgs { status: Some("nope".into()) };
        let err = execute(&layout, &VerbRequest::SpecList(args)).unwrap_err();
        assert_eq!(err.to_string(), "spec.list: malformed: status must be 'open' or 'closed', got 'nope'");
    }

    #[test]
    fn spec_list_malformed_file_surfaces_with_correct_verb() {
        // ac-05 verification: error format `<verb>: <category>: <sentence>`,
        // and the verb is the one the caller invoked (not the canonical
        // layer's generic tag).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        layout.ensure_spec_dir("bad").unwrap();
        std::fs::write(layout.spec_file("bad"), "id: '0001'\nunknown_field: oops\n").unwrap();

        let err = execute(&layout, &VerbRequest::SpecList(SpecListArgs::default())).unwrap_err();
        assert!(
            err.to_string().starts_with("spec.list: malformed: "),
            "expected spec.list-tagged malformed error, got {err}"
        );
    }

    #[test]
    fn verb_request_round_trips_through_json() {
        // The MCP surface translates `tools/call` into VerbRequest by
        // constructing `{"verb": name, "args": arguments}` and deserialising.
        // This test pins that contract.
        let json = serde_json::json!({
            "verb": "spec.list",
            "args": { "status": "open" }
        });
        let req: VerbRequest = serde_json::from_value(json).unwrap();
        match req {
            VerbRequest::SpecList(args) => assert_eq!(args.status.as_deref(), Some("open")),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn verb_request_rejects_unknown_args_field() {
        // deny_unknown_fields on args means typo'd MCP arguments fail loudly
        // rather than being silently ignored.
        let json = serde_json::json!({
            "verb": "spec.list",
            "args": { "stutus": "open" }
        });
        let err = serde_json::from_value::<VerbRequest>(json).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn envelope_ok_shape_is_stable() {
        let resp = VerbResponse::SpecList(SpecListResult {
            specs: vec![SpecSummary {
                id: "0001".into(),
                goal: "g".into(),
                status: "open".into(),
                cards: vec![],
                labels: vec![],
            }],
        });
        let s = envelope_ok_string(&resp).unwrap();
        // Object keys are alphabetically ordered by default in serde_json,
        // so "data" comes before "ok". Inner struct fields follow declaration
        // order via the derive: id, goal, status, cards, labels.
        assert!(s.starts_with(r#"{"data":"#), "got {s}");
        assert!(s.contains(r#""ok":true"#), "got {s}");
    }

    #[test]
    fn envelope_err_shape_matches_error_format() {
        let err = Error::not_found("spec.list", "no specs dir");
        let s = envelope_err_string(&err);
        // Outer keys alphabetical: error, ok. Inner keys alphabetical:
        // category, message, verb.
        assert_eq!(
            s,
            r#"{"error":{"category":"not-found","message":"no specs dir","verb":"spec.list"},"ok":false}"#
        );
    }

    #[test]
    fn spec_show_returns_full_spec() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "the goal", SpecStatus::Open);

        let resp = execute(
            &layout,
            &VerbRequest::SpecShow(SpecShowArgs { id: "0001".into() }),
        )
        .unwrap();
        let VerbResponse::SpecShow(r) = resp else {
            panic!("wrong variant")
        };
        assert_eq!(r.spec.id, "0001");
        assert_eq!(r.spec.goal, "the goal");
        assert_eq!(r.spec.status, SpecStatus::Open);
    }

    #[test]
    fn spec_show_missing_id_is_not_found() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let err = execute(
            &layout,
            &VerbRequest::SpecShow(SpecShowArgs { id: "0099".into() }),
        )
        .unwrap_err();
        assert!(
            err.to_string().starts_with("spec.show: not-found: no spec at "),
            "got {err}"
        );
    }

    #[test]
    fn spec_show_empty_id_is_malformed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let err = execute(
            &layout,
            &VerbRequest::SpecShow(SpecShowArgs { id: String::new() }),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "spec.show: malformed: id must not be empty");
    }

    #[test]
    fn spec_show_path_traversal_id_is_malformed() {
        // Defence: a slash or `..` in id MUST fail before any filesystem op.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        for bad in ["../etc/passwd", "..", "0001/../..", "a/b"] {
            let err = execute(
                &layout,
                &VerbRequest::SpecShow(SpecShowArgs { id: bad.into() }),
            )
            .unwrap_err();
            assert!(
                err.to_string().starts_with("spec.show: malformed: "),
                "expected malformed for id={bad:?}, got {err}"
            );
        }
    }

    // ------------------------------------------------------------------------
    // spec.note tests
    // ------------------------------------------------------------------------

    fn read_notes_stream(layout: &OrbitLayout, id: &str) -> String {
        std::fs::read_to_string(layout.notes_stream(id)).unwrap_or_default()
    }

    #[test]
    fn spec_note_appends_jsonl_line_with_supplied_timestamp() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = SpecNoteArgs {
            id: "0001".into(),
            body: "first note".into(),
            labels: vec![],
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let resp = execute(&layout, &VerbRequest::SpecNote(args)).unwrap();
        let VerbResponse::SpecNote(r) = resp else {
            panic!("wrong variant")
        };
        assert_eq!(r.note.spec_id, "0001");
        assert_eq!(r.note.body, "first note");
        assert_eq!(r.note.timestamp, "2026-05-07T12:00:00Z");

        let stream = read_notes_stream(&layout, "0001");
        // One line, JSON-shaped, ends with newline.
        let lines: Vec<_> = stream.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(stream.ends_with('\n'));
        // JSONL streams use direct struct serialisation (declaration order),
        // not envelope serialisation (alphabetical via serde_json::Value).
        // NoteEvent declaration order: spec_id, body, labels, timestamp.
        assert_eq!(
            lines[0],
            r#"{"spec_id":"0001","body":"first note","labels":[],"timestamp":"2026-05-07T12:00:00Z"}"#
        );
    }

    #[test]
    fn spec_note_appends_in_order_across_calls() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        for (i, body) in ["one", "two", "three"].iter().enumerate() {
            let args = SpecNoteArgs {
                id: "0001".into(),
                body: (*body).into(),
                labels: vec![],
                timestamp: Some(format!("2026-05-07T12:00:0{i}Z")),
            };
            execute(&layout, &VerbRequest::SpecNote(args)).unwrap();
        }
        let stream = read_notes_stream(&layout, "0001");
        let bodies: Vec<_> = stream
            .lines()
            .filter_map(|l| serde_json::from_str::<NoteEvent>(l).ok())
            .map(|e| e.body)
            .collect();
        assert_eq!(bodies, vec!["one", "two", "three"]);
    }

    #[test]
    fn spec_note_default_timestamp_is_rfc3339_shaped() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = SpecNoteArgs {
            id: "0001".into(),
            body: "auto-stamped".into(),
            labels: vec![],
            timestamp: None,
        };
        let resp = execute(&layout, &VerbRequest::SpecNote(args)).unwrap();
        let VerbResponse::SpecNote(r) = resp else {
            panic!()
        };
        // Sanity: looks like 2026-MM-DDTHH:MM:SSZ (RFC 3339 UTC). We avoid
        // checking the actual time because tests must be deterministic.
        assert!(
            r.note.timestamp.len() >= 20,
            "timestamp too short: {}",
            r.note.timestamp
        );
        assert!(
            r.note.timestamp.contains('T') && r.note.timestamp.ends_with('Z'),
            "timestamp not RFC 3339 UTC shaped: {}",
            r.note.timestamp
        );
    }

    #[test]
    fn spec_note_missing_spec_is_not_found() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let args = SpecNoteArgs {
            id: "9999".into(),
            body: "x".into(),
            labels: vec![],
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let err = execute(&layout, &VerbRequest::SpecNote(args)).unwrap_err();
        assert!(
            err.to_string().starts_with("spec.note: not-found: no spec at "),
            "got {err}"
        );
    }

    #[test]
    fn spec_note_empty_body_is_malformed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = SpecNoteArgs {
            id: "0001".into(),
            body: String::new(),
            labels: vec![],
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let err = execute(&layout, &VerbRequest::SpecNote(args)).unwrap_err();
        assert_eq!(err.to_string(), "spec.note: malformed: body must not be empty");
    }

    #[test]
    fn spec_note_path_traversal_id_is_malformed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let args = SpecNoteArgs {
            id: "../etc/passwd".into(),
            body: "x".into(),
            labels: vec![],
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let err = execute(&layout, &VerbRequest::SpecNote(args)).unwrap_err();
        assert!(err.to_string().starts_with("spec.note: malformed: "));
    }

    // ------------------------------------------------------------------------
    // spec.create / spec.update / spec.close tests
    // ------------------------------------------------------------------------

    use crate::schema::{Card, CardMaturity};

    fn write_card(layout: &OrbitLayout, slug: &str) {
        let card = Card {
            id: Some(slug.to_string()),
            feature: format!("feature-{slug}"),
            as_a: None,
            i_want: None,
            so_that: None,
            goal: "g".into(),
            maturity: CardMaturity::Planned,
            scenarios: vec![],
            specs: vec![],
            relations: vec![],
            references: vec![],
            notes: vec![],
        };
        let yaml = crate::canonical::serialise_yaml(&card).unwrap();
        std::fs::write(layout.card_file(slug), yaml).unwrap();
    }

    fn read_card(layout: &OrbitLayout, slug: &str) -> Card {
        let text = std::fs::read_to_string(layout.card_file(slug)).unwrap();
        parse_yaml(&text).unwrap()
    }

    #[test]
    fn spec_create_writes_yaml_and_returns_spec() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());

        let args = SpecCreateArgs {
            id: "0001".into(),
            goal: "ship it".into(),
            cards: vec!["0020-orbit-state".into()],
            labels: vec!["spec".into()],
            acceptance_criteria: vec![],
        };
        let resp = execute(&layout, &VerbRequest::SpecCreate(args)).unwrap();
        let VerbResponse::SpecCreate(r) = resp else {
            panic!("wrong variant")
        };
        assert_eq!(r.spec.id, "0001");
        assert_eq!(r.spec.status, SpecStatus::Open);

        // File on disk parses back identically.
        let text = std::fs::read_to_string(layout.spec_file("0001")).unwrap();
        let parsed: Spec = parse_yaml(&text).unwrap();
        assert_eq!(parsed, r.spec);
    }

    #[test]
    fn spec_create_conflict_when_already_exists() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = SpecCreateArgs {
            id: "0001".into(),
            goal: "ship".into(),
            cards: vec![],
            labels: vec![],
            acceptance_criteria: vec![],
        };
        let err = execute(&layout, &VerbRequest::SpecCreate(args)).unwrap_err();
        assert!(
            err.to_string().starts_with("spec.create: conflict: "),
            "got {err}"
        );
    }

    #[test]
    fn spec_create_empty_goal_is_malformed() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());

        let args = SpecCreateArgs {
            id: "0001".into(),
            goal: String::new(),
            cards: vec![],
            labels: vec![],
            acceptance_criteria: vec![],
        };
        let err = execute(&layout, &VerbRequest::SpecCreate(args)).unwrap_err();
        assert_eq!(err.to_string(), "spec.create: malformed: goal must not be empty");
    }

    #[test]
    fn spec_update_replaces_specified_fields_only() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        let original = Spec {
            id: "0001".into(),
            goal: "original".into(),
            cards: vec!["c1".into()],
            status: SpecStatus::Open,
            labels: vec!["spec".into()],
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "ac-01".into(),
                description: "first".into(),
                gate: false,
                checked: false,
                verification: None,
                ac_type: AcType::Code,
            }],
        };
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(
            layout.spec_file("0001"),
            crate::canonical::serialise_yaml(&original).unwrap(),
        )
        .unwrap();

        // Update only goal and labels — cards and ACs must stay.
        let args = SpecUpdateArgs {
            id: "0001".into(),
            goal: Some("revised".into()),
            cards: None,
            labels: Some(vec!["spec".into(), "experimental".into()]),
            acceptance_criteria: None,
        };
        let resp = execute(&layout, &VerbRequest::SpecUpdate(args)).unwrap();
        let VerbResponse::SpecUpdate(r) = resp else {
            panic!("wrong variant")
        };
        assert_eq!(r.spec.goal, "revised");
        assert_eq!(r.spec.cards, vec!["c1".to_string()]);
        assert_eq!(r.spec.labels, vec!["spec".to_string(), "experimental".to_string()]);
        assert_eq!(r.spec.acceptance_criteria.len(), 1);
        // Status must not have changed via update.
        assert_eq!(r.spec.status, SpecStatus::Open);
    }

    #[test]
    fn spec_update_rejects_empty_goal() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = SpecUpdateArgs {
            id: "0001".into(),
            goal: Some(String::new()),
            ..Default::default()
        };
        let err = execute(&layout, &VerbRequest::SpecUpdate(args)).unwrap_err();
        assert_eq!(err.to_string(), "spec.update: malformed: goal must not be empty");
    }

    #[test]
    fn spec_update_missing_spec_is_not_found() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let args = SpecUpdateArgs {
            id: "0099".into(),
            goal: Some("x".into()),
            ..Default::default()
        };
        let err = execute(&layout, &VerbRequest::SpecUpdate(args)).unwrap_err();
        assert!(err.to_string().starts_with("spec.update: not-found: "));
    }

    #[test]
    fn spec_close_flips_status_and_appends_to_linked_cards() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_card(&layout, "0021-tasks");

        // Spec linked to two cards.
        let spec = Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec!["0020-orbit-state".into(), "0021-tasks".into()],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(
            layout.spec_file("0001"),
            crate::canonical::serialise_yaml(&spec).unwrap(),
        )
        .unwrap();

        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else {
            panic!()
        };
        assert_eq!(r.spec.status, SpecStatus::Closed);
        assert_eq!(r.cards_updated.len(), 2);

        // Both cards now have the spec ref.
        let expected_ref = ".orbit/specs/0001/spec.yaml";
        for slug in ["0020-orbit-state", "0021-tasks"] {
            let card = read_card(&layout, slug);
            assert!(
                card.specs.iter().any(|s| s == expected_ref),
                "card {slug} missing spec ref: {:?}",
                card.specs
            );
        }

        // Spec on disk reflects the closed status.
        let text = std::fs::read_to_string(layout.spec_file("0001")).unwrap();
        let reread: Spec = parse_yaml(&text).unwrap();
        assert_eq!(reread.status, SpecStatus::Closed);
    }

    #[test]
    fn spec_close_idempotent_when_card_already_has_ref() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        // Pre-stage card already containing the spec ref (simulates a
        // previous partial close).
        let card = Card {
            id: Some("0020-x".into()),
            feature: "f".into(),
            as_a: None,
            i_want: None,
            so_that: None,
            goal: "g".into(),
            maturity: CardMaturity::Planned,
            scenarios: vec![],
            specs: vec![".orbit/specs/0001/spec.yaml".into()],
            relations: vec![],
            references: vec![],
            notes: vec![],
        };
        std::fs::write(
            layout.card_file("0020-x"),
            crate::canonical::serialise_yaml(&card).unwrap(),
        )
        .unwrap();

        let spec = Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec!["0020-x".into()],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(
            layout.spec_file("0001"),
            crate::canonical::serialise_yaml(&spec).unwrap(),
        )
        .unwrap();

        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else {
            panic!()
        };
        // Card was a no-op, so cards_updated is empty.
        assert!(r.cards_updated.is_empty());
        // Card still has exactly one ref (no duplicate).
        let post = read_card(&layout, "0020-x");
        assert_eq!(post.specs, vec![".orbit/specs/0001/spec.yaml".to_string()]);
    }

    #[test]
    fn spec_close_already_closed_is_conflict() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Closed);

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("spec.close: conflict: "));
    }

    #[test]
    fn spec_close_missing_linked_card_rolls_back_no_writes() {
        // Validate the "all linked cards update or none do" contract: if a
        // card is missing, no card writes happen and the spec stays open.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-present");
        // 0021-missing is intentionally absent.

        let spec = Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec!["0020-present".into(), "0021-missing".into()],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(
            layout.spec_file("0001"),
            crate::canonical::serialise_yaml(&spec).unwrap(),
        )
        .unwrap();

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("spec.close: not-found: "));

        // Present card was NOT written — phase 1 collected before phase 2.
        let present = read_card(&layout, "0020-present");
        assert!(present.specs.is_empty(), "card was modified despite atomicity contract: {:?}", present.specs);

        // Spec still open.
        let reread: Spec =
            parse_yaml(&std::fs::read_to_string(layout.spec_file("0001")).unwrap()).unwrap();
        assert_eq!(reread.status, SpecStatus::Open);
    }

    // ------------------------------------------------------------------------
    // Task verb tests (ac-07)
    // ------------------------------------------------------------------------

    fn open_task(layout: &OrbitLayout, spec_id: &str, task_id: &str, body: &str) {
        let args = TaskOpenArgs {
            spec_id: spec_id.into(),
            body: body.into(),
            labels: vec![],
            task_id: Some(task_id.into()),
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        execute(layout, &VerbRequest::TaskOpen(args)).unwrap();
    }

    #[test]
    fn task_open_appends_event_with_substrate_or_supplied_timestamp() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let args = TaskOpenArgs {
            spec_id: "0001".into(),
            body: "investigate flake".into(),
            labels: vec!["bug".into()],
            task_id: Some("t-001".into()),
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let resp = execute(&layout, &VerbRequest::TaskOpen(args)).unwrap();
        let VerbResponse::TaskOpen(r) = resp else {
            panic!()
        };
        assert_eq!(r.task_id, "t-001");
        assert_eq!(r.event.event, TaskEventKind::Open);

        // JSONL stream contains exactly one event.
        let text = std::fs::read_to_string(layout.task_stream("0001")).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains(r#""event":"open""#));
    }

    #[test]
    fn task_open_generates_unique_task_id_when_none_supplied() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let mk = || TaskOpenArgs {
            spec_id: "0001".into(),
            body: "x".into(),
            labels: vec![],
            task_id: None,
            timestamp: None,
        };
        let r1 = execute(&layout, &VerbRequest::TaskOpen(mk())).unwrap();
        let r2 = execute(&layout, &VerbRequest::TaskOpen(mk())).unwrap();
        let (VerbResponse::TaskOpen(a), VerbResponse::TaskOpen(b)) = (r1, r2) else {
            panic!()
        };
        assert_ne!(a.task_id, b.task_id, "task ids must be unique within a process");
        assert!(a.task_id.starts_with("t-"));
    }

    #[test]
    fn task_open_duplicate_id_is_conflict() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "first");

        let dup = TaskOpenArgs {
            spec_id: "0001".into(),
            body: "again".into(),
            labels: vec![],
            task_id: Some("t1".into()),
            timestamp: Some("2026-05-07T12:00:00Z".into()),
        };
        let err = execute(&layout, &VerbRequest::TaskOpen(dup)).unwrap_err();
        assert!(
            err.to_string().starts_with("task.open: conflict: "),
            "got {err}"
        );
    }

    #[test]
    fn task_list_reduces_to_current_state() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "first");
        open_task(&layout, "0001", "t2", "second");

        // Claim t1.
        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();

        let resp = execute(&layout, &VerbRequest::TaskList(TaskListArgs::default())).unwrap();
        let VerbResponse::TaskList(r) = resp else {
            panic!()
        };
        assert_eq!(r.tasks.len(), 2);
        let by_id: std::collections::HashMap<_, _> =
            r.tasks.iter().map(|t| (t.task_id.as_str(), t.state.as_str())).collect();
        assert_eq!(by_id["t1"], "claim");
        assert_eq!(by_id["t2"], "open");
    }

    #[test]
    fn task_ready_excludes_claimed_and_done() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "ready1");
        open_task(&layout, "0001", "t2", "claimed");
        open_task(&layout, "0001", "t3", "done");

        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t2".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();
        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t3".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();
        execute(
            &layout,
            &VerbRequest::TaskDone(TaskDoneArgs {
                spec_id: "0001".into(),
                task_id: "t3".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:02Z".into()),
            }),
        )
        .unwrap();

        let resp = execute(&layout, &VerbRequest::TaskReady(TaskReadyArgs::default())).unwrap();
        let VerbResponse::TaskReady(r) = resp else {
            panic!()
        };
        assert_eq!(r.tasks.len(), 1);
        assert_eq!(r.tasks[0].task_id, "t1");
    }

    #[test]
    fn task_claim_rejects_non_open_state() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "x");

        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();

        // Second claim — current state is "claim", not "open".
        let err = execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:02Z".into()),
            }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("task.claim: conflict: "));
    }

    #[test]
    fn task_update_after_done_is_conflict() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "x");

        execute(
            &layout,
            &VerbRequest::TaskDone(TaskDoneArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();

        let err = execute(
            &layout,
            &VerbRequest::TaskUpdate(TaskUpdateArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: "post-mortem".into(),
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:02Z".into()),
            }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("task.update: conflict: "));
    }

    #[test]
    fn task_show_returns_full_event_history() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "x");
        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();
        execute(
            &layout,
            &VerbRequest::TaskUpdate(TaskUpdateArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: "in progress".into(),
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:02Z".into()),
            }),
        )
        .unwrap();

        let resp = execute(
            &layout,
            &VerbRequest::TaskShow(TaskShowArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
            }),
        )
        .unwrap();
        let VerbResponse::TaskShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.events.len(), 3);
        assert_eq!(r.state.state, "update");
        assert_eq!(r.state.event_count, 3);
    }

    #[test]
    fn task_state_survives_session_reset() {
        // ac-07 verification: after an open-and-claim, a fresh layout reads
        // the JSONL stream and reproduces the prior state. Tasks live on
        // disk; the index is derivable but not the source of truth.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);
        open_task(&layout, "0001", "t1", "x");
        execute(
            &layout,
            &VerbRequest::TaskClaim(TaskClaimArgs {
                spec_id: "0001".into(),
                task_id: "t1".into(),
                body: None,
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
            }),
        )
        .unwrap();

        // "Restart" — drop and rebuild the layout handle. The disk state is
        // unchanged; the in-memory index is a derived view we don't keep.
        let layout2 = OrbitLayout::at(dir.path());
        let resp = execute(&layout2, &VerbRequest::TaskList(TaskListArgs::default())).unwrap();
        let VerbResponse::TaskList(r) = resp else {
            panic!()
        };
        assert_eq!(r.tasks.len(), 1);
        assert_eq!(r.tasks[0].state, "claim");
    }

    #[test]
    fn spec_close_rejects_unfinished_tasks() {
        // ac-06 verification: "spec.close requires all child tasks done;
        // rejects with a clear error otherwise."
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        let spec = Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0001").unwrap();
        std::fs::write(
            layout.spec_file("0001"),
            crate::canonical::serialise_yaml(&spec).unwrap(),
        )
        .unwrap();
        open_task(&layout, "0001", "t1", "still going");

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("spec.close: conflict: "));
        assert!(err.message.contains("unfinished"));
    }

    #[test]
    fn spec_close_full_lifecycle_integration() {
        // ac-06 integration test: create spec → open tasks → close spec
        // without finishing tasks (rejected) → finish tasks → close spec
        // (succeeds, linked cards' specs_array updated).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-test");

        // 1. Create spec linked to one card
        execute(
            &layout,
            &VerbRequest::SpecCreate(SpecCreateArgs {
                id: "0001".into(),
                goal: "do the thing".into(),
                cards: vec!["0020-test".into()],
                labels: vec![],
                acceptance_criteria: vec![],
            }),
        )
        .unwrap();

        // 2. Open two tasks
        open_task(&layout, "0001", "t1", "task one");
        open_task(&layout, "0001", "t2", "task two");

        // 3. Close fails — tasks unfinished
        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(err.message.contains("unfinished"));

        // 4. Finish both tasks
        for tid in ["t1", "t2"] {
            execute(
                &layout,
                &VerbRequest::TaskDone(TaskDoneArgs {
                    spec_id: "0001".into(),
                    task_id: tid.into(),
                    body: None,
                    labels: vec![],
                    timestamp: Some("2026-05-07T12:00:00Z".into()),
                }),
            )
            .unwrap();
        }

        // 5. Close succeeds
        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else {
            panic!()
        };
        assert_eq!(r.spec.status, SpecStatus::Closed);
        assert_eq!(r.cards_updated, vec!["0020-test".to_string()]);

        // 6. Linked card's specs array now contains the ref
        let card = read_card(&layout, "0020-test");
        assert_eq!(card.specs, vec![".orbit/specs/0001/spec.yaml".to_string()]);
    }

    #[test]
    fn task_show_unknown_task_is_not_found() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "g", SpecStatus::Open);

        let err = execute(
            &layout,
            &VerbRequest::TaskShow(TaskShowArgs {
                spec_id: "0001".into(),
                task_id: "nope".into(),
            }),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("task.show: not-found: "));
    }

    // ------------------------------------------------------------------------
    // Memory / card / choice tests (ac-08, ac-09, ac-10)
    // ------------------------------------------------------------------------

    use crate::schema::{ChoiceStatus, Memory};

    fn write_memory(layout: &OrbitLayout, key: &str, body: &str) {
        layout.ensure_dirs().unwrap();
        let m = Memory {
            key: key.into(),
            body: body.into(),
            timestamp: "2026-05-07T12:00:00Z".into(),
            labels: vec![],
        };
        std::fs::write(
            layout.memory_file(key),
            crate::canonical::serialise_yaml(&m).unwrap(),
        )
        .unwrap();
    }

    fn write_choice(layout: &OrbitLayout, slug: &str, title: &str, body: &str, status: ChoiceStatus) {
        layout.ensure_dirs().unwrap();
        // Real choices use NNNN-suffixed filenames; the `id` field carries just
        // the four-digit prefix per existing convention. Test fixtures supply
        // the full slug (`"0015-foo"`) and we derive the numeric id from it.
        let id = slug.split('-').next().unwrap_or(slug).to_string();
        let c = Choice {
            id,
            title: title.into(),
            status,
            date_created: "2026-05-07".into(),
            date_modified: None,
            body: body.into(),
            references: vec![],
        };
        std::fs::write(
            layout.choice_file(slug),
            crate::canonical::serialise_yaml(&c).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn memory_remember_writes_yaml_and_returns_memory() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        let resp = execute(
            &layout,
            &VerbRequest::MemoryRemember(MemoryRememberArgs {
                key: "estimate-guard".into(),
                body: "recut at Claude-pace".into(),
                labels: vec!["methodology".into()],
                timestamp: Some("2026-05-07T12:00:00Z".into()),
                no_nudge: false,
            }),
        )
        .unwrap();
        let VerbResponse::MemoryRemember(r) = resp else {
            panic!()
        };
        assert_eq!(r.memory.key, "estimate-guard");
        assert!(layout.memory_file("estimate-guard").exists());
    }

    #[test]
    fn memory_remember_upserts_existing_key() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        execute(
            &layout,
            &VerbRequest::MemoryRemember(MemoryRememberArgs {
                key: "k".into(),
                body: "v1".into(),
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:00Z".into()),
                no_nudge: false,
            }),
        )
        .unwrap();
        execute(
            &layout,
            &VerbRequest::MemoryRemember(MemoryRememberArgs {
                key: "k".into(),
                body: "v2".into(),
                labels: vec![],
                timestamp: Some("2026-05-07T12:00:01Z".into()),
                no_nudge: false,
            }),
        )
        .unwrap();
        let text = std::fs::read_to_string(layout.memory_file("k")).unwrap();
        assert!(text.contains("v2"), "upsert failed: {text}");
        assert!(!text.contains("v1"), "v1 still present: {text}");
    }

    #[test]
    fn memory_search_substring_case_insensitive_over_body_and_labels() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_memory(&layout, "k1", "Recut at Claude-pace");
        write_memory(&layout, "k2", "atomic writes for substrate");
        write_memory(&layout, "k3", "completely unrelated");

        let resp = execute(
            &layout,
            &VerbRequest::MemorySearch(MemorySearchArgs {
                query: "CLAUDE".into(),
            }),
        )
        .unwrap();
        let VerbResponse::MemorySearch(r) = resp else {
            panic!()
        };
        assert_eq!(r.memories.len(), 1);
        assert_eq!(r.memories[0].key, "k1");
    }

    #[test]
    fn memory_list_returns_sorted_by_key() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_memory(&layout, "zebra", "z");
        write_memory(&layout, "apple", "a");

        let resp = execute(&layout, &VerbRequest::MemoryList(MemoryListArgs::default())).unwrap();
        let VerbResponse::MemoryList(r) = resp else {
            panic!()
        };
        let keys: Vec<_> = r.memories.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, vec!["apple", "zebra"]);
    }

    #[test]
    fn card_show_returns_full_card() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");

        // Full slug.
        let resp = execute(
            &layout,
            &VerbRequest::CardShow(CardShowArgs {
                slug: "0020-orbit-state".into(),
            }),
        )
        .unwrap();
        let VerbResponse::CardShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.slug, "0020-orbit-state");

        // Bare NNNN resolves via prefix-match per choice 0022.
        let resp = execute(
            &layout,
            &VerbRequest::CardShow(CardShowArgs { slug: "20".into() }),
        )
        .unwrap();
        let VerbResponse::CardShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.slug, "0020-orbit-state");

        // Padded form.
        let resp = execute(
            &layout,
            &VerbRequest::CardShow(CardShowArgs {
                slug: "0020".into(),
            }),
        )
        .unwrap();
        let VerbResponse::CardShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.slug, "0020-orbit-state");

        // Zero-match returns not-found.
        let err = execute(
            &layout,
            &VerbRequest::CardShow(CardShowArgs { slug: "99".into() }),
        )
        .unwrap_err();
        assert_eq!(err.category, Category::NotFound);
    }

    #[test]
    fn card_show_bare_numeric_ambiguous_errors() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        // Two cards both starting `0020-` — ambiguity case the resolver names.
        write_card(&layout, "0020-foo");
        write_card(&layout, "0020-bar");

        let err = execute(
            &layout,
            &VerbRequest::CardShow(CardShowArgs { slug: "20".into() }),
        )
        .unwrap_err();
        assert!(
            err.message.contains("ambiguous"),
            "expected ambiguous error, got: {}",
            err.message
        );
    }

    #[test]
    fn card_list_filters_by_maturity() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        // write_card uses Planned. Add one Established manually.
        write_card(&layout, "0020-planned");
        let est = Card {
            id: Some("0021-established".into()),
            feature: "f".into(),
            as_a: None,
            i_want: None,
            so_that: None,
            goal: "g".into(),
            maturity: CardMaturity::Established,
            scenarios: vec![],
            specs: vec![],
            relations: vec![],
            references: vec![],
            notes: vec![],
        };
        std::fs::write(
            layout.card_file("0021-established"),
            crate::canonical::serialise_yaml(&est).unwrap(),
        )
        .unwrap();

        let resp = execute(
            &layout,
            &VerbRequest::CardList(CardListArgs {
                maturity: Some("established".into()),
            }),
        )
        .unwrap();
        let VerbResponse::CardList(r) = resp else {
            panic!()
        };
        assert_eq!(r.cards.len(), 1);
        assert_eq!(r.cards[0].slug, "0021-established");
    }

    #[test]
    fn card_search_hits_feature_or_goal() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_card(&layout, "0021-tasks");

        let resp = execute(
            &layout,
            &VerbRequest::CardSearch(CardSearchArgs {
                query: "TASKS".into(),
            }),
        )
        .unwrap();
        let VerbResponse::CardSearch(r) = resp else {
            panic!()
        };
        assert_eq!(r.cards.len(), 1);
        assert_eq!(r.cards[0].slug, "0021-tasks");
    }

    #[test]
    fn choice_show_returns_full_choice() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        write_choice(&layout, "0015-orbit-state", "title", "body", ChoiceStatus::Accepted);

        // Bare NNNN resolves via prefix-match per choice 0022.
        let resp = execute(
            &layout,
            &VerbRequest::ChoiceShow(ChoiceShowArgs { id: "0015".into() }),
        )
        .unwrap();
        let VerbResponse::ChoiceShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.choice.title, "title");

        // Bare unpadded form (`15`) resolves identically.
        let resp = execute(
            &layout,
            &VerbRequest::ChoiceShow(ChoiceShowArgs { id: "15".into() }),
        )
        .unwrap();
        let VerbResponse::ChoiceShow(r) = resp else {
            panic!()
        };
        assert_eq!(r.choice.title, "title");

        // Full slug still works.
        let resp = execute(
            &layout,
            &VerbRequest::ChoiceShow(ChoiceShowArgs {
                id: "0015-orbit-state".into(),
            }),
        )
        .unwrap();
        let VerbResponse::ChoiceShow(_) = resp else {
            panic!()
        };

        // Zero-match returns not-found.
        let err = execute(
            &layout,
            &VerbRequest::ChoiceShow(ChoiceShowArgs { id: "99".into() }),
        )
        .unwrap_err();
        assert_eq!(err.category, Category::NotFound);
    }

    #[test]
    fn choice_list_filters_by_status() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        write_choice(&layout, "0015-a", "first", "b", ChoiceStatus::Accepted);
        write_choice(&layout, "0016-b", "second", "b", ChoiceStatus::Proposed);

        let resp = execute(
            &layout,
            &VerbRequest::ChoiceList(ChoiceListArgs {
                status: Some("accepted".into()),
            }),
        )
        .unwrap();
        let VerbResponse::ChoiceList(r) = resp else {
            panic!()
        };
        assert_eq!(r.choices.len(), 1);
        assert_eq!(r.choices[0].id, "0015");
    }

    #[test]
    fn choice_search_hits_title_or_body() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        write_choice(&layout, "0015-atomic", "Atomic writes", "trade-off discussion", ChoiceStatus::Accepted);
        write_choice(&layout, "0016-other", "Other", "irrelevant", ChoiceStatus::Accepted);

        let resp = execute(
            &layout,
            &VerbRequest::ChoiceSearch(ChoiceSearchArgs {
                query: "TRADE".into(),
            }),
        )
        .unwrap();
        let VerbResponse::ChoiceSearch(r) = resp else {
            panic!()
        };
        assert_eq!(r.choices.len(), 1);
        assert_eq!(r.choices[0].id, "0015");
    }

    // ------------------------------------------------------------------------
    // session.prime tests (ac-11)
    // ------------------------------------------------------------------------

    #[test]
    fn session_prime_returns_open_specs_and_capped_memories() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "open one", SpecStatus::Open);
        write_spec(&layout, "0002", "closed one", SpecStatus::Closed);
        write_spec(&layout, "0003", "open two", SpecStatus::Open);
        for i in 0..15 {
            write_memory(
                &layout,
                &format!("k{i:02}"),
                &format!("memory body {i}"),
            );
        }

        let resp = execute(
            &layout,
            &VerbRequest::SessionPrime(SessionPrimeArgs::default()),
        )
        .unwrap();
        let VerbResponse::SessionPrime(r) = resp else {
            panic!()
        };

        // Only open specs.
        assert_eq!(r.open_specs.len(), 2);
        assert!(r.open_specs.iter().all(|s| s.status == "open"));

        // Memories capped at K=10.
        assert_eq!(r.memories.len(), 10);

        // Bound formula: 40 + 2*open + min(10, 10) = 40 + 4 + 10 = 54.
        assert_eq!(r.item_bound, 54);
    }

    #[test]
    fn session_prime_includes_global_latest_handover_and_bumps_bound() {
        // spec 2026-05-16-session-handover ac-07: prime surfaces the most-
        // recent Session globally, bumps item_bound by +1, and prefixes the
        // next_step sentinel.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "open one", SpecStatus::Open);

        let s = Session {
            id: "sess-X".into(),
            started_at: "2026-05-15T12:00:00Z".into(),
            ended_at: Some("2026-05-15T13:00:00Z".into()),
            distillate: "what I tried, what worked".into(),
            card_id: Some("0036-session-handover".into()),
            labels: vec![],
        };
        std::fs::write(layout.session_file("sess-X"), serialise_yaml(&s).unwrap()).unwrap();

        let resp = execute(
            &layout,
            &VerbRequest::SessionPrime(SessionPrimeArgs::default()),
        )
        .unwrap();
        let VerbResponse::SessionPrime(r) = resp else { panic!() };

        let h = r.handover.expect("handover should be Some");
        assert_eq!(h.session_id, "sess-X");
        // Bound: 40 + 2*1 + 10 (default cap.min(DEFAULT_MEMORY_CAP))
        //        + 1 (handover) = 53
        assert_eq!(r.item_bound, 53);
        // next_step prefix matches the stable sentinel.
        assert!(
            r.next_step.starts_with("Read the handover above before any other action. "),
            "expected sentinel prefix on next_step; got: {}",
            r.next_step,
        );
    }

    #[test]
    fn session_prime_handover_absent_keeps_next_step_unchanged() {
        // spec 2026-05-16-session-handover ac-07: when no sessions exist,
        // handover stays None, item_bound has no +1 addend, and next_step
        // is the un-prefixed base text.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_spec(&layout, "0001", "open one", SpecStatus::Open);

        let resp = execute(
            &layout,
            &VerbRequest::SessionPrime(SessionPrimeArgs::default()),
        )
        .unwrap();
        let VerbResponse::SessionPrime(r) = resp else { panic!() };

        assert!(r.handover.is_none());
        // 40 + 2*1 + 10 = 52 (no +1 for handover).
        assert_eq!(r.item_bound, 52);
        assert!(
            !r.next_step.starts_with("Read the handover above"),
            "unprefixed next_step expected; got: {}",
            r.next_step,
        );
    }

    #[test]
    fn session_prime_respects_custom_memory_cap() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        for i in 0..5 {
            write_memory(&layout, &format!("k{i}"), &format!("body {i}"));
        }

        let resp = execute(
            &layout,
            &VerbRequest::SessionPrime(SessionPrimeArgs {
                memory_cap: Some(3),
            }),
        )
        .unwrap();
        let VerbResponse::SessionPrime(r) = resp else {
            panic!()
        };
        assert_eq!(r.memories.len(), 3);
    }

    #[test]
    fn envelope_round_trip_deterministic() {
        // Two independent serialisations of the same response must produce
        // byte-identical envelopes — this is the parity guarantee for ac-05
        // expressed at the envelope layer.
        let resp = VerbResponse::SpecList(SpecListResult {
            specs: vec![
                SpecSummary {
                    id: "0001".into(),
                    goal: "first".into(),
                    status: "open".into(),
                    cards: vec!["0020-orbit-state".into()],
                    labels: vec!["spec".into()],
                },
                SpecSummary {
                    id: "0002".into(),
                    goal: "second".into(),
                    status: "closed".into(),
                    cards: vec![],
                    labels: vec![],
                },
            ],
        });
        let a = envelope_ok_string(&resp).unwrap();
        let b = envelope_ok_string(&resp).unwrap();
        assert_eq!(a, b);
    }

    // -----------------------------------------------------------------------
    // spec.close AC pre-flight (spec 2026-05-13-spec-close-ac-preflight)
    // -----------------------------------------------------------------------

    /// Helper: write a spec with the given ACs to disk, ready for spec.close.
    fn write_spec_with_acs(
        layout: &OrbitLayout,
        id: &str,
        cards: Vec<String>,
        acs: Vec<AcceptanceCriterion>,
    ) {
        let spec = Spec {
            id: id.into(),
            goal: "g".into(),
            cards,
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: acs,
        };
        layout.ensure_spec_dir(id).unwrap();
        std::fs::write(
            layout.spec_file(id),
            crate::canonical::serialise_yaml(&spec).unwrap(),
        )
        .unwrap();
    }

    fn ac(id: &str, gate: bool, checked: bool, ac_type: AcType) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: id.into(),
            description: format!("description for {id}"),
            gate,
            checked,
            verification: None,
            ac_type,
        }
    }

    #[test]
    fn spec_close_rejects_unchecked_acs() {
        // ac-02 verification (spec 2026-05-13-spec-close-ac-preflight,
        // generalised by spec 2026-05-16-ac-taxonomy ac-02): spec.close
        // returns Error::conflict when one or more blocking-kind
        // (Code/Config/Doc) ACs are unchecked, listing them by id.
        // No files are written.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, true, AcType::Code),
                ac("ac-02", false, false, AcType::Code),
                ac("ac-03", false, false, AcType::Code),
            ],
        );

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(
            err.to_string().starts_with("spec.close: conflict: "),
            "expected spec.close conflict, got: {err}"
        );
        assert!(err.message.contains("ac-02"), "missing ac-02 in: {err}");
        assert!(err.message.contains("ac-03"), "missing ac-03 in: {err}");
        // Spec is untouched on disk.
        let on_disk: Spec = parse_yaml(&std::fs::read_to_string(layout.spec_file("0001")).unwrap()).unwrap();
        assert_eq!(on_disk.status, SpecStatus::Open);
        // Linked card's specs array unchanged.
        let card = read_card(&layout, "0020-orbit-state");
        assert!(card.specs.is_empty(), "card mutated: {:?}", card.specs);
    }

    #[test]
    fn spec_close_unchecked_gate_ac_flagged_in_error() {
        // ac-02 verification: gate ACs in the unchecked set are flagged
        // separately in the error message.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", true, false, AcType::Code),  // unchecked gate
                ac("ac-02", false, false, AcType::Code), // unchecked non-gate
            ],
        );

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        // Both ids appear in the message.
        assert!(err.message.contains("ac-01"), "missing ac-01 in: {err}");
        assert!(err.message.contains("ac-02"), "missing ac-02 in: {err}");
        // The gate suffix "(gate: ac-01)" names only the gate AC.
        assert!(
            err.message.contains("(gate: ac-01)"),
            "missing gate suffix in: {err}",
        );
    }

    #[test]
    fn spec_close_force_proceeds_despite_unchecked() {
        // ac-03 verification (spec 2026-05-13-spec-close-ac-preflight):
        // --force closes despite unchecked blocking-kind ACs; the bypassed
        // AC ids land in SpecCloseResult.forced_unchecked.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, true, AcType::Code),
                ac("ac-02", false, false, AcType::Code),
                ac("ac-03", false, false, AcType::Code),
            ],
        );

        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: true }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else { panic!() };
        assert_eq!(r.spec.status, SpecStatus::Closed);
        assert_eq!(r.cards_updated, vec!["0020-orbit-state".to_string()]);
        assert_eq!(
            r.forced_unchecked,
            vec!["ac-02".to_string(), "ac-03".to_string()]
        );
        assert!(r.deferrable_open.is_empty());
    }

    #[test]
    fn spec_close_observation_acs_do_not_block() {
        // spec 2026-05-16-ac-taxonomy ac-02 verification: deferrable-kind
        // (Observation in this case) unchecked ACs do not block close
        // (no --force needed) and are reported in deferrable_open.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, true, AcType::Code),
                ac("ac-02", false, false, AcType::Observation), // unchecked but deferrable
                ac("ac-03", false, false, AcType::Observation), // unchecked but deferrable
            ],
        );

        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else { panic!() };
        assert_eq!(r.spec.status, SpecStatus::Closed);
        assert!(r.forced_unchecked.is_empty());
        assert_eq!(
            r.deferrable_open,
            vec!["ac-02".to_string(), "ac-03".to_string()]
        );
    }

    #[test]
    fn spec_close_mixed_blocking_and_deferrable() {
        // spec 2026-05-16-ac-taxonomy ac-02 verification: a spec with one
        // unchecked blocking AC + one unchecked deferrable AC: exit=conflict,
        // blocking list names only the blocking AC, deferrable_open is not
        // populated (the error path returns before it would be).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, false, AcType::Code),        // unchecked blocking
                ac("ac-02", false, false, AcType::Observation), // unchecked deferrable
            ],
        );

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("blocking AC"),
            "expected 'blocking AC' wording in: {err}"
        );
        assert!(err.message.contains("ac-01"), "blocking AC ac-01 missing in: {err}");
        assert!(
            !err.message.contains("ac-02"),
            "deferrable ac-02 must NOT appear in blocking error in: {err}"
        );
    }

    #[test]
    fn spec_close_doc_ac_blocks() {
        // spec 2026-05-16-ac-taxonomy ac-02 verification: AcType::Doc is in
        // the blocking band (Code/Config/Doc all block close).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, true, AcType::Code),
                ac("ac-02", false, false, AcType::Doc), // unchecked, doc, must block
            ],
        );

        let err = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap_err();
        assert!(err.message.contains("ac-02"), "doc AC ac-02 must block: {err}");
    }

    #[test]
    fn spec_close_ops_ac_defers() {
        // spec 2026-05-16-ac-taxonomy ac-02 verification: AcType::Ops is in
        // the deferrable band (Ops/Observation both defer).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0020-orbit-state");
        write_spec_with_acs(
            &layout,
            "0001",
            vec!["0020-orbit-state".into()],
            vec![
                ac("ac-01", false, true, AcType::Code),
                ac("ac-02", false, false, AcType::Ops), // unchecked, ops, must defer
            ],
        );

        let resp = execute(
            &layout,
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into(), force: false }),
        )
        .unwrap();
        let VerbResponse::SpecClose(r) = resp else { panic!() };
        assert_eq!(r.spec.status, SpecStatus::Closed);
        assert_eq!(r.deferrable_open, vec!["ac-02".to_string()]);
    }

    // ========================================================================
    // Spec 2026-05-15-agent-learning-loop — Track A (skill self-improvement)
    // ========================================================================

    fn record_invocation(
        layout: &OrbitLayout,
        skill_id: &str,
        outcome: &str,
        correction: Option<&str>,
        session_id: &str,
        timestamp: Option<&str>,
    ) -> Result<SkillInvocation> {
        let args = SkillRecordInvocationArgs {
            skill_id: skill_id.into(),
            outcome: outcome.into(),
            correction: correction.map(|s| s.to_string()),
            session_id: Some(session_id.into()),
            timestamp: timestamp.map(|s| s.to_string()),
        };
        skill_record_invocation(layout, &args).map(|r| r.invocation)
    }

    #[test]
    fn skill_record_invocation_appends_row() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let inv = record_invocation(&layout, "card", "worked", None, "sess-1", None).unwrap();
        assert_eq!(inv.skill_id, "card");
        assert_eq!(inv.session_id, "sess-1");
        assert_eq!(inv.outcome, InvocationOutcome::Worked);
        assert!(!inv.timestamp.is_empty());

        let path = layout.skill_invocations_file("card");
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().count(), 1, "exactly one JSONL row");

        let parsed: SkillInvocation = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert_eq!(parsed, inv);
    }

    #[test]
    fn skill_record_invocation_rejects_bad_outcome() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let err = record_invocation(&layout, "card", "fantastic", None, "sess-1", None)
            .unwrap_err();
        assert_eq!(err.category, Category::Malformed);
        // The accepted set must surface in the message so agents see the
        // valid options without re-reading the spec.
        for expected in ["worked", "partial", "didnt-apply", "incorrect"] {
            assert!(
                err.message.contains(expected),
                "expected '{expected}' in error message: {}",
                err.message
            );
        }
    }

    #[test]
    fn skill_record_invocation_missing_session_id() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        // Don't set ORBIT_SESSION_ID, don't write .orbit/.session-id.
        // session_id arg also None → should be unavailable.
        let args = SkillRecordInvocationArgs {
            skill_id: "card".into(),
            outcome: "worked".into(),
            correction: None,
            session_id: None,
            timestamp: None,
        };
        let _g = ENV_LOCK.lock().unwrap();
        let prior = std::env::var("ORBIT_SESSION_ID").ok();
        std::env::remove_var("ORBIT_SESSION_ID");
        let result = skill_record_invocation(&layout, &args);
        if let Some(v) = prior {
            std::env::set_var("ORBIT_SESSION_ID", v);
        }
        let err = result.unwrap_err();
        assert_eq!(err.category, Category::Unavailable);
        assert!(err.message.contains("ORBIT_SESSION_ID"));
        assert!(err.message.contains(".session-id"));
    }

    #[test]
    fn skill_record_invocation_omits_null_correction() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        record_invocation(&layout, "card", "worked", None, "sess-1", None).unwrap();
        let body = std::fs::read_to_string(layout.skill_invocations_file("card")).unwrap();
        let line = body.lines().next().unwrap();
        assert!(
            !line.contains("\"correction\""),
            "absent correction must be omitted, not null: {line}"
        );
    }

    #[test]
    fn skill_record_invocation_creates_skills_dir() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        // ensure_dirs deliberately not called — skills_dir is created lazily.
        layout.ensure_dirs().unwrap();
        std::fs::remove_dir_all(layout.skills_dir()).ok();
        assert!(!layout.skills_dir().exists());

        record_invocation(&layout, "card", "worked", None, "sess-1", None).unwrap();
        assert!(layout.skills_dir().is_dir());
    }

    #[test]
    fn skill_recurrence_returns_per_outcome_counts() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        for (outcome, sess, t) in [
            ("worked", "s1", "2026-05-15T10:00:00Z"),
            ("worked", "s2", "2026-05-15T11:00:00Z"),
            ("partial", "s1", "2026-05-15T12:00:00Z"),
            ("incorrect", "s2", "2026-05-15T13:00:00Z"),
        ] {
            record_invocation(&layout, "design", outcome, None, sess, Some(t)).unwrap();
        }

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: None,
            },
        )
        .unwrap();
        assert_eq!(resp.total, 4);
        assert_eq!(resp.by_outcome.worked.count, 2);
        assert_eq!(resp.by_outcome.partial.count, 1);
        assert_eq!(resp.by_outcome.didnt_apply.count, 0);
        assert_eq!(resp.by_outcome.incorrect.count, 1);
    }

    #[test]
    fn skill_recurrence_all_outcome_keys_present() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        record_invocation(&layout, "design", "worked", None, "s1", None).unwrap();

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: None,
            },
        )
        .unwrap();
        let json = serde_json::to_value(&resp).unwrap();
        let by = &json["by_outcome"];
        for key in ["worked", "partial", "didnt-apply", "incorrect"] {
            assert!(by[key].is_object(), "missing outcome key: {key}");
            assert!(by[key]["count"].is_number());
            assert!(by[key]["invocations"].is_array());
        }
    }

    #[test]
    fn skill_recurrence_returns_corrections() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        record_invocation(
            &layout,
            "design",
            "incorrect",
            Some("missed the cold-fork contract"),
            "s1",
            Some("2026-05-15T10:00:00Z"),
        )
        .unwrap();

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: None,
            },
        )
        .unwrap();
        assert_eq!(resp.by_outcome.incorrect.invocations.len(), 1);
        assert_eq!(
            resp.by_outcome.incorrect.invocations[0].correction.as_deref(),
            Some("missed the cold-fork contract")
        );
    }

    #[test]
    fn skill_recurrence_omits_null_correction() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        record_invocation(&layout, "design", "worked", None, "s1", None).unwrap();

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: None,
            },
        )
        .unwrap();
        let json = serde_json::to_value(&resp).unwrap();
        let inv = &json["by_outcome"]["worked"]["invocations"][0];
        assert!(
            inv.get("correction").is_none(),
            "correction must be absent (not null) when not recorded: {inv}"
        );
    }

    #[test]
    fn skill_recurrence_filters_by_since() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        record_invocation(&layout, "design", "worked", None, "s1", Some("2026-05-10T00:00:00Z"))
            .unwrap();
        record_invocation(&layout, "design", "worked", None, "s1", Some("2026-05-15T00:00:00Z"))
            .unwrap();

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: Some("2026-05-12T00:00:00Z".into()),
            },
        )
        .unwrap();
        assert_eq!(resp.total, 1);
        assert_eq!(resp.by_outcome.worked.count, 1);
    }

    #[test]
    fn skill_recurrence_empty_when_no_file() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let resp = skill_recurrence(
            &layout,
            &SkillRecurrenceArgs {
                skill_id: "design".into(),
                since: None,
            },
        )
        .unwrap();
        assert_eq!(resp.total, 0);
        assert_eq!(resp.by_outcome.worked.count, 0);
        assert_eq!(resp.by_outcome.partial.count, 0);
        assert_eq!(resp.by_outcome.didnt_apply.count, 0);
        assert_eq!(resp.by_outcome.incorrect.count, 0);
    }

    // ========================================================================
    // Spec 2026-05-15-agent-learning-loop — Track B (session continuity)
    // ========================================================================

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn session_start_writes_uuid_v4() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r = session_start(&layout, &SessionStartArgs::default()).unwrap();
        let uuid: uuid::Uuid = r.session_id.parse().expect("session id must be a UUID");
        assert_eq!(uuid.get_version(), Some(uuid::Version::Random));

        let on_disk = std::fs::read_to_string(layout.session_id_file()).unwrap();
        assert_eq!(on_disk.trim(), r.session_id);
    }

    #[test]
    fn session_start_with_id_arg_uses_verbatim() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r = session_start(
            &layout,
            &SessionStartArgs {
                id: Some("fixture-session-42".into()),
            },
        )
        .unwrap();
        assert_eq!(r.session_id, "fixture-session-42");
        assert_eq!(
            std::fs::read_to_string(layout.session_id_file())
                .unwrap()
                .trim(),
            "fixture-session-42"
        );
    }

    #[test]
    fn session_distill_first_call_creates_file() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-A".into()),
                distillate: "first reflection".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.id, "sess-A");
        assert_eq!(r.session.distillate, "first reflection");
        assert_eq!(r.session.started_at, r.session.ended_at.as_deref().unwrap_or(""));
        assert!(layout.session_file("sess-A").exists());
    }

    #[test]
    fn session_distill_is_idempotent() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r1 = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-B".into()),
                distillate: "v1".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        let started = r1.session.started_at.clone();

        // Sleep briefly so the second call's RFC 3339 timestamp differs.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        let r2 = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-B".into()),
                distillate: "v2".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r2.session.started_at, started, "started_at preserved");
        assert_ne!(
            r2.session.ended_at.as_deref(),
            Some(started.as_str()),
            "ended_at advances"
        );
        assert_eq!(r2.session.distillate, "v2");

        // Exactly one file on disk.
        let count = std::fs::read_dir(layout.sessions_dir()).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn session_distill_resolves_card_id_arg_first() {
        // spec 2026-05-16-session-handover ac-03: explicit --card / card_id
        // arg wins over .orbit/.session-card fallback. No validation at
        // distill time — id is opaque here.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.session_card_file(), "fallback-card\n").unwrap();

        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-card-A".into()),
                distillate: "first".into(),
                card_id: Some("explicit-card".into()),
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.card_id.as_deref(), Some("explicit-card"));
    }

    #[test]
    fn session_distill_falls_back_to_session_card_file() {
        // spec 2026-05-16-session-handover ac-03: when no arg is passed,
        // read .orbit/.session-card and write the trimmed slug to Session.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.session_card_file(), "0036-session-handover\n").unwrap();

        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-card-B".into()),
                distillate: "second".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.card_id.as_deref(), Some("0036-session-handover"));
    }

    #[test]
    fn session_distill_card_id_none_when_no_arg_and_no_file() {
        // spec 2026-05-16-session-handover ac-03: missing .session-card and
        // no arg → card_id stays None. Absence is normal.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-card-C".into()),
                distillate: "third".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.card_id, None);
    }

    #[test]
    fn session_distill_overwrites_card_id_on_subsequent_call() {
        // spec 2026-05-16-session-handover ac-03 idempotency contract:
        // latest write wins for everything except started_at.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let _ = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-card-D".into()),
                distillate: "v1".into(),
                card_id: Some("first-card".into()),
                labels: vec![],
            },
        )
        .unwrap();
        let r2 = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("sess-card-D".into()),
                distillate: "v2".into(),
                card_id: Some("second-card".into()),
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r2.session.card_id.as_deref(), Some("second-card"));
    }

    #[test]
    fn session_distill_does_not_delete_session_id_file() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.session_id_file(), "sess-C\n").unwrap();

        for _ in 0..2 {
            session_distill(
                &layout,
                &SessionDistillArgs {
                    session_id: Some("sess-C".into()),
                    distillate: "x".into(),
                    card_id: None,
                    labels: vec![],
                },
            )
            .unwrap();
        }
        assert!(layout.session_id_file().exists(), "Stop hook owns deletion, not distill");
    }

    #[test]
    fn session_distill_session_id_precedence() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.session_id_file(), "from-file\n").unwrap();

        let _g = ENV_LOCK.lock().unwrap();
        let prior = std::env::var("ORBIT_SESSION_ID").ok();

        // Arg overrides env + file.
        std::env::set_var("ORBIT_SESSION_ID", "from-env");
        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: Some("from-arg".into()),
                distillate: "d".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.id, "from-arg");

        // Env overrides file when arg is absent.
        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: None,
                distillate: "d".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.id, "from-env");

        // File only when env unset.
        std::env::remove_var("ORBIT_SESSION_ID");
        let r = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: None,
                distillate: "d".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.session.id, "from-file");

        match prior {
            Some(v) => std::env::set_var("ORBIT_SESSION_ID", v),
            None => std::env::remove_var("ORBIT_SESSION_ID"),
        }
    }

    #[test]
    fn session_verbs_work_without_hooks() {
        // ac-09 invariant: even with no hooks installed, CLI verbs succeed
        // when ORBIT_SESSION_ID is set in env.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let _g = ENV_LOCK.lock().unwrap();
        let prior = std::env::var("ORBIT_SESSION_ID").ok();
        std::env::set_var("ORBIT_SESSION_ID", "env-only-session");

        let inv = skill_record_invocation(
            &layout,
            &SkillRecordInvocationArgs {
                skill_id: "design".into(),
                outcome: "worked".into(),
                correction: None,
                session_id: None,
                timestamp: None,
            },
        )
        .unwrap();
        assert_eq!(inv.invocation.session_id, "env-only-session");

        let dist = session_distill(
            &layout,
            &SessionDistillArgs {
                session_id: None,
                distillate: "x".into(),
                card_id: None,
                labels: vec![],
            },
        )
        .unwrap();
        assert_eq!(dist.session.id, "env-only-session");

        match prior {
            Some(v) => std::env::set_var("ORBIT_SESSION_ID", v),
            None => std::env::remove_var("ORBIT_SESSION_ID"),
        }
    }

    // ------------------------------------------------------------------------
    // spec 2026-05-16-session-handover — set-card + handover verbs
    // ------------------------------------------------------------------------

    #[test]
    fn session_set_card_writes_canonical_slug_atomically() {
        // ac-04: validate the slug, then write it newline-terminated to
        // .orbit/.session-card. Output echoes the resolved canonical slug
        // and the path.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0036-session-handover");

        let r = session_set_card(
            &layout,
            &SessionSetCardArgs {
                card_id: "0036-session-handover".into(),
            },
        )
        .unwrap();
        assert_eq!(r.card_id, "0036-session-handover");
        let on_disk = std::fs::read_to_string(layout.session_card_file()).unwrap();
        assert_eq!(on_disk, "0036-session-handover\n");
    }

    #[test]
    fn session_set_card_resolves_bare_numeric() {
        // ac-04: bare-NNNN and padded NNNN both resolve via the same
        // prefix-match helper as card.show.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0036-session-handover");

        let r =
            session_set_card(&layout, &SessionSetCardArgs { card_id: "36".into() }).unwrap();
        assert_eq!(r.card_id, "0036-session-handover");

        let r2 =
            session_set_card(&layout, &SessionSetCardArgs { card_id: "0036".into() }).unwrap();
        assert_eq!(r2.card_id, "0036-session-handover");
    }

    #[test]
    fn session_set_card_unknown_card_returns_not_found() {
        // ac-04: unknown card → Error::not_found; nothing is written.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let err = session_set_card(
            &layout,
            &SessionSetCardArgs { card_id: "9999".into() },
        )
        .unwrap_err();
        assert_eq!(err.category, crate::error::Category::NotFound);
        assert!(!layout.session_card_file().exists());
    }

    #[test]
    fn session_set_card_overwrites_existing() {
        // ac-04 + ac-10(g): mid-session re-set-card is legal and overwrites.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0036-session-handover");
        write_card(&layout, "0001-other-card");

        session_set_card(
            &layout,
            &SessionSetCardArgs { card_id: "36".into() },
        )
        .unwrap();
        session_set_card(
            &layout,
            &SessionSetCardArgs { card_id: "1".into() },
        )
        .unwrap();
        let on_disk = std::fs::read_to_string(layout.session_card_file()).unwrap();
        assert_eq!(on_disk, "0001-other-card\n");
    }

    #[test]
    fn session_handover_returns_null_when_no_sessions() {
        // ac-06: empty sessions dir → handover: None (NOT not_found).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        let r = session_handover(&layout, &SessionHandoverArgs::default()).unwrap();
        assert!(r.handover.is_none());
    }

    #[test]
    fn session_handover_global_latest_across_cards() {
        // ac-06: no --card → most-recent session across all cards.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0001-card-a");
        write_card(&layout, "0036-session-handover");

        // Plant three sessions; sess-3 is the latest.
        let plant = |slug: &str, started: &str, card: Option<&str>| {
            let s = Session {
                id: slug.into(),
                started_at: started.into(),
                ended_at: Some(started.into()),
                distillate: format!("hi from {slug}"),
                card_id: card.map(String::from),
                labels: vec![],
            };
            std::fs::write(
                layout.session_file(slug),
                serialise_yaml(&s).unwrap(),
            )
            .unwrap();
        };
        plant("sess-1", "2026-05-15T10:00:00Z", Some("0001-card-a"));
        plant("sess-2", "2026-05-15T11:00:00Z", None);
        plant("sess-3", "2026-05-15T12:00:00Z", Some("0036-session-handover"));

        let r = session_handover(&layout, &SessionHandoverArgs::default()).unwrap();
        let h = r.handover.expect("expected a handover");
        assert_eq!(h.session_id, "sess-3");
    }

    #[test]
    fn session_handover_filters_by_card_and_since() {
        // ac-06: --card filters by card_id; --since drops rows whose
        // started_at lexically predates the cutoff.
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0036-session-handover");
        write_card(&layout, "0001-other-card");

        let plant = |slug: &str, started: &str, card: &str| {
            let s = Session {
                id: slug.into(),
                started_at: started.into(),
                ended_at: Some(started.into()),
                distillate: format!("hi from {slug}"),
                card_id: Some(card.into()),
                labels: vec![],
            };
            std::fs::write(
                layout.session_file(slug),
                serialise_yaml(&s).unwrap(),
            )
            .unwrap();
        };
        plant("sess-old", "2026-05-10T10:00:00Z", "0036-session-handover");
        plant("sess-new", "2026-05-15T12:00:00Z", "0036-session-handover");
        plant("sess-other", "2026-05-15T13:00:00Z", "0001-other-card");

        // Card filter alone.
        let r = session_handover(
            &layout,
            &SessionHandoverArgs {
                card_id: Some("36".into()),
                since: None,
            },
        )
        .unwrap();
        let h = r.handover.expect("expected match");
        assert_eq!(h.session_id, "sess-new");

        // Card + since filter drops sess-old.
        let r = session_handover(
            &layout,
            &SessionHandoverArgs {
                card_id: Some("0036-session-handover".into()),
                since: Some("2026-05-12T00:00:00Z".into()),
            },
        )
        .unwrap();
        let h = r.handover.expect("expected match");
        assert_eq!(h.session_id, "sess-new");

        // Unrecorded card returns Err — caller asked for a card that
        // doesn't exist on disk; this is the not-found path on the
        // cards directory itself (per ac-04 resolution semantics).
        let err = session_handover(
            &layout,
            &SessionHandoverArgs {
                card_id: Some("9999-missing".into()),
                since: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.category, crate::error::Category::NotFound);
    }

    #[test]
    fn session_handover_null_when_card_has_no_sessions() {
        // ac-06: --card pointing at a real card with no sessions returns
        // handover: None (legitimate question, not an error).
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        write_card(&layout, "0036-session-handover");

        let r = session_handover(
            &layout,
            &SessionHandoverArgs {
                card_id: Some("36".into()),
                since: None,
            },
        )
        .unwrap();
        assert!(r.handover.is_none());
    }

    #[test]
    fn session_prime_prefers_label_overlap_memories() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();

        // Spec with labels.
        let spec = Spec {
            id: "0010".into(),
            goal: "do the foo".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec!["foo".into(), "bar".into()],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0010").unwrap();
        std::fs::write(layout.spec_file("0010"), serialise_yaml(&spec).unwrap()).unwrap();

        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "older-overlap".into(),
                body: "matches foo".into(),
                labels: vec!["foo".into()],
                timestamp: Some("2026-05-01T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();
        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "newer-unrelated".into(),
                body: "no overlap".into(),
                labels: vec!["unrelated".into()],
                timestamp: Some("2026-05-14T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();

        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        let keys: Vec<_> = resp.memories.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["older-overlap", "newer-unrelated"],
            "label-overlap memory comes first even when older"
        );
    }

    #[test]
    fn session_prime_falls_back_to_recency_when_no_overlap() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        let spec = Spec {
            id: "0011".into(),
            goal: "x".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec!["xyz".into()],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0011").unwrap();
        std::fs::write(layout.spec_file("0011"), serialise_yaml(&spec).unwrap()).unwrap();

        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "older".into(),
                body: "x".into(),
                labels: vec!["a".into()],
                timestamp: Some("2026-05-01T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();
        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "newer".into(),
                body: "x".into(),
                labels: vec!["b".into()],
                timestamp: Some("2026-05-14T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();

        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        let keys: Vec<_> = resp.memories.iter().map(|m| m.key.as_str()).collect();
        // Both have zero overlap; tie-break by timestamp DESC.
        assert_eq!(keys, vec!["newer", "older"]);
    }

    #[test]
    fn session_prime_unchanged_when_no_spec_labels() {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        let spec = Spec {
            id: "0012".into(),
            goal: "x".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        layout.ensure_spec_dir("0012").unwrap();
        std::fs::write(layout.spec_file("0012"), serialise_yaml(&spec).unwrap()).unwrap();

        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "older".into(),
                body: "x".into(),
                labels: vec!["foo".into()],
                timestamp: Some("2026-05-01T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();
        memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "newer".into(),
                body: "x".into(),
                labels: vec!["bar".into()],
                timestamp: Some("2026-05-14T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();

        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        let keys: Vec<_> = resp.memories.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, vec!["newer", "older"]);
    }

    // ----- audit.topology tests (spec 2026-05-18-documentation-topology ac-06) -----

    /// Build a layout rooted at a tmp `.orbit/` dir. The repo root is the
    /// parent of `.orbit/`, so anchor pointers in topology.md resolve from
    /// the tmp dir itself.
    fn fresh_topology_layout() -> (tempfile::TempDir, OrbitLayout) {
        let dir = tempfile::tempdir().unwrap();
        let orbit_dir = dir.path().join(".orbit");
        std::fs::create_dir_all(&orbit_dir).unwrap();
        let layout = OrbitLayout::at_orbit_dir(&orbit_dir);
        (dir, layout)
    }

    #[test]
    fn audit_topology_not_configured_when_config_absent() {
        let (_dir, layout) = fresh_topology_layout();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        assert!(!result.configured);
        assert!(result.topology_drift.is_empty());
    }

    #[test]
    fn audit_topology_not_configured_when_docs_topology_unset() {
        let (_dir, layout) = fresh_topology_layout();
        std::fs::write(layout.config_file(), "{}\n").unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        assert!(!result.configured);
        assert!(result.topology_drift.is_empty());
    }

    #[test]
    fn audit_topology_stale_pointer_when_topology_doc_missing() {
        let (_dir, layout) = fresh_topology_layout();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        assert!(result.configured);
        assert_eq!(result.topology_drift.len(), 1);
        assert_eq!(result.topology_drift[0].drift_kind, "stale_pointer");
    }

    #[test]
    fn audit_topology_clean_when_entries_match_codebase() {
        let (dir, layout) = fresh_topology_layout();
        let repo = dir.path();
        // Create a subsystem dir + an authoritative file inside it so all
        // anchors resolve.
        std::fs::create_dir_all(repo.join("src/auth")).unwrap();
        std::fs::write(repo.join("src/auth/mod.rs"), "// auth module\n").unwrap();
        std::fs::write(repo.join("docs-decision.md"), "# Decision\n").unwrap();
        std::fs::write(repo.join("docs-ops.md"), "# Ops\n").unwrap();
        std::fs::write(repo.join("tests-auth.rs"), "// tests\n").unwrap();
        // Write the topology doc.
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        let topology = "\
# Topology

## auth

- code: src/auth/mod.rs
- decision: docs-decision.md
- operational: docs-ops.md
- tests: tests-auth.rs
- what: handles login and session validation
";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        assert!(result.configured);
        assert!(
            result.topology_drift.is_empty(),
            "expected clean, got {:?}",
            result.topology_drift
        );
    }

    #[test]
    fn audit_topology_detects_stale_pointer_in_entry() {
        let (dir, layout) = fresh_topology_layout();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src/auth")).unwrap();
        std::fs::write(repo.join("src/auth/mod.rs"), "// auth\n").unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        // decision: points at a path that doesn't exist
        let topology = "\
## auth

- code: src/auth/mod.rs
- decision: nonexistent.md
- operational: missing-too.md
- tests: also-missing.rs
- what: handles login
";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        assert!(result.configured);
        let stale: Vec<_> = result
            .topology_drift
            .iter()
            .filter(|d| d.drift_kind == "stale_pointer")
            .collect();
        assert_eq!(stale.len(), 3, "expected 3 stale pointers, got {stale:?}");
    }

    #[test]
    fn audit_topology_detects_shape_drift_when_anchors_missing() {
        let (dir, layout) = fresh_topology_layout();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        // Only two of five anchors present.
        let topology = "\
## auth

- code: src/auth/mod.rs
- what: handles login
";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        let shape: Vec<_> = result
            .topology_drift
            .iter()
            .filter(|d| d.drift_kind == "shape_drift")
            .collect();
        // Missing: decision, operational, tests (3 missing anchors)
        assert_eq!(
            shape.len(),
            3,
            "expected 3 missing anchors, got {shape:?}"
        );
    }

    #[test]
    fn audit_topology_detects_missing_entry_for_undocumented_subsystem() {
        let (dir, layout) = fresh_topology_layout();
        let repo = dir.path();
        // Two subsystems in the codebase, one undocumented.
        std::fs::create_dir_all(repo.join("src/auth")).unwrap();
        std::fs::create_dir_all(repo.join("src/ingest")).unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        // Topology doc only covers `auth`.
        let topology = "\
## auth

- code: src/auth
- decision: docs-d.md
- operational: docs-o.md
- tests: tests.rs
- what: auth subsystem
";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(repo.join("docs-d.md"), "x").unwrap();
        std::fs::write(repo.join("docs-o.md"), "x").unwrap();
        std::fs::write(repo.join("tests.rs"), "x").unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let result = audit_topology(&layout, &AuditTopologyArgs::default()).unwrap();
        let missing: Vec<_> = result
            .topology_drift
            .iter()
            .filter(|d| d.drift_kind == "missing_entry")
            .collect();
        assert_eq!(missing.len(), 1, "expected ingest as missing");
        assert_eq!(missing[0].subsystem, "ingest");
    }

    #[test]
    fn audit_topology_dispatched_through_execute() {
        // The verb is wired through the execute() entry point — confirms
        // VerbRequest::AuditTopology routes to VerbResponse::AuditTopology.
        let (_dir, layout) = fresh_topology_layout();
        let response = execute(&layout, &VerbRequest::AuditTopology(Default::default())).unwrap();
        match response {
            VerbResponse::AuditTopology(result) => {
                assert!(!result.configured);
            }
            other => panic!("expected AuditTopology, got {other:?}"),
        }
    }

    // ============================================================
    // Tests for spec 2026-05-18-topology-substrate-wires
    // ============================================================

    /// Build a topology config + doc at the layout root with the given
    /// subsystems wired (all anchors resolve to the same path, so no
    /// stale_pointer drift).
    fn install_topology(layout: &OrbitLayout, subsystems: &[&str]) {
        let repo = layout.root.parent().unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        let mut topology = String::from("# Topology\n\n");
        // Ensure each anchor target exists so audit_topology stays clean.
        std::fs::create_dir_all(repo.join("src")).unwrap();
        for s in subsystems {
            std::fs::create_dir_all(repo.join(format!("src/{s}"))).unwrap();
            std::fs::write(repo.join(format!("src/{s}/mod.rs")), "// mod\n").unwrap();
            topology.push_str(&format!(
                "## {s}\n\n- code: src/{s}/mod.rs\n- decision: src/{s}/mod.rs\n- operational: src/{s}/mod.rs\n- tests: src/{s}/mod.rs\n- what: subsystem {s}\n\n",
                s = s,
            ));
        }
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
    }

    // ----- ac-02: session_prime topology_drift -----

    #[test]
    fn session_prime_topology_drift_none_when_config_absent() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        assert!(
            resp.topology_drift.is_none(),
            "expected None (key omitted), got {:?}",
            resp.topology_drift
        );
    }

    #[test]
    fn session_prime_topology_drift_none_when_docs_topology_unset() {
        // ac-02 4th case: config file present but docs.topology unset →
        // configured == false → topology_drift key absent.
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.config_file(), "{}\n").unwrap();
        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        assert!(
            resp.topology_drift.is_none(),
            "expected None on config-present-but-docs.topology-absent, got {:?}",
            resp.topology_drift
        );
    }

    #[test]
    fn session_prime_topology_drift_some_empty_when_configured_clean() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_topology(&layout, &["auth"]);
        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        match resp.topology_drift {
            Some(d) => assert!(d.is_empty(), "expected empty drift, got {d:?}"),
            None => panic!("expected Some(empty), got None"),
        }
    }

    #[test]
    fn session_prime_topology_drift_some_populated_when_drift_present() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let repo = layout.root.parent().unwrap();
        // Topology covers `auth` but codebase also has `ingest` → missing_entry.
        std::fs::create_dir_all(repo.join("src/auth")).unwrap();
        std::fs::create_dir_all(repo.join("src/ingest")).unwrap();
        std::fs::write(repo.join("src/auth/mod.rs"), "// auth\n").unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        let topology = "## auth\n\n- code: src/auth/mod.rs\n- decision: src/auth/mod.rs\n- operational: src/auth/mod.rs\n- tests: src/auth/mod.rs\n- what: auth\n";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();
        let resp = session_prime(&layout, &SessionPrimeArgs::default()).unwrap();
        let drift = resp.topology_drift.expect("Some when configured");
        assert!(!drift.is_empty(), "expected populated drift");
        assert!(drift.iter().any(|d| d.subsystem == "ingest" && d.drift_kind == "missing_entry"));
    }

    // ----- ac-03: spec.close topology_warnings -----

    /// Plant a spec + sidecars under `layout.spec_dir(id)` with the given
    /// text inside spec.yaml's goal. Spec ACs are empty so spec.close does
    /// not block.
    fn install_spec_for_warnings(layout: &OrbitLayout, id: &str, spec_text: &str, interview: Option<&str>, design_note: Option<&str>) {
        layout.ensure_spec_dir(id).unwrap();
        let spec = Spec {
            id: id.into(),
            goal: spec_text.to_string(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        };
        std::fs::write(layout.spec_file(id), serialise_yaml(&spec).unwrap()).unwrap();
        if let Some(body) = interview {
            std::fs::write(layout.spec_dir(id).join("interview.md"), body).unwrap();
        }
        if let Some(body) = design_note {
            std::fs::write(layout.spec_dir(id).join("design-note.md"), body).unwrap();
        }
    }

    #[test]
    fn spec_close_topology_warnings_populated_on_word_boundary_match() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_topology(&layout, &["session_prime"]);
        install_spec_for_warnings(
            &layout,
            "0001",
            "Adding a topology_drift field to session_prime envelope.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0001".into(), force: false }).unwrap();
        assert!(
            result.topology_warnings.iter().any(|w| w.subsystem == "session_prime"),
            "expected session_prime warning, got {:?}",
            result.topology_warnings
        );
    }

    #[test]
    fn spec_close_topology_warnings_empty_on_substring_only() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_topology(&layout, &["session_prime"]);
        // Substring (no word boundaries — "session_primer" contains
        // "session_prime" as a substring but not on word boundaries).
        install_spec_for_warnings(
            &layout,
            "0002",
            "Spec touches the session_primer module which is unrelated.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0002".into(), force: false }).unwrap();
        assert!(
            !result.topology_warnings.iter().any(|w| w.subsystem == "session_prime"),
            "substring should not match: {:?}",
            result.topology_warnings
        );
    }

    #[test]
    fn spec_close_topology_warnings_excludes_short_subsystem_names() {
        // ≥5 char filter — "memo" (4 chars) should be excluded even when
        // matched in the spec text.
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_topology(&layout, &["memo"]);
        install_spec_for_warnings(
            &layout,
            "0003",
            "We propagate memo handling across the new layer.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0003".into(), force: false }).unwrap();
        assert!(
            !result.topology_warnings.iter().any(|w| w.subsystem == "memo"),
            "4-char subsystem must be filtered out: {:?}",
            result.topology_warnings
        );
    }

    #[test]
    fn spec_close_topology_warnings_match_in_design_note_only() {
        // ac-03 cycle-1 LOW: design-note.md must be in the scan set, not
        // just spec.yaml + interview.md.
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_topology(&layout, &["session_prime"]);
        install_spec_for_warnings(
            &layout,
            "0004",
            "Goal text mentions nothing relevant.",
            Some("# Interview\n\nNo subsystem names here.\n"),
            Some("# Design Note\n\nThis pinned approach extends session_prime.\n"),
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0004".into(), force: false }).unwrap();
        assert!(
            result.topology_warnings.iter().any(|w| w.subsystem == "session_prime"),
            "design-note.md must be scanned: {:?}",
            result.topology_warnings
        );
    }

    #[test]
    fn spec_close_topology_warnings_regex_escape_on_metachars() {
        // ac-03 cycle-2 LOW carried from parent: regex::escape must be
        // applied to subsystem names so metacharacters match literally.
        // Without escape, `foo.bar` would match `fooXbar` (any character)
        // because `.` is a regex wildcard.
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let repo = layout.root.parent().unwrap();
        // Install a topology entry with a metachar-bearing subsystem name.
        // Anchors point at the repo root so they resolve.
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        let topology = "## foo.bar\n\n- code: docs/topology.md\n- decision: docs/topology.md\n- operational: docs/topology.md\n- tests: docs/topology.md\n- what: meta-charged subsystem\n";
        std::fs::write(repo.join("docs/topology.md"), topology).unwrap();
        std::fs::write(
            layout.config_file(),
            "docs:\n  topology: docs/topology.md\n",
        )
        .unwrap();

        // Literal "foo.bar" present — must match.
        install_spec_for_warnings(
            &layout,
            "0005",
            "We have foo.bar in the spec text.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0005".into(), force: false }).unwrap();
        assert!(
            result.topology_warnings.iter().any(|w| w.subsystem == "foo.bar"),
            "literal foo.bar should match: {:?}",
            result.topology_warnings
        );

        // Different spec — "fooXbar" present, "foo.bar" not. Without
        // regex::escape, the `.` would be a wildcard and match the X.
        install_spec_for_warnings(
            &layout,
            "0006",
            "We have fooXbar in the spec text but not the literal name.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0006".into(), force: false }).unwrap();
        assert!(
            !result.topology_warnings.iter().any(|w| w.subsystem == "foo.bar"),
            "fooXbar must NOT match foo.bar — proves regex::escape is applied: {:?}",
            result.topology_warnings
        );
    }

    #[test]
    fn spec_close_topology_warnings_empty_when_not_configured() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        install_spec_for_warnings(
            &layout,
            "0007",
            "session_prime mentioned but topology not configured.",
            None,
            None,
        );
        let result = spec_close(&layout, &SpecCloseArgs { id: "0007".into(), force: false }).unwrap();
        assert!(
            result.topology_warnings.is_empty(),
            "no warnings when capability unconfigured: {:?}",
            result.topology_warnings
        );
    }

    // ----- ac-04: memory.remember topology nudge -----

    #[test]
    fn memory_remember_topology_label_emits_nudge() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let result = memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "k-with-topology".into(),
                body: "body".into(),
                labels: vec!["topology".into()],
                timestamp: Some("2026-05-18T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();
        assert!(
            result.nudge.is_some(),
            "expected nudge populated when topology label present"
        );
        let nudge = result.nudge.unwrap();
        assert!(
            nudge.contains("/orb:topology"),
            "nudge text must mention /orb:topology, got {nudge}"
        );
    }

    #[test]
    fn memory_remember_without_topology_label_emits_no_nudge() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let result = memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "k-no-label".into(),
                body: "body".into(),
                labels: vec!["unrelated".into()],
                timestamp: Some("2026-05-18T00:00:00Z".into()),
                no_nudge: false,
            },
        )
        .unwrap();
        assert!(
            result.nudge.is_none(),
            "no nudge when topology label absent, got {:?}",
            result.nudge
        );
    }

    #[test]
    fn memory_remember_no_nudge_flag_suppresses_nudge() {
        let (_dir, layout) = fresh_topology_layout();
        layout.ensure_dirs().unwrap();
        let result = memory_remember(
            &layout,
            &MemoryRememberArgs {
                key: "k-suppressed".into(),
                body: "body".into(),
                labels: vec!["topology".into()],
                timestamp: Some("2026-05-18T00:00:00Z".into()),
                no_nudge: true,
            },
        )
        .unwrap();
        assert!(
            result.nudge.is_none(),
            "--no-nudge must suppress even with topology label, got {:?}",
            result.nudge
        );
    }

    #[test]
    fn memory_remember_canonical_nudge_text_const() {
        // Lock the canonical text via a const — tests grep for this to
        // confirm the implementation matches the documented contract.
        assert!(TOPOLOGY_NUDGE.contains("consider /orb:topology"));
    }
}
