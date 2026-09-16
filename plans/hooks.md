# Hooks support

## Goal

Extend Skills Editor from "skills only" to also manage AI **hooks**: find every
hook configured on the machine, show them grouped logically (like skills),
edit them with a structured, schema-aware editor, edit the scripts they call,
test-run them with realistic input, enable/disable them, and include them in
the git tracking repo. Claude Code first; the model leaves room for other
agents' hook formats.

## Environment / context

- Continues `plans/skills-editor.md` (app) and `plans/release-pipeline.md`
  (publishing — v0.1.0 tag still not pushed as of 2026-09-16).
- Real hook configs on this machine:
  - `~/.claude/settings.json`: `PreToolUse` (matcher `Bash`, `if: "Bash(git *)"`,
    command `bash ~/.claude/hooks/block-force-push.sh`, `statusMessage`) and
    `Stop` (`type: "agent"` with a long `prompt`).
  - `~/git/Personal Projects/ops/.claude/settings.json`: `PreToolUse` with
    `shell: "bash"` and a command resolving the script via
    `$(git rev-parse --show-toplevel)/.claude/hooks/…`.
  - `~/.claude/hooks/block-force-push.sh` — script dir for user hooks.
  - Plugin `hooks/hooks.json` files: 1 in `plugins/cache` (gitkraken-hooks,
    disabled via `enabledPlugins`), 6 in `plugins/marketplaces` (catalog
    copies — NOT active installs).

## Reference: Claude Code hooks (code.claude.com/docs/en/hooks, fetched 2026-09-16)

- **Locations**: `~/.claude/settings.json`, `.claude/settings.json`,
  `.claude/settings.local.json`, managed policy
  (`C:\Program Files\ClaudeCode\managed-settings.json` + `managed-settings.d/*.json`;
  also HKLM/HKCU `SOFTWARE\Policies\ClaudeCode` `Settings` registry values),
  `<plugin>/hooks/hooks.json` (optional top-level `description`), skill
  frontmatter `hooks:`, subagent frontmatter `hooks:`. Entries MERGE across
  levels (nothing replaces).
- **Shape**: `hooks: { <Event>: [ { matcher?, hooks: [ <handler> ] } ] }`.
- **Events** (33): SessionStart, SessionEnd, Setup, UserPromptSubmit,
  UserPromptExpansion, Stop, StopFailure, PreToolUse, PermissionRequest,
  PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch,
  SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Elicitation,
  ElicitationResult, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate,
  WorktreeRemove, Notification, ConfigChange, InstructionsLoaded, PreCompact,
  PostCompact, PreModelSwitch, PostModelSwitch, MessageDisplay, TeammateIdle.
- **Matchers**: tool name for Pre/PostToolUse(+Failure), PermissionRequest,
  PermissionDenied; enumerated reasons for SessionStart/End, Setup,
  Notification, Pre/PostCompact, ConfigChange, DirectoryAdded, StopFailure,
  InstructionsLoaded; agent type for Subagent*; model for *ModelSwitch;
  filenames for FileChanged; command name for UserPromptExpansion; MCP server
  for Elicitation*. No matcher: UserPromptSubmit, PostToolBatch, Stop,
  TeammateIdle, TaskCreated, TaskCompleted, WorktreeCreate/Remove,
  MessageDisplay. `""`/`*`/omitted = all; only `[A-Za-z0-9_\- ,|]` = exact
  list; anything else = unanchored JS regex.
- **Handler types**: `command` (command, args[], async, asyncRewake, shell
  bash|powershell), `http` (url, headers, allowedEnvVars), `mcp_tool`
  (server, tool, input), `prompt` / `agent` (prompt with `$ARGUMENTS`,
  model). Common: `type`, `if` (permission-rule syntax, tool events only),
  `timeout` (s; defaults 600 / 30 prompt / 60 agent), `statusMessage`,
  `once` (skill frontmatter only).
- **Kill switches**: `disableAllHooks` (any settings file),
  `allowManagedHooksOnly` (managed only).
- **Hook env**: `CLAUDE_PROJECT_DIR`, `CLAUDE_PLUGIN_ROOT`,
  `CLAUDE_PLUGIN_DATA`, `CLAUDE_EFFORT`; stdin JSON common fields
  `session_id, transcript_path, cwd, permission_mode, hook_event_name, …`;
  exit 0 = ok (JSON stdout parsed if `{…}`), exit 2 = block (stderr = reason),
  other = non-blocking error.
- **Validation behavior** (settings docs): a value the schema rejects → a
  blocking "Settings Error" dialog at session start; an unknown hook event
  name → "Settings Warning", entry skipped.

