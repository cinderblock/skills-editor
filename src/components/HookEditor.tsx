import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import CodeMirror from "@uiw/react-codemirror";
import { json as jsonLang } from "@codemirror/lang-json";
import {
  hooksDeleteParked,
  hooksDisable,
  hooksEnable,
  hooksSet,
  hooksSetDisableAll,
  hooksTest,
  isConflict,
} from "../api";
import {
  CATEGORIES,
  DEFAULT_TIMEOUT,
  EVENT_INFO,
  EVENT_ORDER,
  HANDLER_TYPES,
  KNOWN_FIELDS,
  TYPE_LABEL,
  describeMatcher,
  eventInfo,
  findHandler,
  groupMatcher,
  groupSize,
  interpret,
  removeHandler,
  sameJson,
  sampleInput,
  upsertHandler,
  validateHandler,
  validateRaw,
  type HandlerType,
  type HookLoc,
  type Json,
} from "../hooksModel";
import type {
  HookGroup,
  HookSelection,
  HookSource,
  HookTarget,
  HookTestResult,
  HooksOverview,
  ParkedHook,
} from "../types";

export interface HookEditorHandle {
  save: () => Promise<void>;
  isDirty: () => boolean;
}

interface Resolved {
  mode: "handler" | "parked" | "new" | "missing";
  group: HookGroup | null;
  source: HookSource | null;
  loc: HookLoc | null;
  handler: Json | null;
  matcher: string | undefined;
  parked: ParkedHook | null;
}

function resolve(ov: HooksOverview | null, sel: HookSelection): Resolved {
  const none: Resolved = { mode: "missing", group: null, source: null, loc: null, handler: null, matcher: undefined, parked: null };
  if (!ov) return none;
  if (sel.kind === "new") return { ...none, mode: "new" };
  for (const group of ov.groups) {
    for (const source of group.sources) {
      if (sel.kind === "parked") {
        const parked = source.parked.find((p) => p.id === sel.id);
        if (parked) {
          return {
            mode: "parked",
            group,
            source,
            loc: null,
            handler: parked.handler as Json,
            matcher: typeof parked.matcher === "string" ? parked.matcher : undefined,
            parked,
          };
        }
      } else if (source.file === sel.file) {
        const loc = { event: sel.event, group: sel.group, handler: sel.handler };
        const handler = findHandler(source, loc);
        if (!handler) return { ...none, group, source };
        return { mode: "handler", group, source, loc, handler, matcher: groupMatcher(source, loc), parked: null };
      }
    }
  }
  return none;
}

function findSource(ov: HooksOverview | null, file: string): HookSource | null {
  for (const g of ov?.groups ?? []) for (const s of g.sources) if (s.file === file) return s;
  return null;
}

const NEW_HANDLER: Json = { type: "command", command: "" };

interface Draft {
  event: string;
  matcher: string;
  handler: Json;
}

/** Type-specific known fields — dropped (and stashed) when the type changes. */
const TYPE_FIELDS: Record<HandlerType, string[]> = {
  command: ["command", "args", "async", "asyncRewake", "shell"],
  http: ["url", "headers", "allowedEnvVars"],
  mcp_tool: ["server", "tool", "input"],
  prompt: ["prompt", "model"],
  agent: ["prompt", "model"],
};

function headersToText(h: unknown): string {
  if (!h || typeof h !== "object") return "";
  return Object.entries(h as Record<string, unknown>)
    .map(([k, v]) => `${k}: ${String(v)}`)
    .join("\n");
}

function textToHeaders(text: string): Record<string, string> | undefined {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const i = line.indexOf(":");
    if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return Object.keys(out).length ? out : undefined;
}

const HookEditor = forwardRef<
  HookEditorHandle,
  {
    overview: HooksOverview | null;
    selection: HookSelection;
    /** Fetch a fresh overview (and update the app's copy). */
    reload: () => Promise<HooksOverview | null>;
    onSelect: (sel: HookSelection | null) => void;
    onStatus: (msg: string) => void;
    onDirtyChange: (dirty: boolean) => void;
    onOpenScript: (path: string, editable: boolean, group: HookGroup | null) => void;
  }
