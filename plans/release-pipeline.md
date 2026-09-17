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
- **All three platforms** (user, 2026-09-17). The old Windows-only decision
  was based on the frontend joining paths with `\`; that's now a shared
  `src/paths.ts` helper (separator taken from the path), and CI compiles and
  tests on Windows, macOS and Linux so it can't rot again.
- **macOS ships unsigned** (user, 2026-09-17): no Apple Developer ID, so
  Gatekeeper warns on first open. Revisit if that becomes annoying —
  notarization needs a paid account plus secrets in the workflow.
- **Releases only from CI** (global rule). The tag is pushed only when the
  user explicitly asks for a release; v0.1.0 and v0.2.0 were both released
  that way.
- **Signing key** at `~/.tauri/skills-editor.key` (+ `.password` beside it),
  never in the repo; both stored as Actions secrets.

## Plan / steps

1. [x] History scan for personal strings.
2. [x] Backup branch `backup/pre-scrub` (tree was clean, so no stash needed);
   rewrite in temp clone; verified 0 hits (control: 10 in original); swapped
   `master` via `git reset --mixed`; temp clone deleted.
3. [x] Updater: `tauri-plugin-updater` + `tauri-plugin-process` (Rust + JS),
   capabilities `updater:default` + `process:allow-restart`,
   `createUpdaterArtifacts`, pubkey + endpoint, `src/updates.ts` hook,
   `UpdateBanner`, version + manual check in Settings.
4. [x] Signing keypair at `~/.tauri/skills-editor.key` (+ `.password`,
   `.pub`); password file read back and verified; key+password verified by
   signing a test file.
5. [x] CI + release workflows (Bun). Release has a tag↔version guard
   (tested locally: v0.1.0 passes, v0.2.0 fails naming all three files).
6. [x] Local checks green: `bun run build`, `cargo test` 11/11.
7. [x] README install/update/release docs; MIT LICENSE added.
8. [x] Public repo https://github.com/cinderblock/skills-editor created
   (default branch `master`, MIT detected); secrets set; only `master`
   pushed; first CI run green (frontend + rust).
9. [x] v0.1.0 published 2026-09-16 on the user's go-ahead: annotated tag
   on `23c15c2` (includes the Hooks tab), release run 35169605447 green.
   Verified: release is public (not draft/prerelease) with NSIS + MSI
   installers and `.sig` files; `releases/latest/download/latest.json`
   serves version 0.1.0 for windows-x86_64 / -nsis / -msi; installer URL
   returns 200; all three signatures carry key ID C272163F9A7064F4, the
   same key as the pubkey in tauri.conf.json.
   https://github.com/cinderblock/skills-editor/releases/tag/v0.1.0
10. [x] v0.2.0 released 2026-09-17 (Hooks, Config, Memory tabs + fixes):
   version bumped in all three files, CI green (35256653181), tag pushed,
   release run 35257047290 green. Verified: public release with NSIS + MSI
   installers and `.sig` files, `latest.json` serves 0.2.0 for all three
   windows targets, installer URL 200, all signatures carry key ID
   C272163F9A7064F4 (matches tauri.conf.json). Release notes written by
   hand with `gh release edit` — the workflow only sets a one-line body.
   https://github.com/cinderblock/skills-editor/releases/tag/v0.2.0
11. [ ] ← current. Self-update still untested end to end: install v0.2.0
   from the release, then the NEXT release should show the banner and
   install + restart.

## Findings / gotchas

- **Never signal a process group to kill a child** (2026-09-17). The hook
  test runner's timeout path ran `kill -KILL -<pid>`; a negative PID means
  "the process group", and if the child isn't the group leader that reaches
  whatever owns the group. On the Linux CI runner it killed the runner agent:
  the job failed at exactly 48 minutes (GitHub's lost-communication timeout)
  with **no logs, no failing step, and step `timeout-minutes` never firing** —
  because nothing was alive to enforce or upload them. macOS never reproduced
  it (BSD `kill` rejects that argument form), so it looked like flaky Linux
  infrastructure. Fix: `pkill -KILL -P <pid>` then `child.kill()`.
  Diagnosis technique that worked: split the CI step (compile vs run, then
  subprocess tests vs the rest) — the *step list* still reports progress even
  when logs are lost.

- Inherited from claude-usage (don't relearn):
  - filter-repo hard-resets a non-bare worktree → never run it in the main
    repo. Never fetch the original repo into the rewrite clone.
  - PowerShell `Set-Content -NoNewline <path> <value>` can bind the value as
    the path — write the password file via pipeline and verify it.
  - `tauri::generate_context!` embeds the frontend dist at compile time, so
    CI must build the frontend before any cargo command.
  - CI may lack types that local hoisting provides.

- Git Bash `grep -icF` on `git log -p` output aborted with core dumps, and
  `grep -E` rejected a pattern with a trailing backslash ("Trailing
  backslash") while the `|| echo clean` fallback printed a false "clean".
  Verify scrubs with `git grep -F -l <pat> <rev>` over `git rev-list`, and
  include a control count against the unscrubbed history.
- Key files from `tauri signer generate` have no trailing newline, so
  `gh secret set NAME < file` (bash redirect) stores them exactly.

## Remaining / follow-ups

- Local-only leftover: branch `backup/pre-scrub` holds the unscrubbed history
  — never push it; delete once v0.1.0 is out and verified.
- App icons are still the Tauri defaults.
- Back up `~/.tauri/skills-editor.key` + password somewhere durable; losing
  them strands every installed copy on its current version.

## Things not to do

- Don't push until the history greps are clean.
- Don't commit the private key or password.
- Don't push the release tag or publish from the CLI.
- Don't rename `master`.

## Progress log

- [x] Repo confirmed unpublished; scan done (one path hit).
- [x] History rewritten + verified (16 commits on `master` after new work).
- [x] Updater wired, signed, secrets set.
- [x] Published; CI green on `master` (run 35142461510).
- [x] v0.1.0 released 2026-09-16; v0.2.0 released 2026-09-17 (assets and
  `latest.json` verified both times).
