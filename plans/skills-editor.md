# Skills Editor — greenfield Tauri app

## Goal

A small IDE dedicated to managing AI agent skills (Claude first, extensible to other
agents). Discover skills across the machine, edit them (markdown + frontmatter aware),
install popular skills from a catalog, and track a machine's skills in a dedicated git
repo so they can be shared between hosts (per-host branches, cherry-pick friendly).

## Environment / context

- Windows 11, Bun 1.3.0, Rust 1.97.1, git 2.45.2, Tauri v2 (create-tauri-app react-ts).
- Project: `~\git\vibed-out\skills-editor`, fresh git repo on `master`.
- Real skill layouts on this machine:
  - User skills: `~/.claude/skills/<name>/SKILL.md` (+ optional extra files/dirs).
    Frontmatter: `name`, `description` (at minimum).
  - Project skills: `<project>/.claude/skills/<name>/SKILL.md`.
  - Plugins keep skills under `~/.claude/plugins/cache/...` (marketplace clones).
- **Project discovery**: `~/.claude.json` → `projects` object keys are real absolute
  paths (57 on this machine). Much better than decoding the lossy
  `~/.claude/projects/C--Users-...` dir-name encoding. Use `.claude.json`; the encoded
  dirs are only a fallback idea, not implemented.

## Decisions already made (don't re-ask)

- Bun for JS (global rule). Branch `master` (global rule). No `title=` attributes in UI.
- Rust backend shells out to system `git` (no git2 crate) — matches user's git-centric
  workflow, keeps build light, and cherry-pick/branch tooling is free.
- Editor: CodeMirror 6 (@uiw/react-codemirror) — markdown, yaml, json, js, python, html,
  css highlighting by extension. No prettier integration in v1 ("simple edits").
- Frontmatter: parsed with `yaml` npm package; editable name/description fields, unknown
  keys preserved verbatim on save.