## Decisions (made here — revisit only if the user objects)

1. **Sidebar gets a Skills / Hooks switch.** Hooks view groups by scope, same
   "logical group, location shown" style as skills: User, each project
   (shared + local merged into one group, per-entry file badge), Managed
   (read-only), each active plugin (read-only), skills/subagents that declare
   frontmatter hooks (read-only here; edited via their own file).
   Rows are compact: `Event · matcher` + handler summary.
2. **Per-handler granularity.** A "hook" in the UI = one handler at
   (source file, event, group index, handler index). The editor edits one
   handler plus its group's event/matcher.
3. **Structured editor, schema-aware**: event dropdown (known events only),
   matcher field with per-event hint + known values, type-specific fields,
   validation messages inline (no `title=`). Raw JSON view of the whole
   source's `hooks` block as the escape hatch.
4. **Writes are surgical and conflict-safe.** Backend replaces only the
   `hooks` key (serde_json `preserve_order`), removes it when empty, and
   refuses the write if the file changed since it was read (content hash
   round-tripped) — Claude Code itself rewrites settings.json.
5. **Per-hook disable lives in the app's config dir** (`disabled-hooks.json`),
   because unknown keys in settings.json risk a Settings Error. Disable =
   remove from the settings file and park it (with source path, event,
   matcher, handler) in the sidecar; enable = re-insert. Plus the native
   per-file `disableAllHooks` toggle.
6. **Scripts are first-class.** Commands are scanned for script paths
   (`~/…`, `${CLAUDE_PROJECT_DIR}/…`, `$(git rev-parse --show-toplevel)/…`,
   `${CLAUDE_PLUGIN_ROOT}/…`, absolute paths, bare `.claude/hooks/…`);
   resolved scripts open in the same CodeMirror editor (shell/PowerShell
   highlighting added). Unreferenced files in `.claude/hooks/` are listed as
   orphans. File guard allows exactly these files/dirs.
7. **Test run** for `command` hooks: user-triggered button, event-specific
   sample stdin JSON (editable), runs with the hook's shell, cwd and
   `CLAUDE_PROJECT_DIR`, honors timeout, shows exit code + stdout + stderr
   + the interpretation (allow / block / non-blocking error / parsed JSON).
   Other handler types aren't runnable locally.
8. **Sync repo** snapshots hooks too: `hooks/<scope>.json` (the `hooks`
   block + `disableAllHooks`) and script dirs under `hooks/scripts/<scope>/`.
9. **Only active plugins.** Plugin hooks come from `plugins/cache` installs;
   `plugins/marketplaces` copies are catalog data, not active hooks.
10. **Registry-delivered managed policy** isn't read (Windows HKLM/HKCU) —
   file-based managed settings only. Noted in UI detail text.

## Plan / steps

1. [x] Research schema/locations; survey this machine. ← done
2. [ ] Backend `hooks.rs`: discovery (all sources above), script-path
   resolution, orphan scripts, read with content hash.
3. [ ] Backend writes: `set_hooks` (hash-checked, surgical), `disableAllHooks`
   toggle, sidecar disable/enable.
4. [ ] Backend test runner.
5. [ ] File guard + sync snapshot extended for hooks/scripts.
6. [ ] Unit tests: script-path extraction, matcher classification, surgical
   write + conflict detection, sidecar round trip, test-run exit semantics.
7. [ ] Frontend: types/api, Skills/Hooks switch, hooks sidebar, HookEditor
   (structured + raw), script editing, test-run panel, disable toggles.
8. [ ] Checks (`bun run build`, `cargo test`), README, commit, push (CI).

## Findings / gotchas

- `WebFetch` of docs.claude.com 301s to code.claude.com; big doc pages come
  back as persisted files — grep them rather than trusting the summary.

## Open questions for the user

1. Other agents' hooks (Cursor `.cursor/hooks.json`, Gemini CLI settings,
   Codex…) — which first? Recommend waiting until Claude support settles.
2. A hooks install catalog (like skills)? No obvious canonical source yet —
   recommend skipping for now.
3. AI edit for hook scripts (the structured-response flow) — worth
   generalizing the AI dialog beyond skills? Recommend yes, as a follow-up.

## Things not to do

- Don't store app-only state (disabled hooks) inside Claude settings files.
- Don't rewrite whole settings files or reorder keys; only the `hooks` /
  `disableAllHooks` keys.
- Don't treat `plugins/marketplaces/**/hooks.json` as active.
- Don't run hook commands without an explicit user click.
- No `title=` attributes.
