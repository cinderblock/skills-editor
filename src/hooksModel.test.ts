import { describe, expect, test } from "bun:test";
import {
  describeMatcher,
  flatten,
  interpret,
  removeHandler,
  sampleInput,
  upsertHandler,
  validateHandler,
  validateRaw,
  type Json,
} from "./hooksModel";
import type { HookTestResult } from "./types";

const cmd = (command: string): Json => ({ type: "command", command });

const base = (): Json => ({
  PreToolUse: [
    { matcher: "Bash", hooks: [cmd("a"), cmd("b")] },
    { matcher: "Edit|Write", hooks: [cmd("c")] },
  ],
  Stop: [{ hooks: [{ type: "prompt", prompt: "p" }] }],
});

describe("upsertHandler", () => {
  test("edits in place when event and matcher are unchanged", () => {
    const { hooks, loc } = upsertHandler(base(), { event: "PreToolUse", group: 0, handler: 1 }, { event: "PreToolUse", matcher: "Bash" }, cmd("B"));
    expect(loc).toEqual({ event: "PreToolUse", group: 0, handler: 1 });
    expect((hooks.PreToolUse as Json[])[0].hooks).toEqual([cmd("a"), cmd("B")]);
  });

  test("moving to another matcher only moves that handler", () => {
    const { hooks, loc } = upsertHandler(base(), { event: "PreToolUse", group: 0, handler: 0 }, { event: "PreToolUse", matcher: "Edit|Write" }, cmd("a"));
    const groups = hooks.PreToolUse as Json[];
    expect(groups[0].hooks).toEqual([cmd("b")]);
    expect(groups[1].hooks).toEqual([cmd("c"), cmd("a")]);
    expect(loc).toEqual({ event: "PreToolUse", group: 1, handler: 1 });
  });

  test("moving the last handler out prunes its group and event", () => {
    const { hooks, loc } = upsertHandler(base(), { event: "Stop", group: 0, handler: 0 }, { event: "SubagentStop", matcher: undefined }, { type: "prompt", prompt: "p" });
    expect(hooks.Stop).toBeUndefined();
    expect(hooks.SubagentStop).toEqual([{ hooks: [{ type: "prompt", prompt: "p" }] }]);
    expect(loc).toEqual({ event: "SubagentStop", group: 0, handler: 0 });
  });

  test("group indices are computed after the removal", () => {
    // Removing group 0's only handler shifts the Edit|Write group to index 0.
    const hooks0 = base();
    (hooks0.PreToolUse as Json[])[0].hooks = [cmd("a")];
    const { hooks, loc } = upsertHandler(hooks0, { event: "PreToolUse", group: 0, handler: 0 }, { event: "PreToolUse", matcher: "Edit|Write" }, cmd("a"));
    expect((hooks.PreToolUse as Json[]).length).toBe(1);
    expect(loc).toEqual({ event: "PreToolUse", group: 0, handler: 1 });
  });

  test("new handlers join a matching group or create one; the input is not mutated", () => {
    const input = base();
    const snapshot = JSON.stringify(input);
    const joined = upsertHandler(input, null, { event: "PreToolUse", matcher: "Bash" }, cmd("z"));
    expect(joined.loc).toEqual({ event: "PreToolUse", group: 0, handler: 2 });
    const created = upsertHandler(input, null, { event: "PostToolUse", matcher: undefined }, cmd("z"));
    expect(created.hooks.PostToolUse).toEqual([{ hooks: [cmd("z")] }]);
    expect(JSON.stringify(input)).toBe(snapshot);
  });

  test('an explicit "" matcher is distinct from an absent one', () => {
    const hooks0: Json = { Stop: [{ matcher: "", hooks: [cmd("a")] }] };
    const inPlace = upsertHandler(hooks0, { event: "Stop", group: 0, handler: 0 }, { event: "Stop", matcher: "" }, cmd("A"));
    expect(inPlace.hooks.Stop).toEqual([{ matcher: "", hooks: [cmd("A")] }]);
  });

  test("keeps unknown group keys when editing in place", () => {
    const hooks0: Json = { Stop: [{ hooks: [cmd("a")], note: "keep me" }] };
    const { hooks } = upsertHandler(hooks0, { event: "Stop", group: 0, handler: 0 }, { event: "Stop", matcher: undefined }, cmd("b"));
    expect((hooks.Stop as Json[])[0].note).toBe("keep me");
  });
});

test("removeHandler prunes empty containers", () => {
  const h1 = removeHandler(base(), { event: "Stop", group: 0, handler: 0 });
  expect(h1.Stop).toBeUndefined();
  const h2 = removeHandler(base(), { event: "PreToolUse", group: 0, handler: 0 });
  expect((h2.PreToolUse as Json[])[0].hooks).toEqual([cmd("b")]);
});

