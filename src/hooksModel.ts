import type { HookSource, HookTestResult, ParkedHook } from "./types";

/** A JSON object (hooks config is untyped JSON we must round-trip intact). */
export type Json = Record<string, unknown>;

// ---------------------------------------------------------------------------
// Event metadata (code.claude.com/docs/en/hooks — see plans/hooks.md)
// ---------------------------------------------------------------------------

export interface MatcherInfo {
  /** What the matcher is compared against. */
  label: string;
  /** Known values offered as suggestions. */
  values: string[];
  /** Tool names and model names accept lists and regexes. */
  regex: boolean;
}

export interface EventInfo {
  category: string;
  blurb: string;
  /** null = this event ignores matchers (fires on every occurrence). */
  matcher: MatcherInfo | null;
  /** Tool events: the `if` permission-rule field applies. */
  tool: boolean;
  /** Plain stdout on exit 0 is added to Claude's context. */
  stdoutIsContext: boolean;
}

const TOOLS = ["Bash", "Edit", "Write", "Read", "Glob", "Grep", "WebFetch", "WebSearch", "Task", "NotebookEdit", "Edit|Write", "mcp__.*"];
const tool = (blurb: string): EventInfo => ({
  category: "Tool calls",
  blurb,
  matcher: { label: "tool name", values: TOOLS, regex: true },
  tool: true,
  stdoutIsContext: false,
});
const plain = (category: string, blurb: string, extra: Partial<EventInfo> = {}): EventInfo => ({
  category,
  blurb,
  matcher: null,
  tool: false,
  stdoutIsContext: false,
  ...extra,
});
const matched = (category: string, blurb: string, label: string, values: string[], extra: Partial<EventInfo> = {}): EventInfo => ({
  category,
  blurb,
  matcher: { label, values, regex: false },
  tool: false,
  stdoutIsContext: false,
  ...extra,
});

export const EVENT_INFO: Record<string, EventInfo> = {
  SessionStart: matched("Session", "A session starts, resumes, clears, or compacts. Stdout becomes context.", "start reason", ["startup", "resume", "clear", "compact", "fork"], { stdoutIsContext: true }),
  SessionEnd: matched("Session", "A session ends. Shares a short time budget.", "end reason", ["clear", "resume", "logout", "prompt_input_exit", "other"]),
  Setup: matched("Session", "Runs for `claude --init` / maintenance.", "CLI flag", ["init", "maintenance"]),
  UserPromptSubmit: plain("Turn", "Before Claude sees a prompt. Can block it; stdout becomes context.", { stdoutIsContext: true }),
  UserPromptExpansion: matched("Turn", "A slash command / skill expands into a prompt.", "command name", [], { stdoutIsContext: true }),
  Stop: plain("Turn", "Claude finishes responding. Can make it keep going."),
  StopFailure: matched("Turn", "A turn ends in an API error.", "error type", ["rate_limit", "overloaded", "authentication_failed", "oauth_org_not_allowed", "account_on_hold", "billing_error", "invalid_request", "model_not_found", "server_error", "max_output_tokens", "cloud_credential_error", "unknown"]),
  PreToolUse: tool("Before a tool runs. Can allow, deny, or rewrite the call."),
  PermissionRequest: tool("A permission prompt is about to be shown."),
  PermissionDenied: tool("A tool call was denied; can request a retry."),
  PostToolUse: tool("After a tool succeeds. Can add context for Claude."),
  PostToolUseFailure: tool("After a tool call fails."),
  PostToolBatch: plain("Tool calls", "After a batch of parallel tool calls completes."),
  SubagentStart: matched("Agents", "A subagent starts.", "agent type", ["general-purpose", "Explore", "Plan"], { matcher: { label: "agent type", values: ["general-purpose", "Explore", "Plan"], regex: true } }),
  SubagentStop: matched("Agents", "A subagent finishes.", "agent type", ["general-purpose", "Explore", "Plan"], { matcher: { label: "agent type", values: ["general-purpose", "Explore", "Plan"], regex: true } }),
  TaskCreated: plain("Agents", "A task is created."),
  TaskCompleted: plain("Agents", "A task completes."),
  TeammateIdle: plain("Agents", "A teammate agent goes idle."),
  Elicitation: matched("MCP", "An MCP server asks the user for input.", "MCP server", []),
  ElicitationResult: matched("MCP", "The user answered an MCP elicitation.", "MCP server", []),
  Notification: matched("Environment", "Claude Code shows a notification.", "notification type", ["permission_prompt", "idle_prompt", "auth_success", "elicitation_dialog", "elicitation_url_dialog", "elicitation_complete", "elicitation_response", "agent_needs_input", "agent_completed", "quota_auto_resume_fired", "quota_auto_resume_stale", "quota_auto_resume_disabled"]),
  CwdChanged: plain("Environment", "The working directory changes."),
  DirectoryAdded: matched("Environment", "A directory is added to the session.", "add method", ["slash_command", "register_repo_root"]),
  FileChanged: matched("Environment", "A watched file changes. Matcher lists exact filenames.", "file names", [".env", ".envrc"]),
  WorktreeCreate: plain("Environment", "A git worktree is created."),
  WorktreeRemove: plain("Environment", "A git worktree is removed."),
  ConfigChange: matched("Environment", "A settings file or skill changes on disk.", "config source", ["user_settings", "project_settings", "local_settings", "policy_settings", "skills"]),
  InstructionsLoaded: matched("Environment", "CLAUDE.md-style instructions are loaded.", "load reason", ["session_start", "nested_traversal", "path_glob_match", "include", "compact"]),
  PreCompact: matched("Context", "Before the conversation is compacted.", "trigger", ["manual", "auto"]),
  PostCompact: matched("Context", "After compaction.", "trigger", ["manual", "auto"]),
  PreModelSwitch: matched("Context", "Before the model changes. A timeout blocks the switch.", "model", ["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5"], { matcher: { label: "model", values: ["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5", ".*opus.*"], regex: true } }),
  PostModelSwitch: matched("Context", "After the model changes. Stdout becomes context.", "model", [], { matcher: { label: "model", values: ["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5", ".*opus.*"], regex: true }, stdoutIsContext: true }),
  MessageDisplay: plain("Context", "A message is displayed. Short time budget."),
};