- Skills sync repo: separate managed repo (default `~/.claude-skills-repo`, configurable),
  branch per hostname, layout `user/<skill>/…` and `projects/<project-dirname>/<skill>/…`
  plus `manifest.json` (source paths, hostname, timestamp). Plain push/pull only —
  NEVER force-push (global rule; there's a hook that blocks it anyway).
- Install catalog: curated repo list (anthropics/skills etc.), listings fetched live from
  GitHub contents API (frontend fetch; GitHub sends CORS headers). Install = shallow
  `git clone` to temp + copy the skill subdir into `~/.claude/skills/<name>`.
- Settings stored at Tauri app-config-dir/settings.json (extra roots, repo path, remote).
- Dev ports are project-specific and fixed: 27391 (vite dev), 27392 (HMR) — set in
  vite.config.ts and tauri.conf.json devUrl. Chosen to avoid the shared Tauri
  default 1420. Don't move them back.

## Plan / steps

1. [x] Scaffold Tauri v2 + React + TS (via temp dir; `.git` blocked in-place scaffold).
2. [x] Fix scaffold naming (package/product/lib were named after the temp dir).
3. [x] Rust backend: discovery (user/projects/plugins), file read/write/create/delete,
       sync-repo commands (init/status/snapshot/branches/push/pull), install command,
       settings load/save.
4. [x] Frontend: sidebar tree (logical groups → skills → inner file tree only when
       multi-file), editor pane (frontmatter panel + CodeMirror), install dialog,
       sync panel, settings dialog.
5. [x] AI jobs (user request mid-build): parallel background `claude -p` runs against
       a selection, a skill, or a set of skills; files update in place; editor
       auto-reloads or shows a conflict banner when dirty; failed jobs keep output.
6. [x] Validate: `cargo check` + `bun run build` (tsc). Mind the compute-budget skill
       before heavy builds.
7. [x] README, commit at logical steps.  ← all committed, see progress log
8. [x] Smoke test: `bun run tauri dev` compiled (3m00s dev build), app process
       launched and stayed alive, vite served 200. Killed after verification.
       NOT yet exercised end-to-end by a human: install flow, sync flow.
9. [x] Follow-ups from use: mattpocock catalog source, dedicated dev ports
       27391/27392, disabled-state display + editor-header toggle (NOT in the
       sidebar rows — user wants the list compact), newline-flicker fix,
       discovery dedupe, model selector, structured AI responses.
10. [x] Publish + self-update — tracked in `plans/release-pipeline.md`
       (v0.1.0 tag still to be pushed by the user).
11. [x] Hooks support (Hooks tab) — tracked in `plans/hooks.md`.
12. [x] Instructions & memory (Memory tab: CLAUDE.md, rules, imports, auto
       memory, other agents' files) — tracked in `plans/instructions.md`.

## Findings / gotchas

- `~/.claude` is a PROTECTED directory for the claude CLI — spawned `claude -p`
  jobs cannot edit files there even with `--permission-mode acceptEdits`. AI jobs
  therefore run read-only (`--allowedTools Read,Glob,Grep`) and answer with
  structured JSON `{"files":[{path,content}],"notes"}` that ai.rs parses
  (leniently: raw / ```json fence / outermost braces) and applies itself via
  the app's own guarded writes. Contract smoke-tested against real
  `claude -p --model haiku` 2026-08-23: returns fenced JSON, parses clean.
- `~/.claude.json` may register the home dir itself as a project → its
  .claude/skills == user skills root. Discovery dedupes canonicalized roots.
- Frontmatter split/join MUST be byte-exact inverses or the editor oscillates
  (add/remove newline at top per keystroke). Also preserve CRLF vs LF.

- `bun create tauri-app . …` refuses non-empty dirs (the `.git` dir counts) — scaffold
  to a temp dir and move files in.
- Scaffold bakes the directory name into package.json, Cargo.toml ([package] name,
  [lib] name), tauri.conf.json (productName, window title), and lib import in main.rs —
  all needed fixing.
- `~/.claude.json` is large; only the `projects` keys are needed (parse with serde_json,
  iterate `projects` object keys).
- GitHub contents API unauthenticated = 60 req/hr — fetch listings lazily, cache in
  component state, fetch SKILL.md description only on demand.
- Tauri v2 IPC: commands return `Result<T, String>`; frontend `invoke()` rejects with
  the String. Keep error surfaces human-readable.
- `plugin:opener` ACL identifier in capabilities is `opener:default`.
- CodeMirror needs explicit `basicSetup` height CSS (`.cm-editor { height: 100% }`).
- vite build warns >500kB chunk (CodeMirror langs) — fine for a desktop app.
- Write guard (`files.rs` `check_allowed`): canonicalize + prefix check against the
  known skills roots keeps writes inside them.
- Windows `git` shell-out: pass `-C <repo>` rather than setting cwd; avoids UNC/verbatim
  path issues with canonicalized paths (`\\?\C:\...`). std::process on Windows does not
  use a shell, so no quoting issues, but AVOID canonicalized paths as `-C` args — strip
  the `\\?\` prefix (`paths::strip_verbatim`).
- Skills are disabled via `skillOverrides` (`"<name>": "off"`) in the scope's
  `settings.json`, with `settings.local.json` winning; plugins via
  `enabledPlugins: false`. Toggling rewrites settings.json, so serde_json uses
  `preserve_order` to keep the user's key order.
- Frontmatter extraction on CRLF files left a trailing `\r` (matched `\n---\r\n`
  first) — caught by a unit test, fixed.
- AI job output parsing must use stdout only; stderr used to be appended first,
  which could break the JSON parse.

## Progress log

- [x] Env verified (bun/rust/git), skill layouts inspected, project source confirmed
- [x] git init on master; scaffold moved into place
- [x] Plan doc created
- [x] Rust backend modules: paths.rs (verbatim-prefix strip), git.rs (safe git
      runner; force-push structurally impossible — args are fixed per command),
      discovery.rs, files.rs (guarded read/write/create/delete), settings.rs,
      sync.rs (init/status/snapshot/fetch/pull/push), install.rs
      (shallow clone + copy), overrides.rs (enable/disable), lib.rs wiring.
- [x] Frontend: types.ts, api.ts (typed invoke wrappers), App.tsx (3-pane layout),
      Sidebar (groups → skills → conditional file tree), EditorPane (frontmatter
      key/value panel + CodeMirror body, dirty tracking, Ctrl+S), InstallDialog
      (GitHub live listing + lazy SKILL.md preview), SyncPanel (init/snapshot/
      branches/push/pull), SettingsDialog (repo path, remote, extra roots),
      NewSkillDialog. `bun run build` (tsc + vite) clean.
- [x] AI job runner: ai.rs (spawn `claude -p <prompt> --allowedTools Read,Glob,Grep
      [--model m]` in the skill dir, threads drain stdout/stderr, parse + apply
      the structured JSON response, status map in tauri State),
      AiTaskDialog (selection mode + multi-skill mode with templated prompts),
      JobsPanel (poll every 2s, cancel, expand output), App bumps a reloadToken on
      job completion so EditorPane re-checks disk freshness
- [x] README rewritten for the actual app
- [x] Commits: scaffold → backend → frontend → README (4 logical commits on master)
- [x] Smoke test via `bun run tauri dev`: launch verified, then cleaned up
      (process killed, dev port freed, CPU-broker slots released)
- [x] WIP tracker entry added (personal WIP tracker, `skills-editor.md`)
- [x] Rust unit tests: AI response parsing + path safety, frontmatter extraction.

## Open questions for the user

1. Which other agents' skill formats to support next? (Codex `~/.codex`, Cursor rules,
   OpenCode, Windsurf…) — recommend Codex first; format is nearly identical.
2. Catalog curation: happy with anthropics/skills, mattpocock/skills, and
   obra/superpowers as seed repos?
   Easy to extend — it's a const list in `src/catalog.ts`.
3. Cherry-pick UX between host branches: v1 ships branch list + fetch/pull/push. A
   "diff two hosts, pick skills to copy" view is the natural next step — worth it?

## Things not to do

- Don't decode `~/.claude/projects` dir names for discovery — lossy; use `~/.claude.json`.
- Don't force-push from sync code, ever (hook blocks it; also structurally excluded).
- No `title=` attributes anywhere in the UI (global rule; touch devices).
- Don't run `bun run tauri build` casually — full Rust release build, minutes of CPU;
  `bun run build` + `cargo test` is the validation loop. Release builds come
  from CI only.
- Don't use `npm`/`package-lock.json` — Bun only.
