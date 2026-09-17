# Instructions & memory support (CLAUDE.md et al.)

## Goal

A third sidebar view ("Memory", named after Claude Code's `/memory`) that finds
and edits every instruction/memory file Claude Code loads — managed, user,
project, local, rules, nested, ancestor-folder, imports, and auto memory — plus
the equivalent files other agents use (AGENTS.md etc.). Also shows, per
project, what actually loads at session start and roughly what it costs.

## Environment / context

- Continues `plans/skills-editor.md`, `plans/hooks.md`,
  `plans/release-pipeline.md` (v0.1.0 released 2026-09-16).
- Machine facts: `~/.claude.json` registers the home dir itself as a project
  (so its `.claude/CLAUDE.md` IS the user file) — dedupe by canonical path.
  Auto memory dirs exist under `~/.claude/projects/<encoded>/memory/`.
- Encoding of `<encoded>`: every char outside `[A-Za-z0-9]` → `-`, case kept.
  Verified against existing dirs, e.g. `C:\Users\me\.t3\worktrees\X` →
  `C--Users-me--t3-worktrees-X`. Docs: derived from the git repo root, so
  subdirs/worktrees share one dir.

## Reference (code.claude.com/docs/en/memory, fetched 2026-09-16)

- Load order, broad → specific, all concatenated (nothing overrides):
  managed `C:\Program Files\ClaudeCode\CLAUDE.md` (also `claudeMd` string in
  managed-settings.json; can't be excluded) → `~/.claude/CLAUDE.md` →
  `~/.claude/rules/**/*.md` (before project rules) → `CLAUDE.md` +
  `CLAUDE.local.md` in every directory from filesystem root down to cwd
  (`CLAUDE.local.md` after `CLAUDE.md` per dir) → project `./CLAUDE.md` or
  `./.claude/CLAUDE.md` → `.claude/rules/**/*.md` without `paths` (same
  priority as `.claude/CLAUDE.md`) → auto memory `MEMORY.md` (first 200 lines
  or 25 KB).
- On demand: `CLAUDE.md`/`CLAUDE.local.md` in subdirectories (when Claude
  reads files there); rules with `paths:` globs (when a matching file is read);
  auto-memory topic files (Claude reads them itself).
- `@path` imports: relative to the importing file, absolute, or `~/`; max 4
  hops; ignored inside code spans and fenced blocks; project imports resolving
  outside the working dir need a one-time approval.
- Claude Code does NOT read `AGENTS.md`; recommended bridge is `@AGENTS.md`
  in CLAUDE.md.
- Block-level HTML comments are stripped before injection.
- Files over 4 MiB are skipped; >200 lines hurts adherence (guidance).
- `claudeMdExcludes`: glob list matched against absolute paths, any settings
  layer, arrays merge; managed CLAUDE.md can't be excluded.
- Auto memory: on by default; `autoMemoryEnabled` (user or project settings),
  `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `autoMemoryDirectory` (absolute or
  `~/`). Topic files carry frontmatter `name`, `description`, `type`
  (user/feedback/project/reference), `modified`.

## Decisions (made here — revisit only if the user objects)

1. **Groups mirror skills/hooks**: Managed (read-only), User, one per project
   that has any instruction file, "Parent folders" (ancestor CLAUDE.md files
   that apply to registered projects), and "Other auto memory" (memory dirs
   that match no registered project). Same canonical file appears once.
2. **Sections inside a group**: instructions, rules, nested (on demand), auto
   memory, other agents. Imports appear under the file that imports them.
3. **Badges, not prose**: local, on demand, paths-scoped, excluded, over
   limit/truncated, not read by Claude Code, missing import.
4. **Startup context view** per project: the ordered list of what loads at
   launch with sizes and a rough token estimate (bytes / 4 — labelled rough).
5. **Editing reuses EditorPane** (markdown). Create missing standard files
   (user/project/`.claude`/local CLAUDE.md, new rule, AGENTS.md) with an
   option to add `CLAUDE.local.md` to `.gitignore`. Delete only with confirm.
6. **Auto memory toggle** edits `autoMemoryEnabled` surgically (user level or
   a project's `.claude/settings.local.json`), same technique as hooks.
7. **Nested scan is bounded**: `ignore` crate walker (respects .gitignore),
   depth ≤ 8, entry budget per project, and skipped for the home dir and drive
   roots (a registered home "project" would otherwise walk everything).
8. **File guard is structural** (known names under known roots) plus a cached
   set of resolved imports from the last scan — no full rescan per read.
9. **Sync** snapshots user/project instruction files, rules, nested files and
   auto memory under `instructions/` and `memory/`. Imports aren't copied
   (they're arbitrary files, usually already in the project repo).
10. **Other agents** listed: `AGENTS.md`, `GEMINI.md`,
   `.github/copilot-instructions.md`, `.cursorrules`, `.cursor/rules/*`,
   `.windsurfrules`, `.clinerules`; user-level `~/.codex/AGENTS.md`,
   `~/.gemini/GEMINI.md`. Marked "imported" when a Claude file imports them.

## Plan / steps

1. [x] Research + design.
2. [x] Backend `instructions.rs`: discovery, encoding, imports, excludes,
   rules frontmatter, load-order + sizes, other agents, bounded nested scan
   (parallel, cached 120 s; Refresh forces).
3. [x] Backend ops: create (+ .gitignore), delete, autoMemoryEnabled toggle;
   file-guard (`files::check_allowed` now takes a write flag; managed is
   read-only) + sync (`instructions/`, `memory/`); tests: 8 unit + 1
   end-to-end against the fake home + sync mapping test (31 Rust total).
4. [x] Frontend: Memory tab (filter box, folded auto-memory sections),
   `InstructionInfo` strip in EditorPane, `StartupContext` view with the
   auto-memory switch, `NewInstructionDialog`; `instructionsModel.ts` with
   6 bun tests (20 frontend total).
5. [x] Verified in the running dev app against real files (screenshots):
   user CLAUDE.md shows the 257-line warning; t3code's startup context lists
   `@AGENTS.md` nested under its CLAUDE.md; AGENTS.md marked "imported";
   New-file dialog defaults sensibly. Nothing was created, deleted, or
   toggled on real files.
6. [ ] README ✓, commit, push, CI. ← current

## Findings / gotchas

- Live scan of this machine: 57 registered projects, ~5.8 s cold (the
  nested walk dominates) → cached per project for 120 s; focus refreshes
  reuse the cache, the Refresh button forces.
- 75 auto-memory folders exist but most belong to old t3 worktrees that
  aren't registered projects → "Other auto memory" group, collapsed.
- `~/.claude.json` registers the home folder as a project; its `.claude`
  files are the user files (deduped) and nesting isn't scanned there. The
  group is labelled "Home folder (~)" and holds its auto memory.
- A registered project can live under `~/.claude` (e.g. a skill folder),
  so "is this a nested rules file" must use the path *inside* the project.
- Real `t3code/CLAUDE.md` is just `@AGENTS.md` — the import bridge the docs
  recommend works as modelled (AGENTS.md shown as loaded via import).
- The agent badge "AGENTS.md (Codex, others)" squeezed the file name to
  "AG…" — shortened to "Codex etc.".

## Progress log

- [x] Removed the looping `Stop` agent hook from `~/.claude/settings.json`
  at the user's request (backup `settings.json.bak-remove-stop-hook-*`).
- [x] Noticed the force-push guard hook is currently *disabled via Skills
  Editor* (parked in the app's `disabled-hooks.json`, written 15:48 after
  the agent's test session ended) — reported to the user, not touched.

## Open questions for the user

1. Re-enable the force-push guard? (Recommend yes — CLAUDE.md relies on it.)

## Things not to do

- Don't walk the home directory or drive roots for nested CLAUDE.md files.
- Don't copy `@import` targets into the sync repo.
- Don't treat `AGENTS.md` as loaded by Claude Code unless imported.
