//! orbit-state-core — the files-canonical agent substrate.
//!
//! Layering:
//!   - [`error`]   : the single error taxonomy (`<verb>: <category>: <sentence>`).
//!   - [`schema`]  : strongly-typed entity definitions with `deny_unknown_fields`.
//!   - [`canonical`] : LF-only, deterministic-key serialiser + parser entry points.
//!   - [`atomic`]  : temp + rename writes; CRLF-rejecting line policy.
//!
//! Higher layers (verbs, index, locks, MCP) build on these.

pub mod atomic;
pub mod canonical;
pub mod canonicalise;
pub mod error;
pub mod index;
pub mod layout;
pub mod locks;
pub mod migrate;
pub mod migrations;
pub mod reconcile;
pub mod schema;
pub mod session;
pub mod sqlite_link;
pub mod verbs;
pub mod verify;

pub use canonicalise::{canonicalise_all, CanonicaliseReport};
pub use migrate::{migrate_spec_layout, MigrateReport, PlannedMove};
pub use reconcile::{reconcile_all, Disposition, DispositionRecord, EntityType, ReconcileReport};
pub use error::{Category, Error, Result};
pub use sqlite_link::{link_sanity_check, sqlite_version};
pub use verify::{verify_all, RoundTripFailure, RoundTripFailureKind, VerifyOutcome};
pub use verbs::{
    envelope_err, envelope_err_string, envelope_ok, envelope_ok_string, execute, CardListArgs,
    AuditDriftArgs, AuditDriftResult, CardListResult, CardMaturityCounts, CardSearchArgs,
    CardShowArgs, CardShowResult, CardSpecsArgs, CardSpecsEntry, CardSpecsResult, CardSummary,
    CardTreeArgs, CardTreeEdge, CardTreeNode, CardTreeResult, ChoiceListArgs, DriftEntry,
    GraphArgs, GraphFormat, GraphResult, MostConnectedCard, OverviewArgs, OverviewResult,
    ChoiceListResult, ChoiceSearchArgs, ChoiceShowArgs, ChoiceShowResult, ChoiceSummary,
    MemoryListArgs, MemoryListResult, MemoryRememberArgs, MemoryRememberResult, MemorySearchArgs,
    HandoverSummary, RecurrenceBucket, RecurrenceByOutcome, RecurrenceInvocation,
    SessionDistillArgs, SessionDistillResult, SessionHandoverArgs, SessionHandoverResult,
    SessionPrimeArgs, SessionPrimeResult, SessionSetCardArgs, SessionSetCardResult,
    SessionStartArgs, SessionStartResult, SkillRecordInvocationArgs, SkillRecordInvocationResult,
    SkillRecurrenceArgs, SkillRecurrenceResult, SpecCloseArgs, SpecCloseResult, SpecCreateArgs,
    SpecCreateResult, SpecListArgs, SpecListResult, SpecNoteArgs, SpecNoteResult, SpecShowArgs,
    SpecShowResult, SpecSummary, SpecUpdateArgs, SpecUpdateResult, TaskClaimArgs, TaskDoneArgs,
    TaskEventResult, TaskListArgs, TaskListResult, TaskOpenArgs, TaskOpenResult, TaskReadyArgs,
    TaskShowArgs, TaskShowResult, TaskState, TaskUpdateArgs, VerbRequest, VerbResponse,
};