export const EVENT_ORDER = Object.keys(EVENT_INFO);

export const CATEGORIES = [...new Set(EVENT_ORDER.map((e) => EVENT_INFO[e].category))];

export function eventInfo(event: string): EventInfo {
  return EVENT_INFO[event] ?? plain("Unknown", "Not a recognised Claude Code event — it will be ignored.");
}

/** How Claude Code will interpret a matcher string. */
export function describeMatcher(matcher: string | undefined): string {
  if (matcher === undefined || matcher === "" || matcher === "*") return "matches everything";
  if (/^[A-Za-z0-9_\- ,|]+$/.test(matcher)) {
    const parts = matcher.split(/[|,]/).map((s) => s.trim()).filter(Boolean);
    return parts.length > 1 ? `exact match on any of: ${parts.join(", ")}` : `exact match: ${parts[0]}`;
  }
  try {
    new RegExp(matcher);
    return "regular expression (unanchored)";
  } catch (e) {
    return `invalid regular expression: ${(e as Error).message}`;
  }
}

export const HANDLER_TYPES = ["command", "http", "mcp_tool", "prompt", "agent"] as const;
export type HandlerType = (typeof HANDLER_TYPES)[number];

export const TYPE_LABEL: Record<HandlerType, string> = {
  command: "command — run a program",
  http: "http — POST to a URL",
  mcp_tool: "mcp_tool — call an MCP tool",
  prompt: "prompt — ask a model",
  agent: "agent — run a subagent",
};

export const DEFAULT_TIMEOUT: Record<HandlerType, number> = {
  command: 600,
  http: 600,
  mcp_tool: 600,
  prompt: 30,
  agent: 60,
};

/** Fields the structured form edits; anything else is carried through. */
export const KNOWN_FIELDS = new Set([
  "type", "if", "timeout", "statusMessage", "once",
  "command", "args", "async", "asyncRewake", "shell",
  "url", "headers", "allowedEnvVars",
  "server", "tool", "input",
  "prompt", "model",
]);

// ---------------------------------------------------------------------------
// Flattening and summaries
// ---------------------------------------------------------------------------

export interface HookLoc {
  event: string;
  group: number;
  handler: number;
}

