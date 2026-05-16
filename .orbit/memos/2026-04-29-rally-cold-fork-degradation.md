# Rally sub-agents silently lose cold-fork review capability

**Date:** 2026-04-29
**Source:** McGill (post-rally observation on card 0027)

## Observation

During a 6-card rally, at least one drive sub-agent (card 0027) could not spawn its cold-fork reviewer Agents. The Agent tool was not in the sub-agent's deferred-tool list, and `ToolSearch select:Agent` returned nothing. The sub-agent fell back to running review-spec and review-pr inline — self-reviewing in its own context.

The sub-agent wrote canonical-format verdict files so drive's file-on-disk parsing still worked, but the cold-fork separation (decisions 0005, 0006) was violated. The remaining 5 cards may have hit the same issue silently.

## Root cause

The Claude Code harness derives the deferred-tool set per Agent spawn from the `subagent_type` definition, not from the parent's tool surface. `subagent_type: general-purpose` is documented as "all tools" but at nesting depth 2+ (rally lead -> drive sub-agent -> review fork), the Agent tool may not appear in the deferred list. This is a harness-level constraint, not an orbit bug.

Nesting structure where it fails:

```
rally lead (top-level session)
  └─ Agent(general-purpose, run_in_background) -> drive sub-agent
       └─ Agent(general-purpose) -> review-spec fork   <- Agent tool missing here
       └─ Agent(general-purpose) -> review-pr fork     <- Agent tool missing here
```

## Impact

- **Cold-fork quality gate silently bypassed.** Drive's review architecture (decisions 0004-0007) depends on fresh-context reviewers who read the spec/diff cold. Inline review is self-review — the implementer mind-shortcuts past obvious issues.
- **Silent degradation.** Only one sub-agent surfaced the fallback explicitly. Others may have degraded without reporting.

## Fix applied (path 1 — fail loudly)

Added ToolSearch pre-flight to drive SKILL.md at:
- **s5.3** (review-spec fork): `ToolSearch select:Agent` before invoking the Agent tool. If unavailable, escalate with `status: escalated` rather than falling back to inline review.
- **s7.3** (review-pr fork): Same pre-flight.

Added `tool_surface_incomplete` as a sixth reason_label to rally SKILL.md:
- **s7c brief**: Sub-agent can now report this label.
- **s9 table**: Rally absorbs the escalation as a single-strike park.

This converts silent degradation to an honest park — the card can be driven individually later with full cold-fork capability.

## Proper fix (path 2 — harness feature, not yet actionable)

The Agent tool's spawn API accepts `subagent_type` but not a tool whitelist override. If the spawn API allowed explicitly enumerating required tools for the sub-agent, rally could guarantee Agent availability at all nesting depths. This is a Claude Code platform constraint — not patchable from the orbit plugin.

## Status

Path 1 shipped. Path 2 tracked here for future harness capability.
