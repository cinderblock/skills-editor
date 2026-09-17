# Memory support (auto memory, as its own thing)

## Goal

Split the one "Memory" tab into two, because they are different kinds of
thing:

- **Config** — files *you* write that Claude reads: CLAUDE.md, rules,
  imports, other agents' instruction files. (Built in `plans/instructions.md`;
  this plan only renames the tab and moves memory out of it.)
- **Memory** — notes *Claude* writes for itself: auto memory per project and
  subagent memory. First-class support: the index vs. its topic files, what
  each note is (type, description, age), whether the index and files agree,
  and creating/deleting notes.

## Reference (fetched 2026-09-16/17)

- Auto memory dir: `~/.claude/projects/<encoded>/memory/`, or
  `autoMemoryDirectory`. `MEMORY.md` is the index: only its first 200 lines
  or 25 KB load. Topic files load on demand, written by Claude.
- Topic frontmatter per docs: `name`, `description`, `type` (user | feedback
  | project | reference), plus `modified` written by Claude Code ≥ 2.1.214.
- Subagent memory (`memory:` in a subagent's frontmatter):
  - `user` → `~/.claude/agent-memory/<agent>/`
  - `project` → `.claude/agent-memory/<agent>/`
  - `local` → `.claude/agent-memory-local/<agent>/`
  Same `MEMORY.md` 200-line / 25 KB rule. Turning auto memory off disables
  subagent memory too.
- Subagent definitions: `~/.claude/agents/` and `.claude/agents/`, scanned
  recursively.

## This machine (2026-09-17)

- 268 topic files across ~60 auto-memory folders.
- Frontmatter in the wild: **189** use nested `metadata.type` (with
  `node_type`, `originSessionId`), **75** use top-level `type`, **4** have no
  frontmatter at all, and only **2** carry `modified`. Read all shapes; write
  the documented one.
- Index lines look like `- [Title](file.md) — hook`.
- No `agent-memory` folders exist yet (support them anyway; cheap).

## Decisions

1. Tab order: Skills | Hooks | Config | Memory.
2. A **store** is one memory folder: a project's auto memory, a custom
   `autoMemoryDirectory`, a subagent's memory, or a stray folder matching no
   registered project. Subagent stores show even when empty if an agent
   declares `memory:`.
3. Each store reports index vs. files disagreements: notes missing from the
   index, index links pointing at missing files, index past its load limit.
4. Entries show name, type, description and age; editing a note gets the
   structured frontmatter panel (name, description, type) like SKILL.md.
   Writing uses top-level `type`; an existing `metadata.type` is updated in
   place instead, so Claude's own files keep their shape.
5. Creating a note writes the documented frontmatter and (opt-in, default on)
   adds the index line. Deleting offers to drop its index line too.
6. The auto-memory on/off switch appears on the store view as well as in the
   Config tab's startup context (same backend call).

## Plan / steps

1. [x] Research; confirm real-world frontmatter shapes.
2. [x] Backend `memory.rs`: stores, index parsing/consistency, entries,
   create/delete/add-to-index, guard, sync; move memory out of
   `instructions.rs` (keep the auto-memory state + MEMORY.md startup entry).
3. [x] Frontend: Config tab renamed, Memory tab added (sidebar with type
   chips, store view, note editor with frontmatter panel incl. type,
   new-note dialog), `memoryModel.ts` + 6 bun tests.
4. [x] Verified in the running app against real data (screenshots): 261
   notes across ~30 stores, chips count user 8 / feedback 113 / project 93 /
   reference 47, 8 notes not in any index; a note with `metadata.type` reads
   and shows as `feedback`; Config tab no longer lists memory.
   Checks: 36 Rust, 26 frontend, build clean.
5. [x] README, commit `96b7806` (amended for quoting), pushed; CI run
   35255998789 green.
6. [ ] Not released — needs a version bump + tag when the user wants one.

## Findings

- Real finding from the live app: `tomsawyerlabs.com` has 2 notes and **no
  MEMORY.md**, so Claude would never load them. Exactly the kind of thing
  this tab is meant to surface.
- Nothing was created/deleted/toggled on real data during verification.

## Things not to do

- Don't rewrite `metadata.type` files into the documented shape wholesale —
  edit in place, where the field already is.
- Don't auto-update `modified` on a human edit (it records Claude's writes).
- Don't list 57 empty auto-memory folders: show a store when its folder
  exists, or when a subagent declares memory.
