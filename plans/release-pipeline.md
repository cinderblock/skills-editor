# Release Pipeline — scrub, publish, CI, self-update

## Goal

Make Skills Editor installable from GitHub Releases and self-updating: scrub
personal data from history, publish a public repo, add CI checks and a
tag-triggered release workflow that builds a signed Windows installer, and wire
`tauri-plugin-updater` to GitHub Releases' `latest.json`.

## Environment / context

- Repo: local-only at the project root, branch `master` (keep it — the user's
  deliberate default), 12 commits, all authored
  `Cameron Tacklind <cameron@tacklind.com>` (public identity, kept).
- GitHub account `cinderblock` (gh authenticated, ssh). Target repo
  `cinderblock/skills-editor` — name confirmed free.
- App: Tauri v2 + React/TS, Bun (`bun.lock`), `productName` "Skills Editor",
  identifier `com.vibedout.skills-editor`, version 0.1.0.
- Reference implementation: `~/git/claude-usage` (`plans/release-pipeline.md`,
  `.github/workflows/{ci,release}.yml`) — same pattern, npm instead of Bun.
- `git-filter-repo` installed at `~/.local/bin`.

## Decisions already made (don't re-ask)

- **Public repo** (user, 2026-09-16) — also makes updater downloads tokenless.
- **Auto-update in the first release** (user, 2026-09-16) — so no installed copy
  ever predates the updater.
- **Scrub scope:** same rule as claude-usage — home-dir paths / Windows
  username go; author name/email stay. Only hit in history:
  `C:\Users\<user>\git\vibed-out\skills-editor` in `plans/skills-editor.md`.
- **Rewrite mechanics:** temp clone + `git filter-repo --replace-text`, then
  swap refs in the main repo with `git reset --mixed` (never `--hard`). Backup
  branch + safety stash first.
- **Windows-only release artifacts.** The frontend builds skill file paths with
  `\` separators, so macOS/Linux builds would compile but misbehave. Revisit
  when paths are made platform-neutral.
- **Releases only from CI** (global rule). Final step handed back to the user
  is pushing the tag — I don't push release tags.
- **Signing key** at `~/.tauri/skills-editor.key` (+ `.password` beside it),
  never in the repo; both stored as Actions secrets.

## Plan / steps

1. [x] History scan for personal strings.
2. [ ] Backup branch + safety stash; rewrite in temp clone; verify; swap refs.
   ← current
3. [ ] Updater: Rust + JS plugins, capabilities, `createUpdaterArtifacts`,
   pubkey + endpoint, in-app update check UI.
4. [ ] Signing keypair generated; password file verified.
5. [ ] CI + release workflows (Bun).
6. [ ] Local checks green (`bun run build`, `cargo check`/`test`).
7. [ ] README install/update/release docs.
8. [ ] Create public repo, push `master`, set secrets, confirm CI green.
9. [ ] Hand back: user pushes `v0.1.0` tag; then verify release assets and
   `releases/latest/download/latest.json`.

## Findings / gotchas

- Inherited from claude-usage (don't relearn):
  - filter-repo hard-resets a non-bare worktree → never run it in the main
    repo. Never fetch the original repo into the rewrite clone.
  - PowerShell `Set-Content -NoNewline <path> <value>` can bind the value as
    the path — write the password file via pipeline and verify it.
  - `tauri::generate_context!` embeds the frontend dist at compile time, so
    CI must build the frontend before any cargo command.
  - CI may lack types that local hoisting provides.

## Things not to do

- Don't push until the history greps are clean.
- Don't commit the private key or password.
- Don't push the release tag or publish from the CLI.
- Don't rename `master`.

## Progress log

- [x] Repo confirmed unpublished; scan done (one path hit).
