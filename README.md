# Skills Editor

A small desktop IDE for managing AI agent skills — discovering, editing,
installing, enabling/disabling, AI-assisted rewriting, and sharing them between
machines. Built with Tauri v2, React, TypeScript, and Bun. Claude Code's skill
format is supported first; the discovery layer is designed to grow to other
agents.

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
- **Tracks skill evolution in a git repo** (default `~/.claude-skills-repo`):
  one branch per host, snapshot commits of all user/project/extra skills plus
  a `manifest.json` mapping repo paths back to their sources. Push/pull against
  a shared remote to move skills between machines and cherry-pick between host
  branches. Only fast-forward pulls and plain pushes — never force.

## Development

Dev ports are project-specific to avoid colliding with other Tauri projects
(which all default to 1420): **27391** (vite dev server) and **27392** (HMR).

```sh
bun install
bun run tauri dev     # run the app with hot reload
bun run build         # typecheck + bundle frontend
cargo test --manifest-path src-tauri/Cargo.toml   # backend tests (needs dist/ from the build)
```

Requires Bun and Rust. Automatic update checks are skipped in dev builds; the
manual check in Settings still works.

## CI and releases

- **CI** (`.github/workflows/ci.yml`) runs on every push to `master` and on
  PRs: frontend typecheck + build on Linux, Rust tests on Windows.
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

- `src/` — React frontend (components, `api.ts` IPC wrappers, `catalog.ts`
  install sources, `frontmatter.ts` YAML round-tripping, `updates.ts`
  self-update)
- `src-tauri/src/` — Rust backend: `discovery.rs` (skill scanning and enabled
  state), `overrides.rs` (enable/disable), `files.rs` (guarded
  read/write/create/delete), `sync.rs` (tracking repo), `install.rs` (catalog
  installs), `ai.rs` (parallel `claude -p` jobs and response application),
  `git.rs` (safe git shell-out), `settings.rs`
- `plans/` — living plan documents

## License

[MIT](LICENSE)