test("flatten orders by event then position and skips malformed entries", () => {
  const flat = flatten({ ...base(), Bogus: "x", PostToolUse: [null, { hooks: [5, cmd("d")] }] });
  expect(flat.map((f) => `${f.event}:${f.group}:${f.handler}`)).toEqual([
    "Stop:0:0",
    "PreToolUse:0:0",
    "PreToolUse:0:1",
    "PreToolUse:1:0",
    "PostToolUse:1:1",
  ]);
  expect(flat[1].matcher).toBe("Bash");
});

test("describeMatcher mirrors Claude Code's rules", () => {
  expect(describeMatcher(undefined)).toBe("matches everything");
  expect(describeMatcher("*")).toBe("matches everything");
  expect(describeMatcher("Bash")).toBe("exact match: Bash");
  expect(describeMatcher("Edit|Write")).toBe("exact match on any of: Edit, Write");
  expect(describeMatcher("mcp__.*")).toBe("regular expression (unanchored)");
  expect(describeMatcher("(")).toStartWith("invalid regular expression");
});

describe("validation", () => {
  test("flags missing fields, bad timeouts, misplaced if/matcher", () => {
    expect(validateHandler("PreToolUse", "Bash", cmd("x"))).toEqual([]);
    const problems = validateHandler("Stop", "x", { type: "command", command: " ", timeout: 0, if: "Bash(*)" });
    expect(problems.join("\n")).toContain("Stop ignores matchers");
    expect(problems.join("\n")).toContain("command is required");
    expect(problems.join("\n")).toContain("Timeout must be");
    expect(problems.join("\n")).toContain('"if" only applies');
    expect(validateHandler("Nope", undefined, cmd("x"))[0]).toContain("not a Claude Code hook event");
    expect(validateHandler("PreToolUse", undefined, { type: "http", url: "ftp://x" })).toEqual([
      "URL must start with http:// or https://.",
    ]);
  });

  test("validateRaw reports JSON and shape errors", () => {
    expect(validateRaw("{").problems[0]).toStartWith("Not valid JSON");
    expect(validateRaw("[]").problems).toEqual(["The hooks block must be a JSON object."]);
    const r = validateRaw(JSON.stringify({ PreToolUsee: [], Stop: [{ hooks: [{ type: "agent" }] }] }));
    expect(r.problems).toEqual(['Unknown event "PreToolUsee".', "Stop[0].hooks[0]: prompt is required."]);
    expect(validateRaw(JSON.stringify(base())).problems).toEqual([]);
  });
});

describe("test runs", () => {
  const result = (over: Partial<HookTestResult>): HookTestResult => ({
    exit_code: 0,
    stdout: "",
    stderr: "",
    timed_out: false,
    duration_ms: 1,
    runner: "bash -c",
    ...over,
  });

  test("interpret distinguishes success, JSON, block, error, timeout", () => {
    expect(interpret("PreToolUse", result({})).headline).toBe("Exit 0 — success");
    expect(interpret("UserPromptSubmit", result({ stdout: "hi" })).detail).toContain("added to Claude's context");
    expect(interpret("PreToolUse", result({ stdout: "hi" })).detail).toContain("debug log");
    const js = interpret("PreToolUse", result({ stdout: '{"hookSpecificOutput":{"permissionDecision":"deny"}}' }));
    expect(js.headline).toBe("Exit 0 — JSON output");
    expect(js.json).toEqual({ hookSpecificOutput: { permissionDecision: "deny" } });
    expect(interpret("PreToolUse", result({ exit_code: 2 })).tone).toBe("block");
    expect(interpret("PreToolUse", result({ exit_code: 1 })).tone).toBe("warn");
    expect(interpret("PreModelSwitch", result({ timed_out: true, exit_code: null })).detail).toContain("blocks the switch");
  });

  test("sampleInput is valid JSON shaped for the event and matcher", () => {
    const pre = JSON.parse(sampleInput("PreToolUse", "Edit|Write", "C:\\proj"));
    expect(pre.hook_event_name).toBe("PreToolUse");
    expect(pre.tool_name).toBe("Edit");
    expect(pre.tool_input.file_path).toBe("C:/proj/src/example.ts");
    expect(JSON.parse(sampleInput("PreToolUse", "mcp__.*", null)).tool_name).toBe("Bash");
    expect(JSON.parse(sampleInput("SessionStart", "resume", null)).source).toBe("resume");
    expect(JSON.parse(sampleInput("TeammateIdle", undefined, null))).toEqual({
      session_id: "skills-editor-test",
      transcript_path: "",
      cwd: "",
      permission_mode: "default",
      hook_event_name: "TeammateIdle",
    });
  });
});