export interface FlatHook extends HookLoc {
  matcher: string | undefined;
  handler_json: Json;
}

function asArray(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

function asObject(v: unknown): Json | null {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Json) : null;
}

export function flatten(hooks: unknown): FlatHook[] {
  const out: FlatHook[] = [];
  const events = asObject(hooks) ?? {};
  for (const [event, groups] of Object.entries(events)) {
    asArray(groups).forEach((g, gi) => {
      const group = asObject(g);
      if (!group) return;
      const matcher = typeof group.matcher === "string" ? group.matcher : undefined;
      asArray(group.hooks).forEach((h, hi) => {
        const handler = asObject(h);
        if (handler) out.push({ event, group: gi, handler: hi, matcher, handler_json: handler });
      });
    });
  }
  out.sort(
    (a, b) =>
      EVENT_ORDER.indexOf(a.event) - EVENT_ORDER.indexOf(b.event) ||
      a.group - b.group ||
      a.handler - b.handler,
  );
  return out;
}

export function handlerSummary(h: Json): string {
  const type = (h.type as string) ?? "command";
  switch (type) {
    case "command": {
      const args = Array.isArray(h.args) ? ` ${(h.args as unknown[]).join(" ")}` : "";
      return `${h.command ?? ""}${args}`;
    }
    case "http":
      return `POST ${h.url ?? ""}`;
    case "mcp_tool":
      return `${h.server ?? "?"} → ${h.tool ?? "?"}`;
    default: {
      const first = String(h.prompt ?? "").split("\n").find((l) => l.trim()) ?? "";
      return `${type}: ${first}`;
    }
  }
}

export function parkedTitle(p: ParkedHook): string {
  return p.matcher !== null && p.matcher !== undefined && p.matcher !== ""
    ? `${p.event} · ${String(p.matcher)}`
    : p.event;
}

export function findHandler(src: HookSource, loc: HookLoc): Json | null {
  const groups = asArray(asObject(src.hooks)?.[loc.event]);
  const group = asObject(groups[loc.group]);
  return asObject(asArray(group?.hooks)[loc.handler]);
}

export function groupSize(src: HookSource, loc: HookLoc): number {
  const groups = asArray(asObject(src.hooks)?.[loc.event]);
  return asArray(asObject(groups[loc.group])?.hooks).length;
}

export function groupMatcher(src: HookSource, loc: HookLoc): string | undefined {
  const groups = asArray(asObject(src.hooks)?.[loc.event]);
  const m = asObject(groups[loc.group])?.matcher;
  return typeof m === "string" ? m : undefined;
}

export function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

// ---------------------------------------------------------------------------
// Edits (pure; the backend re-validates and writes)
// ---------------------------------------------------------------------------

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v ?? {}));
}

/** Remove a handler, pruning emptied groups/events. Mutates `hooks`. */
function removeAt(hooks: Json, loc: HookLoc): void {
  const groups = asArray(hooks[loc.event]);
  const group = asObject(groups[loc.group]);
  const handlers = asArray(group?.hooks);
  handlers.splice(loc.handler, 1);
  if (handlers.length === 0) groups.splice(loc.group, 1);
  if (groups.length === 0) delete hooks[loc.event];
}

/**
 * Put `handler` at `to` (event + matcher). Editing in place keeps its slot;
 * a changed event or matcher moves only this handler — into the first group
 * with that matcher, or a new group. Returns the new hooks and location.
 */
export function upsertHandler(
  hooksIn: unknown,
  from: HookLoc | null,
  to: { event: string; matcher: string | undefined },
  handler: Json,
): { hooks: Json; loc: HookLoc } {
  const hooks = clone(asObject(hooksIn) ?? {});
  if (from) {
    const groups = asArray(hooks[from.event]);
    const group = asObject(groups[from.group]);
    const current = typeof group?.matcher === "string" ? group.matcher : undefined;
    if (group && from.event === to.event && current === to.matcher) {
      asArray(group.hooks)[from.handler] = handler;
      return { hooks, loc: from };
    }
    removeAt(hooks, from);
  }
  if (!Array.isArray(hooks[to.event])) hooks[to.event] = [];
  const groups = hooks[to.event] as unknown[];
  let gi = groups.findIndex((g) => {
    const o = asObject(g);
    const m = typeof o?.matcher === "string" ? o.matcher : undefined;
    return !!o && Array.isArray(o.hooks) && m === to.matcher;
  });
  if (gi < 0) {
    const group: Json = {};
    if (to.matcher !== undefined) group.matcher = to.matcher;
    group.hooks = [];
    groups.push(group);
    gi = groups.length - 1;
  }
  const handlers = (groups[gi] as Json).hooks as unknown[];
  handlers.push(handler);
  return { hooks, loc: { event: to.event, group: gi, handler: handlers.length - 1 } };
}

