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
    AcceptanceCriterion, Card, Choice, Memory, NoteEvent, Spec, SpecStatus, TaskEvent,
    TaskEventKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
    #[serde(rename = "choice.show")]
    ChoiceShow(ChoiceShowArgs),
    #[serde(rename = "choice.list")]
    ChoiceList(ChoiceListArgs),
    #[serde(rename = "choice.search")]
    ChoiceSearch(ChoiceSearchArgs),
    #[serde(rename = "session.prime")]
    SessionPrime(SessionPrimeArgs),
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecCloseArgs {
    pub id: String,
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
    #[serde(rename = "choice.show")]
    ChoiceShow(ChoiceShowResult),
    #[serde(rename = "choice.list")]
    ChoiceList(ChoiceListResult),
    #[serde(rename = "choice.search")]
    ChoiceSearch(ChoiceListResult),
    #[serde(rename = "session.prime")]
    SessionPrime(SessionPrimeResult),
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecCloseResult {
    pub spec: Spec,
    pub cards_updated: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRememberResult {
    pub memory: Memory,
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
    /// Hard upper bound on items: 40 + 2*open_specs + min(memory_cap, 10).
    pub item_bound: usize,
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
        VerbRequest::ChoiceShow(args) => choice_show(layout, args).map(VerbResponse::ChoiceShow),
        VerbRequest::ChoiceList(args) => choice_list(layout, args).map(VerbResponse::ChoiceList),
        VerbRequest::ChoiceSearch(args) => {
            choice_search(layout, args).map(VerbResponse::ChoiceSearch)
        }
        VerbRequest::SessionPrime(args) => {
            session_prime(layout, args).map(VerbResponse::SessionPrime)
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

    Ok(SpecCloseResult { spec, cards_updated })
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
    Ok(MemoryRememberResult { memory })
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

    // Memories — sort by timestamp DESC, take up to cap.
    let mut memories = read_all_memories(layout, VERB)?;
    memories.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let effective = cap.min(memories.len());
    memories.truncate(effective);

    let item_bound = 40 + 2 * open_specs.len() + cap.min(DEFAULT_MEMORY_CAP);

    Ok(SessionPrimeResult {
        open_specs,
        memories,
        item_bound,
    })
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
            &VerbRequest::SpecClose(SpecCloseArgs { id: "0001".into() }),
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
}
