# orbit

This repo is the orbit workflow plugin for Claude Code. Sessions here are about **workflow refinement** — improving how orbit guides the card → design → spec → implement → review pipeline.

Agent-to-Hugh prose follows the BLUF / Decision Brief contract — see card 0026 (`.orbit/cards/0026-executive-communication.yaml`). The canonical source is `.orbit/STYLE.md`, imported below.

@.orbit/STYLE.md

## What This Is

orbit is a Claude Code plugin that provides specification-driven workflow skills (`/orb:card`, `/orb:distill`, `/orb:design`, `/orb:spec`, `/orb:implement`, `/orb:review-pr`, etc.). The skills, hooks, and card format are the product.

## Working in This Repo

- **Skills live in** `plugins/orb/skills/<name>/SKILL.md`
- **Cards describe orbit's own capabilities** in `.orbit/cards/`
- **Specs for orbit changes** live in `.orbit/specs/`
- orbit uses itself — cards, specs, and decisions apply to orbit's own development

## Deployment

The plugin is installed into projects via the Claude Code plugin marketplace. All development happens in this repo — installed copies in other projects receive updates via the marketplace.

## Push discipline

- Work is not complete until `git push` succeeds. Run `git pull --rebase && git push` at session end and confirm `git status` shows "up to date with origin".
- Run quality gates (tests, linters, builds) before push when code changed.
- Clean up stashes and prune remote branches at session end.
- If push fails, resolve and retry until it succeeds.

@.orbit/METHOD.md
