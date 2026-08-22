# Skills Editor

A small desktop IDE for managing AI agent skills — discovering, editing,
installing, AI-assisted rewriting, and sharing them between machines.
Built with Tauri v2, React, TypeScript, and Bun. Claude Code's skill format
is supported first; the discovery layer is designed to grow to other agents.

## What it does

- **Discovers skills across the machine** and groups them logically in the
  sidebar, abstracting file locations (while still showing them):
  - User skills: `~/.claude/skills/<name>/SKILL.md`
  - Project skills: `<project>/.claude/skills/…` — projects are found via the
    real paths recorded in `~/.claude.json`
  - Plugin-provided skills under `~/.claude/plugins` (shown read-only)
  - Any extra roots you add in Settings
- **Edits skills** with a frontmatter-aware editor: `name`/`description` as
  form fields (other frontmatter keys are preserved, editable in raw view) and
  a CodeMirror body with markdown highlighting. Other file types get syntax
  highlighting by extension (yaml, json, js/ts, python, html, css). Skills that
  are just a single `SKILL.md` show as one node; multi-file skills expand into
  a file tree.
- **Installs popular skills** from a catalog (Anthropic's `anthropics/skills`,
  `mattpocock/skills`, `obra/superpowers`) — listings come live from the GitHub
  API, previews show
  the real SKILL.md, and install is a shallow clone + copy into
  `~/.claude/skills`.
- **AI edits via `claude -p`**: select text in a file and run a one-off
  "fix this selection" prompt, or run a prompt against one or many skills at
  once. Jobs run in parallel in the background while you keep working; files
  update in place when a job lands, the editor reloads automatically (or shows
  a conflict banner if you have unsaved changes), and failed jobs keep their
  full output for inspection.
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
bun run tauri dev     # run the app
bun run build         # typecheck + bundle frontend
cargo check --manifest-path src-tauri/Cargo.toml   # check backend
bun run tauri build   # release build/installer
```

Requires Bun, Rust, git on PATH, and the `claude` CLI for AI edits.

## Layout

- `src/` — React frontend (components, `api.ts` IPC wrappers, `catalog.ts`
  install sources, `frontmatter.ts` YAML round-tripping)
- `src-tauri/src/` — Rust backend: `discovery.rs` (skill scanning),
  `files.rs` (guarded read/write/create/delete), `sync.rs` (tracking repo),
  `install.rs` (catalog installs), `ai.rs` (parallel `claude -p` jobs),
  `git.rs` (safe git shell-out), `settings.rs`
- `plans/` — living plan documents
