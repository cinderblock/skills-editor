# Skills Editor

A small desktop IDE for managing AI agent skills, hooks, and instructions
(CLAUDE.md, rules, auto memory) — discovering, editing, installing,
enabling/disabling, testing, AI-assisted rewriting, and sharing them between
machines. Built with Tauri v2, React, TypeScript, and Bun.
Claude Code's formats are supported first; the discovery layer is designed to
grow to other agents.

## Install

Download the latest `Skills Editor_<version>_x64-setup.exe` from
[Releases](https://github.com/cinderblock/skills-editor/releases/latest) and run
it. Windows is the only supported platform for now.

The app keeps itself up to date: it checks GitHub Releases shortly after launch
and every few hours, and shows a banner when a new version is out. **Install &
restart** downloads the signed update and relaunches — it warns first if you
have unsaved changes or AI jobs still running. Settings shows the installed
version and has a manual **Check for updates** button.

Requires `git` on `PATH` (sync and catalog installs) and the
[`claude` CLI](https://docs.claude.com/en/docs/claude-code) for AI edits.

## What it does

- **Discovers skills across the machine** and groups them logically in the
  sidebar, abstracting file locations (while still showing them):
  - User skills: `~/.claude/skills/<name>/SKILL.md`
  - Project skills: `<project>/.claude/skills/…` — projects are found via the
    real paths recorded in `~/.claude.json`
  - Plugin-provided skills under `~/.claude/plugins` (shown read-only)
  - Any extra roots you add in Settings

  A directory reachable more than one way (e.g. your home dir registered as a
  project) is only listed once.
- **Edits skills** with a frontmatter-aware editor: `name`/`description` as
  form fields (other frontmatter keys are preserved, editable in raw view) and
  a CodeMirror body with markdown highlighting. Other file types get syntax
  highlighting by extension (yaml, json, js/ts, python, html, css). Line
  endings are preserved. Skills that are just a single `SKILL.md` show as one
  node; multi-file skills expand into a file tree.
- **Shows and toggles enabled state.** Disabled skills are dimmed with a
  badge. **Disable/Enable skill** in the editor header edits Claude Code's
  `skillOverrides` in the governing `settings.json` (user or project) —
  disabling writes `"<name>": "off"`, enabling removes the entry from both
  `settings.json` and `settings.local.json`. Plugin skills follow their
  plugin's `enabledPlugins` state and can't be toggled individually. Changes
  apply to new Claude Code sessions.
- **Installs popular skills** from a catalog (Anthropic's `anthropics/skills`,
  `mattpocock/skills`, `obra/superpowers`) — listings come live from the GitHub
  API, previews show the real SKILL.md, and install is a shallow clone + copy
  into `~/.claude/skills`.
- **AI edits via `claude -p`**: select text and run a one-off "fix this
  selection" prompt, or run a prompt against one or many skills at once, with a
  choice of model. Jobs run in parallel in the background while you keep
  working. Because the `claude` CLI treats `~/.claude` as protected, jobs run
  with read-only tools and reply with structured JSON (full new content per
  changed file, plus notes); the app validates and applies it. A response that
  can't be parsed or names an unsafe path changes nothing and fails the job
  with its full output kept. The editor reloads changed files automatically,
  or shows a conflict banner if you have unsaved edits.
- **Manages Claude Code hooks** (the **Hooks** tab):
  - Finds hooks in user and project settings (`settings.json` and
    `settings.local.json`), file-based managed policy, installed plugins, and
    skill/subagent frontmatter. Each is grouped like skills, with its file
    shown and a note when it won't run (disabled plugin, `disableAllHooks`).
    Managed, plugin, and frontmatter hooks are read-only.
  - A structured editor covers every event and handler type (`command`,
    `http`, `mcp_tool`, `prompt`, `agent`). It explains what the matcher
    matches, validates before saving, and has a raw-JSON view of a file's
    whole `hooks` block.
  - Saving rewrites only the `hooks` key. The rest of the file stays
    byte-for-byte the same, including key order. If Claude Code changed the
    file since you opened it, the save is refused rather than silently
    overwriting it.
  - **Disable/Enable** per hook. Claude Code has no such switch, so a
    disabled hook is moved out of the settings file into the app's own
    storage, and put back into its matcher group when re-enabled. There's
    also the native per-file `disableAllHooks` toggle.
  - **Scripts** that hooks run are detected from the command (`~`, `$HOME`,
    `$CLAUDE_PROJECT_DIR`, `$(git rev-parse --show-toplevel)`,
    `${CLAUDE_PLUGIN_ROOT}`, exec-form args). They open in the editor with
    shell/PowerShell highlighting. Unreferenced files in `.claude/hooks` are
    listed too.
  - **Test run** executes a command hook with editable, event-shaped sample
    input, using Git Bash or PowerShell like Claude Code does. It shows the
    exit code, stdout/stderr, and how Claude Code would read the result
    (allow, block, JSON decision, non-blocking error, timeout).
- **Manages instructions and memory** (the **Memory** tab):
  - Finds every file Claude Code reads as instructions:
    - managed policy (`CLAUDE.md` and `claudeMd`), `~/.claude/CLAUDE.md` and
      `~/.claude/rules/`;
    - each project's `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md` and
      `.claude/rules/`;
    - `CLAUDE.md` files in parent folders, and in subfolders (these load on
      demand);
    - auto memory: the `MEMORY.md` index plus its topic files.
  - Also lists other agents' files: `AGENTS.md`, `GEMINI.md`, Copilot,
    Cursor, Windsurf and Cline. They're marked as loaded when a `CLAUDE.md`
    `@import`s them.
  - Each file shows when it loads: at startup, on demand (subfolders and
    rules with `paths:`), through an import, or never. Files excluded by
    `claudeMdExcludes`, broken or out-of-project `@imports`, a `MEMORY.md`
    past its 200-line / 25 KB load limit, and over-long files are all
    flagged.
  - **Startup context** lists, per project, what a new session reads before
    your first message, in load order, with sizes and a rough token count.
  - Create the standard files (optionally adding `CLAUDE.local.md` to
    `.gitignore`), edit them in the same editor as skills (AI selection
    edits included), delete them, and turn auto memory on or off globally
    or per project.
  - Scanning subfolders respects `.gitignore`, is bounded, and skips your
    home folder. Results are cached for two minutes; **Refresh** rescans.
- **Tracks skill, hook, and instruction evolution in a git repo** (default
  `~/.claude-skills-repo`):
  - One branch per host, with snapshot commits of all user/project/extra
    skills.
  - Each settings file's hook config and the scripts it runs go under
    `hooks/`, and hooks disabled from the app are included too.
  - Instruction files go under `instructions/` and auto memory under
    `memory/`.
  - A `manifest.json` maps repo paths back to their sources.
  - Push/pull against a shared remote to move skills between machines, and
    cherry-pick between host branches. Pulls are fast-forward only and
    pushes are plain — never forced.

## Development

Dev ports are project-specific to avoid colliding with other Tauri projects
(which all default to 1420): **27391** (vite dev server) and **27392** (HMR).

```sh
bun install
bun run tauri dev     # run the app with hot reload
bun run test          # frontend unit tests (bun test)
bun run build         # typecheck + bundle frontend
cargo test --manifest-path src-tauri/Cargo.toml   # backend tests (needs dist/ from the build)
```

Requires Bun and Rust. Automatic update checks are skipped in dev builds; the
manual check in Settings still works.

## CI and releases

- **CI** (`.github/workflows/ci.yml`) runs on every push to `master` and on
  PRs: frontend tests, typecheck, and build on Linux; Rust tests on Windows.
- **Releases** (`.github/workflows/release.yml`) are built only by CI, from a
  version tag. To cut one:
  1. Bump the version in `package.json`, `src-tauri/Cargo.toml`, and
     `src-tauri/tauri.conf.json` (the workflow refuses a tag that doesn't
     match all three), and commit.
  2. `git tag vX.Y.Z && git push origin vX.Y.Z`

  The workflow runs the checks, builds the NSIS and MSI installers, signs the
  updater artifacts, and publishes a GitHub Release with the `latest.json`
  manifest that installed apps poll.
- **Updater signing**: the private key and its password live in the
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repo
  secrets; the matching public key is in `tauri.conf.json`. Losing the private
  key means existing installs can't verify future updates, so keep the local
  copy backed up.

## Layout

- `src/` — React frontend:
  - `components/`, including `HooksSidebar` and `HookEditor`
  - `api.ts` — IPC wrappers
  - `catalog.ts` — install sources
  - `frontmatter.ts` — YAML round-tripping
  - `hooksModel.ts` — hook event metadata, edit/move logic, test-result
    interpretation; unit-tested in `hooksModel.test.ts`
  - `instructionsModel.ts` — badges, load explanations, filtering, and
    create options for the Memory tab; unit-tested in
    `instructionsModel.test.ts`
  - `updates.ts` — self-update
- `src-tauri/src/` — Rust backend:
  - `discovery.rs` — skill scanning and enabled state
  - `overrides.rs` — enable/disable
  - `hooks.rs` — hook discovery, script resolution, conflict-checked edits,
    disabled-hook store, test runner
  - `instructions.rs` — CLAUDE.md/rules/auto-memory discovery, imports,
    excludes, startup context, create/delete, auto-memory toggle
  - `files.rs` — guarded read/write/create/delete
  - `sync.rs` — tracking repo
  - `install.rs` — catalog installs
  - `ai.rs` — parallel `claude -p` jobs and response application
  - `git.rs` — safe git shell-out
  - `settings.rs`
- `plans/` — living plan documents

## License

[MIT](LICENSE)