export function removeHandler(hooksIn: unknown, loc: HookLoc): Json {
  const hooks = clone(asObject(hooksIn) ?? {});
  removeAt(hooks, loc);
  return hooks;
}

/** Client-side checks, so problems show inline before a round trip. */
export function validateHandler(event: string, matcher: string | undefined, h: Json): string[] {
  const problems: string[] = [];
  if (!EVENT_INFO[event]) problems.push(`"${event}" is not a Claude Code hook event.`);
  const info = eventInfo(event);
  if (matcher && info.matcher === null) problems.push(`${event} ignores matchers — leave it empty.`);
  if (matcher && describeMatcher(matcher).startsWith("invalid")) problems.push(`Matcher: ${describeMatcher(matcher)}.`);
  const type = h.type as string;
  const need = (field: string, label = field) => {
    if (typeof h[field] !== "string" || !(h[field] as string).trim()) problems.push(`${label} is required.`);
  };
  switch (type) {
    case "command":
      need("command");
      break;
    case "http":
      need("url", "URL");
      if (typeof h.url === "string" && h.url && !/^https?:\/\//i.test(h.url)) problems.push("URL must start with http:// or https://.");
      break;
    case "mcp_tool":
      need("server");
      need("tool");
      break;
    case "prompt":
    case "agent":
      need("prompt");
      break;
    default:
      problems.push("Pick a handler type.");
  }
  if (h.timeout !== undefined && !(typeof h.timeout === "number" && h.timeout > 0)) {
    problems.push("Timeout must be a positive number of seconds.");
  }
  if (h.if !== undefined && !info.tool) problems.push(`"if" only applies to tool events; ${event} ignores it.`);
  return problems;
}

/** Validate a whole hooks object typed into the raw editor. */
export function validateRaw(text: string): { hooks: Json | null; problems: string[] } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return { hooks: null, problems: [`Not valid JSON: ${(e as Error).message}`] };
  }
  const hooks = asObject(parsed);
  if (!hooks) return { hooks: null, problems: ["The hooks block must be a JSON object."] };
  const problems: string[] = [];
  for (const [event, groups] of Object.entries(hooks)) {
    if (!EVENT_INFO[event]) problems.push(`Unknown event "${event}".`);
    if (!Array.isArray(groups)) {
      problems.push(`${event} must be an array of matcher groups.`);
      continue;
    }
    groups.forEach((g, gi) => {
      const group = asObject(g);
      if (!group || !Array.isArray(group.hooks)) {
        problems.push(`${event}[${gi}] needs a "hooks" array.`);
        return;
      }
      const matcher = typeof group.matcher === "string" ? group.matcher : undefined;
      (group.hooks as unknown[]).forEach((h, hi) => {
        const handler = asObject(h);
        if (!handler) problems.push(`${event}[${gi}].hooks[${hi}] must be an object.`);
        else for (const p of validateHandler(event, matcher, handler)) problems.push(`${event}[${gi}].hooks[${hi}]: ${p}`);
      });
    });
  }
  return { hooks, problems };
}

// ---------------------------------------------------------------------------
// Test runs
// ---------------------------------------------------------------------------

function sampleTool(matcher: string | undefined): string {
  const first = (matcher ?? "").split(/[|,]/)[0]?.trim();
  return first && /^[A-Za-z0-9_]+$/.test(first) ? first : "Bash";
}

function sampleToolInput(tool: string, projectDir: string): Json {
  const file = `${projectDir || "/path/to/project"}/src/example.ts`.replace(/\\/g, "/");
  switch (tool) {
    case "Bash":
      return { command: "git status", description: "Show working tree status" };
    case "Edit":
      return { file_path: file, old_string: "foo", new_string: "bar" };
    case "Write":
      return { file_path: file, content: "export {};\n" };
    case "Read":
      return { file_path: file };
    case "Glob":
      return { pattern: "**/*.ts" };
    case "Grep":
      return { pattern: "TODO", path: projectDir };
    case "WebFetch":
      return { url: "https://example.com", prompt: "Summarize" };
    default:
      return {};
  }
}