>(function HookEditor({ overview, selection, reload, onSelect, onStatus, onDirtyChange, onOpenScript }, ref) {
  const resolved = useMemo(() => resolve(overview, selection), [overview, selection]);
  const targets = overview?.targets ?? [];

  const initial = (): Draft => ({
    event: resolved.loc?.event ?? resolved.parked?.event ?? "PreToolUse",
    matcher: resolved.matcher ?? (resolved.mode === "new" ? "Bash" : ""),
    handler: resolved.handler ?? NEW_HANDLER,
  });
  const [base, setBase] = useState<Draft>(initial);
  const [draft, setDraft] = useState<Draft>(initial);
  const [targetFile, setTargetFile] = useState<string>(
    (selection.kind === "new" && selection.file) || targets[0]?.file || "",
  );
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [rawMode, setRawMode] = useState(false);
  const [rawText, setRawText] = useState("");
  const [rawBase, setRawBase] = useState("");
  const [inputText, setInputText] = useState(() => JSON.stringify(draft.handler.input ?? {}, null, 2));
  const stash = useRef<Json>({});

  // Keep a clean form in sync with the file (e.g. an external edit).
  const handlerOnDisk = resolved.handler ? JSON.stringify(resolved.handler) : null;
  const dirtyForm = !sameJson(draft, base);
  useEffect(() => {
    if (resolved.mode !== "handler" && resolved.mode !== "parked") return;
    const fresh = initial();
    if (sameJson(fresh, base)) return;
    if (!dirtyForm) {
      setBase(fresh);
      setDraft(fresh);
      setInputText(JSON.stringify(fresh.handler.input ?? {}, null, 2));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handlerOnDisk, resolved.matcher]);

  const source = resolved.source;
  const target: HookTarget | undefined = targets.find((t) => t.file === (source?.file ?? targetFile));
  const editable = resolved.mode === "new" || (resolved.mode === "handler" && !!source?.editable);
  const rawDirty = rawMode && rawText !== rawBase;
  const dirty = (editable && dirtyForm) || rawDirty;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange(false), [onDirtyChange]);

  const info = eventInfo(draft.event);
  const h = draft.handler;
  const type = ((h.type as string) ?? "command") as HandlerType;
  const matcher = info.matcher ? draft.matcher.trim() || undefined : undefined;
  const problems = validateHandler(draft.event, matcher, h);
  let inputProblem: string | null = null;
  if (type === "mcp_tool" && inputText.trim()) {
    try {
      JSON.parse(inputText);
    } catch (e) {
      inputProblem = `Input: ${(e as Error).message}`;
    }
  }
  const allProblems = inputProblem ? [...problems, inputProblem] : problems;

  const setField = (field: string, value: unknown) => {
    setDraft((d) => {
      const next: Json = { ...d.handler };
      if (value === undefined || value === "") delete next[field];
      else next[field] = value;
      return { ...d, handler: next };
    });
  };

  const setType = (next: HandlerType) => {
    setDraft((d) => {
      const handler: Json = { ...d.handler };
      for (const f of TYPE_FIELDS[type]) {
        if (f in handler && !TYPE_FIELDS[next].includes(f)) {
          stash.current[f] = handler[f];
          delete handler[f];
        }
      }
      for (const f of TYPE_FIELDS[next]) {
        if (!(f in handler) && f in stash.current) handler[f] = stash.current[f];
      }
      handler.type = next;
      return { ...d, handler };
    });
  };

  // ---- raw view ----
  const openRaw = () => {
    const text = JSON.stringify(source?.hooks ?? {}, null, 2);
    setRawText(text);
    setRawBase(text);
    setRawMode(true);
  };

  // ---- save ----
  const saveForm = useCallback(
    async (asNew = false) => {
      if (!editable || allProblems.length > 0) return;
      setBusy(true);
      setError(null);
      try {
        const file = source?.file ?? targetFile;
        const fresh = await reload();
        const freshSource = findSource(fresh, file);
        let from: HookLoc | null = resolved.mode === "handler" && !asNew ? resolved.loc : null;
        if (from && freshSource) {
          const onDisk = findHandler(freshSource, from);
          if (!onDisk || !sameJson(onDisk, base.handler) || (groupMatcher(freshSource, from) ?? "") !== base.matcher) {
            setConflict(true);
            return;
          }
        }
        if (from && !freshSource) from = null;
        // An untouched matcher keeps its exact on-disk form ("" vs "*" vs absent),
        // so an in-place edit never regroups the hook.
        const toMatcher =
          from && freshSource && draft.matcher === base.matcher ? groupMatcher(freshSource, from) : matcher;
        let handler = draft.handler;
        if (type === "mcp_tool") {
          handler = { ...handler, input: inputText.trim() ? JSON.parse(inputText) : undefined };
          if (handler.input === undefined) delete handler.input;
        }
        const { hooks, loc } = upsertHandler(
          freshSource?.hooks ?? {},
          from,
          { event: draft.event, matcher: toMatcher },
          handler,
        );
        await hooksSet(file, freshSource?.hash ?? "missing", hooks);
        const saved = { ...draft, matcher: matcher ?? "", handler };
        setBase(saved);
        setDraft(saved);
        setConflict(false);
        onStatus(asNew ? "Saved as a new hook" : "Hook saved");
        await reload();
        onSelect({ kind: "handler", file, ...loc });
      } catch (e) {
        if (isConflict(e)) setConflict(true);
        else setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [editable, allProblems.length, source, targetFile, reload, resolved, base, draft, type, inputText, matcher, onStatus, onSelect],
  );

  const saveRaw = useCallback(async () => {
    if (!source) return;
    const { hooks, problems: rawProblems } = validateRaw(rawText);
    if (!hooks || rawProblems.length > 0) {
      setError(rawProblems.join("\n"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await hooksSet(source.file, source.hash, hooks);
      setRawBase(rawText);
      setRawMode(false);
      onStatus("Hooks saved");
      await reload();
      onSelect(null);
    } catch (e) {
      if (isConflict(e)) setConflict(true);
      else setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [source, rawText, reload, onSelect, onStatus]);

  const save = useCallback(async () => {
    if (rawDirty) await saveRaw();
    else if (dirtyForm) await saveForm();
  }, [rawDirty, dirtyForm, saveRaw, saveForm]);

  useImperativeHandle(ref, () => ({ save, isDirty: () => dirtyRef.current }));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [save]);

  const discard = async () => {
    const fresh = await reload();
    setConflict(false);
    setError(null);
    const r = resolve(fresh, selection);
    const next: Draft = {
      event: r.loc?.event ?? draft.event,
      matcher: r.matcher ?? "",
      handler: r.handler ?? NEW_HANDLER,
    };
    setBase(next);
    setDraft(next);
    setInputText(JSON.stringify(next.handler.input ?? {}, null, 2));
    if (!r.handler && selection.kind === "handler") onSelect(null);
  };

  const run = async (action: () => Promise<unknown>, done: string, after?: HookSelection | null) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      onStatus(done);
      await reload();
      if (after !== undefined) onSelect(after);
    } catch (e) {
      if (isConflict(e)) setConflict(true);
      else setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // ---- test runner ----
  const projectDir = source?.project_dir ?? target?.project_dir ?? null;
  const [stdin, setStdin] = useState(() => sampleInput(draft.event, matcher, projectDir));
  const [stdinTouched, setStdinTouched] = useState(false);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<HookTestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  useEffect(() => {
    if (!stdinTouched) setStdin(sampleInput(draft.event, matcher, projectDir));
  }, [draft.event, matcher, projectDir, stdinTouched]);

  const runTest = async () => {
    setTesting(true);
    setTestError(null);
    setResult(null);
    try {
      JSON.parse(stdin);
    } catch (e) {
      setTestError(`Input isn't valid JSON: ${(e as Error).message}`);
      setTesting(false);
      return;
    }
    try {
      setResult(
        await hooksTest({
          command: String(h.command ?? ""),
          args: Array.isArray(h.args) ? (h.args as unknown[]).map(String) : null,
          shell: typeof h.shell === "string" ? h.shell : null,
          timeout_secs: typeof h.timeout === "number" ? h.timeout : null,
          project_dir: projectDir,
          plugin_root: source?.plugin_root ?? null,
          stdin,
        }),
      );
    } catch (e) {
      setTestError(String(e));
    } finally {
      setTesting(false);
    }
  };

  // ---- rendering ----
  if (resolved.mode === "missing") {
    return (
      <div className="editor-pane empty">
        <div className="empty-hint">
          <p>This hook no longer exists — it may have been edited outside the app.</p>
          <button className="btn" onClick={() => onSelect(null)}>
            Close
          </button>
        </div>
      </div>
    );
  }

  const title =
    resolved.mode === "new" ? "New hook" : `${draft.event}${matcher ? ` · ${matcher}` : ""}`;
  const handlerScripts =
    resolved.mode === "handler" && source && resolved.loc
      ? source.scripts.filter(
          (s) =>
            s.event === resolved.loc!.event &&
            s.group === resolved.loc!.group &&
            s.handler === resolved.loc!.handler,
        )
      : [];
  const siblings = resolved.mode === "handler" && source && resolved.loc ? groupSize(source, resolved.loc) - 1 : 0;
  const matcherChanged = resolved.mode === "handler" && (matcher ?? "") !== (base.matcher || "");
  const extras = Object.entries(h).filter(([k]) => !KNOWN_FIELDS.has(k));
  const verdict = result ? interpret(draft.event, result) : null;
  const readOnlyReason =
    resolved.mode === "parked"
      ? "Disabled — stored by Skills Editor, not in any Claude Code file. Enable to put it back."
      : source && !source.editable
        ? source.parse_error
          ? `The file isn't valid JSON (${source.parse_error}) — fix it by hand first.`
          : source.kind === "plugin"
            ? "Provided by a plugin — change it by updating or disabling the plugin."
            : source.kind === "managed"
              ? "Set by managed policy — only an administrator can change it."
              : source.kind === "skill" || source.kind === "agent"
                ? "Declared in frontmatter — edit that file to change it."
                : "Read-only."
        : null;

  return (
    <div className="editor-pane hook-editor">
      <div className="editor-header">
        <div className="editor-title">
          <span className="editor-skill">{title}</span>
          {source && <span className="badge">{source.file_label}</span>}
          {dirty && <span className="dot-dirty">●</span>}
        </div>
        <div className="editor-actions">
          {resolved.mode === "handler" && source?.editable && !rawMode && (
            <button className="btn" onClick={openRaw}>
              Raw JSON
            </button>
          )}
          {rawMode && (
            <button className="btn" onClick={() => setRawMode(false)}>
              Form view
            </button>
          )}
          {resolved.mode === "handler" && source?.editable && !rawMode && (
            <button
              className="btn"
              disabled={busy || dirty}
              onClick={() =>
                void run(
                  () => hooksDisable(source.file, source.hash, resolved.loc!.event, resolved.loc!.group, resolved.loc!.handler),
                  "Hook disabled",
                  null,
                )
              }
            >
              Disable
            </button>
          )}
          {resolved.mode === "parked" && resolved.parked && (
            <button
              className="btn accent"
              disabled={busy}
              onClick={() => void run(() => hooksEnable(resolved.parked!.id), "Hook enabled", null)}
            >
              Enable
            </button>
          )}
          {(editable && resolved.mode === "handler") || resolved.mode === "parked" ? (
            confirmDelete ? (
              <>
                <button
                  className="btn danger"
                  disabled={busy}
                  onClick={() =>
                    void run(
                      () =>
                        resolved.mode === "parked"
                          ? hooksDeleteParked(resolved.parked!.id)
                          : hooksSet(source!.file, source!.hash, removeHandler(source!.hooks, resolved.loc!)),
                      "Hook deleted",
                      null,
                    )
                  }
                >
                  Confirm delete
                </button>
                <button className="btn" onClick={() => setConfirmDelete(false)}>
                  Keep
                </button>
              </>
            ) : (
              <button className="btn danger" disabled={busy} onClick={() => setConfirmDelete(true)}>
                Delete
              </button>
            )
          ) : null}
          {(editable || rawMode) && (
            <button
              className="btn accent"
              disabled={busy || !dirty || (!rawMode && allProblems.length > 0)}
              onClick={() => void save()}
            >
              {busy ? "Saving…" : resolved.mode === "new" ? "Create" : "Save"}
            </button>
          )}
        </div>
      </div>
      <div className="editor-path">{source?.file ?? target?.file ?? ""}</div>

      <div className="hook-body">
        {readOnlyReason && <div className="banner info">{readOnlyReason}</div>}
        {source?.inactive_reason && <div className="banner warn">Not running: {source.inactive_reason}.</div>}
        {conflict && (
          <div className="banner warn">
            <span>This hook changed on disk since you opened it (Claude Code or another tool edited the file).</span>
            <button className="btn small" onClick={() => void discard()}>
              Discard my edits
            </button>
            {!rawMode && (
              <button className="btn small" onClick={() => void saveForm(true)}>
                Save mine as a new hook
              </button>
            )}
          </div>
        )}
        {error && <div className="banner error">{error}</div>}

        {rawMode ? (
          <div className="hook-raw">
            <div className="hook-hint">
              The whole <code>hooks</code> block of {source?.file_label}. Other settings in the file are left untouched.
            </div>
            <div className="editor-cm raw">
              <CodeMirror
                value={rawText}
                theme="dark"
                height="100%"
                extensions={[jsonLang()]}
                onChange={setRawText}
              />
            </div>
            {rawDirty && validateRaw(rawText).problems.length > 0 && (
              <ul className="problems">
                {validateRaw(rawText).problems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            )}
          </div>
        ) : (
          <fieldset className="hook-form" disabled={!editable}>
            {resolved.mode === "new" && (
              <label className="fm-field">
                <span>Add to</span>
                <select value={targetFile} onChange={(e) => setTargetFile(e.target.value)}>
                  {targets.map((t) => (
                    <option key={t.file} value={t.file}>
                      {t.label}
                    </option>
                  ))}
                </select>
                <span className="field-hint">
                  shared <code>settings.json</code> is usually committed; <code>settings.local.json</code> stays on this machine
                </span>
              </label>
            )}

            <div className="form-row">
              <label className="fm-field grow">
                <span>Event</span>
                <select
                  value={draft.event}
                  onChange={(e) => setDraft((d) => ({ ...d, event: e.target.value }))}
                >
                  {!EVENT_INFO[draft.event] && <option value={draft.event}>{draft.event} (unknown)</option>}
                  {CATEGORIES.map((cat) => (
                    <optgroup key={cat} label={cat}>
                      {EVENT_ORDER.filter((e) => EVENT_INFO[e].category === cat).map((e) => (
                        <option key={e} value={e}>
                          {e}
                        </option>
                      ))}
                    </optgroup>
                  ))}
                </select>
                <span className="field-hint">{info.blurb}</span>
              </label>
              <label className="fm-field grow">
                <span>Matcher{info.matcher ? ` — ${info.matcher.label}` : ""}</span>
                <input
                  list="hook-matcher-values"
                  value={info.matcher ? draft.matcher : ""}
                  disabled={!info.matcher}
                  placeholder={info.matcher ? "empty = everything" : "not used by this event"}
                  onChange={(e) => setDraft((d) => ({ ...d, matcher: e.target.value }))}
                />
                <datalist id="hook-matcher-values">
                  {(info.matcher?.values ?? []).map((v) => (
                    <option key={v} value={v} />
                  ))}
                </datalist>
                <span className="field-hint">
                  {info.matcher ? describeMatcher(matcher) : "fires on every occurrence"}
                  {info.matcher?.regex ? " · lists (a|b) and regexes allowed" : ""}
                </span>
              </label>
            </div>
            {matcherChanged && siblings > 0 && (
              <div className="field-hint warn">
                {siblings} other hook{siblings > 1 ? "s share" : " shares"} the current matcher; only this one moves.
              </div>
            )}

            <label className="fm-field">
              <span>Type</span>
              <select value={type} onChange={(e) => setType(e.target.value as HandlerType)}>
                {HANDLER_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {TYPE_LABEL[t]}
                  </option>
                ))}
              </select>
            </label>

            {type === "command" && (
              <>
                <label className="fm-field">
                  <span>Command</span>
                  <textarea
                    className="mono"
                    rows={2}
                    value={String(h.command ?? "")}
                    placeholder='bash "$CLAUDE_PROJECT_DIR/.claude/hooks/check.sh"'
                    onChange={(e) => setField("command", e.target.value)}
                  />
                </label>
                <label className="fm-field">
                  <span>Arguments — exec form, one per line (leave empty to run through a shell)</span>
                  <textarea
                    className="mono"
                    rows={2}
                    value={Array.isArray(h.args) ? (h.args as unknown[]).join("\n") : ""}
                    onChange={(e) => {
                      const lines = e.target.value.split("\n");
                      setField("args", e.target.value === "" ? undefined : lines);
                    }}
                  />
                </label>
                <div className="form-row">
                  <label className="fm-field">
                    <span>Shell</span>
                    <select
                      value={String(h.shell ?? "")}
                      disabled={Array.isArray(h.args)}
                      onChange={(e) => setField("shell", e.target.value || undefined)}
                    >
                      <option value="">default (bash; PowerShell if no Git Bash)</option>
                      <option value="bash">bash</option>
                      <option value="powershell">powershell</option>
                    </select>
                  </label>
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={h.async === true}
                      onChange={(e) => setField("async", e.target.checked || undefined)}
                    />
                    async — don't wait for it
                  </label>
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={h.asyncRewake === true}
                      onChange={(e) => setField("asyncRewake", e.target.checked || undefined)}
                    />
                    asyncRewake — wake Claude on exit 2
                  </label>
                </div>
              </>
            )}

            {type === "http" && (
              <>
                <label className="fm-field">
                  <span>URL (receives the hook input as a POST)</span>
                  <input value={String(h.url ?? "")} onChange={(e) => setField("url", e.target.value)} />
                </label>
                <label className="fm-field">
                  <span>Headers — one "Name: value" per line; $VAR allowed if listed below</span>
                  <textarea
                    className="mono"
                    rows={2}
                    defaultValue={headersToText(h.headers)}
                    onChange={(e) => setField("headers", textToHeaders(e.target.value))}
                  />
                </label>
                <label className="fm-field">
                  <span>Allowed env vars (comma-separated)</span>
                  <input
                    value={Array.isArray(h.allowedEnvVars) ? (h.allowedEnvVars as string[]).join(", ") : ""}
                    onChange={(e) => {
                      const vars = e.target.value.split(",").map((s) => s.trim()).filter(Boolean);
                      setField("allowedEnvVars", vars.length ? vars : undefined);
                    }}
                  />
                </label>
              </>
            )}

            {type === "mcp_tool" && (
              <>
                <div className="form-row">
                  <label className="fm-field grow">
                    <span>MCP server</span>
                    <input value={String(h.server ?? "")} onChange={(e) => setField("server", e.target.value)} />
                  </label>
                  <label className="fm-field grow">
                    <span>Tool</span>
                    <input value={String(h.tool ?? "")} onChange={(e) => setField("tool", e.target.value)} />
                  </label>
                </div>
                <label className="fm-field">
                  <span>Input (JSON; "${"{tool_input.file_path}"}" pulls from the hook input)</span>
                  <textarea
                    className="mono"
                    rows={3}
                    value={inputText}
                    onChange={(e) => setInputText(e.target.value)}
                  />
                </label>
              </>
            )}

            {(type === "prompt" || type === "agent") && (
              <>
                <label className="fm-field">
                  <span>Prompt — $ARGUMENTS is replaced with the hook input JSON</span>
                  <textarea
                    className="mono"
                    rows={8}
                    value={String(h.prompt ?? "")}
                    onChange={(e) => setField("prompt", e.target.value)}
                  />
                </label>
                <label className="fm-field">
                  <span>Model (empty = the fast default)</span>
                  <input
                    list="hook-models"
                    value={String(h.model ?? "")}
                    onChange={(e) => setField("model", e.target.value || undefined)}
                  />
                  <datalist id="hook-models">
                    <option value="claude-haiku-4-5" />
                    <option value="claude-sonnet-5" />
                    <option value="claude-opus-5" />
                  </datalist>
                </label>
              </>
            )}

            <div className="form-row">
              {(info.tool || h.if !== undefined) && (
                <label className="fm-field grow">
                  <span>If — permission rule, e.g. Bash(git *)</span>
                  <input
                    className="mono"
                    value={String(h.if ?? "")}
                    onChange={(e) => setField("if", e.target.value || undefined)}
                  />
                </label>
              )}
              <label className="fm-field">
                <span>Timeout (s)</span>
                <input
                  type="number"
                  min={1}
                  value={typeof h.timeout === "number" ? h.timeout : ""}
                  placeholder={String(DEFAULT_TIMEOUT[type] ?? "")}
                  onChange={(e) => setField("timeout", e.target.value === "" ? undefined : Number(e.target.value))}
                />
              </label>
              <label className="fm-field grow">
                <span>Status message (spinner text)</span>
                <input
                  value={String(h.statusMessage ?? "")}
                  onChange={(e) => setField("statusMessage", e.target.value || undefined)}
                />
              </label>
            </div>
            {h.once !== undefined && (
              <div className="field-hint">
                once: {String(h.once)} — only honored in skill frontmatter
              </div>
            )}
            {extras.length > 0 && (
              <div className="fm-rest">
                <span>other fields (kept as-is)</span>
                <pre>{JSON.stringify(Object.fromEntries(extras), null, 2)}</pre>
              </div>
            )}
            {editable && dirty && allProblems.length > 0 && (
              <ul className="problems">
                {allProblems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            )}
          </fieldset>
        )}

        {!rawMode && handlerScripts.length > 0 && (
          <div className="hook-section">
            <div className="section-title">Scripts this hook runs</div>
            {handlerScripts.map((s) => (
              <div key={s.path} className="script-row">
                <code>{s.path}</code>
                {s.exists ? (
                  <button
                    className="btn small"
                    onClick={() =>
                      onOpenScript(s.path, !!source?.editable, resolved.group)
                    }
                  >
                    Open
                  </button>
                ) : (
                  <span className="badge disabled">missing</span>
                )}
              </div>
            ))}
          </div>
        )}

        {!rawMode && type === "command" && (
          <div className="hook-section">
            <div className="section-title">Test run</div>
            <div className="hook-hint">
              Really runs the command as currently typed (unsaved edits included) with this input on
              stdin, in {projectDir ? <code>{projectDir}</code> : "your home folder"}. Times out after{" "}
              {typeof h.timeout === "number" ? h.timeout : 60}s.
            </div>
            <div className="editor-cm stdin">
              <CodeMirror
                value={stdin}
                theme="dark"
                height="100%"
                extensions={[jsonLang()]}
                onChange={(v) => {
                  setStdin(v);
                  setStdinTouched(true);
                }}
              />
            </div>
            <div className="test-actions">
              <button
                className="btn accent"
                disabled={testing || !String(h.command ?? "").trim()}
                onClick={() => void runTest()}
              >
                {testing ? "Running…" : "Run test"}
              </button>
              {stdinTouched && (
                <button
                  className="btn small"
                  onClick={() => {
                    setStdinTouched(false);
                    setStdin(sampleInput(draft.event, matcher, projectDir));
                  }}
                >
                  Reset sample input
                </button>
              )}
            </div>
            {testError && <div className="banner error">{testError}</div>}
            {result && verdict && (
              <div className={`test-result tone-${verdict.tone}`}>
                <div className="verdict">
                  <strong>{verdict.headline}</strong>
                  <span>{verdict.detail}</span>
                </div>
                <div className="test-meta">
                  {result.duration_ms} ms · {result.runner}
                </div>
                {verdict.json && (
                  <>
                    <div className="section-title">parsed JSON</div>
                    <pre>{JSON.stringify(verdict.json, null, 2)}</pre>
                  </>
                )}
                <div className="section-title">stdout</div>
                <pre>{result.stdout || "(empty)"}</pre>
                <div className="section-title">stderr</div>
                <pre>{result.stderr || "(empty)"}</pre>
              </div>
            )}
          </div>
        )}
        {!rawMode && type !== "command" && (
          <div className="hook-hint">Only command hooks can be test-run locally.</div>
        )}

        {source?.editable && source.exists && resolved.mode === "handler" && !rawMode && (
          <div className="hook-section">
            <div className="section-title">This file</div>
            <label className="check-label">
              <input
                type="checkbox"
                checked={source.disable_all_hooks}
                disabled={busy}
                onChange={(e) =>
                  void run(
                    () => hooksSetDisableAll(source.file, source.hash, e.target.checked),
                    e.target.checked ? "All hooks in this file disabled" : "Hooks in this file re-enabled",
                  )
                }
              />
              disableAllHooks — turn off every hook at this settings level
            </label>
          </div>
        )}
      </div>
    </div>
  );
});

export default HookEditor;
