---
name: memo
description: Quickly jot a rough idea and file it in `.orbit/memos/`
---

# /orb:memo

Capture a rough idea as freeform markdown. No structure required, no interview, no ceremony. Just write it down and move on. The SessionStart hook will surface outstanding memos until you distill them into cards.

## Usage

```
/orb:memo [slug]
```

Where `[slug]` is an optional short label for the memo (e.g. `pipeline-progress`). If omitted, you'll be asked for one.

## Why This Exists

Not every idea is ready for a feature card. Sometimes you just need to jot something down before the context is lost. Memos are the lowest-friction entry point into orbit — write a sentence or a page, and the workflow will remind you about it until you act on it.

## Instructions

### 1. Determine the Slug

- If `$ARGUMENTS` contains a non-empty value, use it as the slug
- If no argument is provided, ask the author: **"What's a short label for this memo?"** (e.g. `pipeline-progress`, `auth-rethink`, `perf-concern`)
- Normalise the slug: lowercase, hyphens instead of spaces, no special characters

### 2. Ensure the Directory Exists

Check that `.orbit/memos/` exists. If not, create it (including `.orbit/cards/` if needed).

### 3. Capture the Memo

Ask the author: **"What's the idea?"**

Use **AskUserQuestion** with no suggested answers — this is freeform. The author's response becomes the memo content verbatim.

**Rules:**
- The memo content is the author's exact text. Do not add frontmatter, headers, metadata, or structure.
- Do not edit, summarise, or reformat the author's words.
- Any markdown content is valid — a single sentence, a bulleted list, multiple paragraphs. There are no requirements.

### 4. Write the File

Save the memo as:

```
.orbit/memos/YYYY-MM-DD-<slug>.md
```

Where `YYYY-MM-DD` is today's date.

If a file with the same name already exists, append a numeric suffix: `YYYY-MM-DD-<slug>-2.md`, `YYYY-MM-DD-<slug>-3.md`, etc.

### 5. Confirm

Report back:

```
Memo saved: .orbit/memos/YYYY-MM-DD-<slug>.md
```

Suggest next steps:
- **Write more memos** if there are other ideas floating around
- **`/orb:distill .orbit/memos/YYYY-MM-DD-<slug>.md`** when ready to turn this into a feature card

## Integration with Other Skills

- **SessionStart hook** — surfaces outstanding memos (those not yet referenced by any card)
- **`/orb:distill`** — reads a memo, extracts candidate feature cards, and deletes the memo after writing cards
- **`/orb:card`** — if the idea is clear enough, skip memo and go straight to card

---

**Next step:** When you're ready, run `/orb:distill` on this memo to extract a feature card.