/**
 * A plausible stdin payload for `event`. Common fields follow the docs;
 * event-specific fields are a best-effort sample — edit before running.
 */
export function sampleInput(event: string, matcher: string | undefined, projectDir: string | null): string {
  const cwd = projectDir ?? "";
  const base: Json = {
    session_id: "skills-editor-test",
    transcript_path: "",
    cwd,
    permission_mode: "default",
    hook_event_name: event,
  };
  const tool = sampleTool(matcher);
  const toolCall = { tool_name: tool, tool_input: sampleToolInput(tool, cwd), tool_use_id: "toolu_test" };
  const first = (matcher ?? "").split(/[|,]/)[0]?.trim() || undefined;
  const extra: Record<string, Json> = {
    PreToolUse: toolCall,
    PermissionRequest: toolCall,
    PermissionDenied: { ...toolCall, reason: "denied by user" },
    PostToolUse: { ...toolCall, tool_response: { stdout: "", stderr: "", exit_code: 0 } },
    PostToolUseFailure: { ...toolCall, error: "Command failed" },
    UserPromptSubmit: { prompt: "Refactor the parser" },
    UserPromptExpansion: { command_name: first ?? "my-skill", prompt: "/my-skill" },
    Stop: { stop_hook_active: false },
    SubagentStop: { stop_hook_active: false, agent_type: first ?? "general-purpose" },
    SubagentStart: { agent_type: first ?? "general-purpose" },
    SessionStart: { source: first ?? "startup" },
    SessionEnd: { reason: first ?? "other" },
    Notification: { message: "Claude needs your permission to use Bash", notification_type: first ?? "permission_prompt" },
    PreCompact: { trigger: first ?? "manual", custom_instructions: "" },
    PostCompact: { trigger: first ?? "manual" },
    ConfigChange: { source: first ?? "user_settings" },
    FileChanged: { file_path: `${cwd}/${first ?? ".env"}`.replace(/\\/g, "/") },
    StopFailure: { error_type: first ?? "rate_limit" },
  };
  return JSON.stringify({ ...base, ...(extra[event] ?? {}) }, null, 2);
}

export interface Verdict {
  tone: "ok" | "block" | "warn";
  headline: string;
  detail: string;
  json: Json | null;
}

/** Explain a test run the way Claude Code would treat it. */
export function interpret(event: string, r: HookTestResult): Verdict {
  if (r.timed_out) {
    return {
      tone: "warn",
      headline: "Timed out",
      detail:
        event === "PreModelSwitch"
          ? "Claude Code would cancel the hook, and a timed-out PreModelSwitch hook blocks the switch."
          : "Claude Code would cancel the hook and discard its output; the action proceeds.",
      json: null,
    };
  }
  const out = r.stdout.trim();
  let json: Json | null = null;
  if (out.startsWith("{") && out.endsWith("}")) {
    try {
      json = asObject(JSON.parse(out));
    } catch {
      json = null;
    }
  }
  if (r.exit_code === 2) {
    return {
      tone: "block",
      headline: "Exit 2 — blocking error",
      detail: "On events that support it, the action is blocked and the reason (JSON decision or stderr) is shown to Claude.",
      json,
    };
  }
  if (r.exit_code === 0) {
    if (json) {
      return { tone: "ok", headline: "Exit 0 — JSON output", detail: "Claude Code parses this and applies any decision fields the event supports.", json };
    }
    return {
      tone: "ok",
      headline: "Exit 0 — success",
      detail: !out
        ? "No output."
        : eventInfo(event).stdoutIsContext
          ? "Plain stdout is added to Claude's context for this event."
          : "Plain stdout only goes to the debug log for this event.",
      json: null,
    };
  }
  return {
    tone: "warn",
    headline: `Exit ${r.exit_code ?? "?"} — non-blocking error`,
    detail: json
      ? "Valid decision JSON is still honored; otherwise the action proceeds."
      : "The action proceeds; the first stderr line is reported.",
    json,
  };
}
