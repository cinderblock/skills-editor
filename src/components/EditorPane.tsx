import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type ReactNode,
} from "react";
import CodeMirror, { EditorView, keymap } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import { markdown } from "@codemirror/lang-markdown";
import { yaml as yamlLang } from "@codemirror/lang-yaml";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { StreamLanguage } from "@codemirror/language";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { powerShell } from "@codemirror/legacy-modes/mode/powershell";
import { ruby } from "@codemirror/legacy-modes/mode/ruby";
import { perl } from "@codemirror/legacy-modes/mode/perl";
import { lua } from "@codemirror/legacy-modes/mode/lua";
import { readSkillFile, writeSkillFile } from "../api";
import {
  joinFrontmatter,
  readFields,
  readMemoryType,
  splitFrontmatter,
  updateFields,
  updateMemoryType,
} from "../frontmatter";
import { relativeTo } from "../paths";
import type { OpenFile, Skill } from "../types";

function languageFor(path: string): Extension[] {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "md":
    case "markdown":
      return [markdown()];
    case "yml":
    case "yaml":
      return [yamlLang()];
    case "json":
      return [json()];
    case "js":
    case "mjs":
    case "cjs":
      return [javascript()];
    case "ts":
    case "mts":
    case "tsx":
      return [javascript({ typescript: true, jsx: ext === "tsx" })];
    case "py":
      return [python()];
    case "html":
    case "htm":
      return [html()];
    case "css":
      return [css()];
    case "sh":
    case "bash":
    case "zsh":
      return [StreamLanguage.define(shell)];
    case "ps1":
    case "psm1":
      return [StreamLanguage.define(powerShell)];
    case "rb":
      return [StreamLanguage.define(ruby)];
    case "pl":
      return [StreamLanguage.define(perl)];
    case "lua":
      return [StreamLanguage.define(lua)];
    default:
      return [];
  }
}

export interface EditorPaneHandle {
  save: () => Promise<void>;
  isDirty: () => boolean;
}

interface Props {
  file: OpenFile | null;
  /** Bumped when an AI job finishes; triggers a disk-freshness check. */
  reloadToken: number;
  onStatus: (msg: string) => void;
  onDirtyChange: (dirty: boolean) => void;
  onDeleteSkill: (skill: Skill) => void;
  onToggleDisabled: (skill: Skill) => void;
  onAiSelection: (file: OpenFile, selection: string) => void;
  /** Extra details shown under the path (instruction files). */
  info?: ReactNode;
  onSaved?: (file: OpenFile) => void;
}

const KIND_TITLE: Record<string, string> = {
  script: "Hook script",
  instruction: "Instructions",
  memory: "Memory",
};

/** Kept in step with memory.rs MEMORY_TYPES. */
const MEMORY_TYPES = ["user", "feedback", "project", "reference"];

const EditorPane = forwardRef<EditorPaneHandle, Props>(function EditorPane(
  { file, reloadToken, onStatus, onDirtyChange, onDeleteSkill, onToggleDisabled, onAiSelection, info, onSaved },
  ref,
) {
  const [diskContent, setDiskContent] = useState<string | null>(null);
  const [text, setText] = useState<string>("");
  const [rawMode, setRawMode] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [externalChange, setExternalChange] = useState(false);
  const [selection, setSelection] = useState("");

  const dirty = diskContent !== null && text !== diskContent;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const textRef = useRef(text);
  textRef.current = text;
  const fileRef = useRef(file);
  fileRef.current = file;

  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange]);

  const load = useCallback(async () => {
    const f = fileRef.current;
    if (!f) return;
    try {
      const content = await readSkillFile(f.path);
      setDiskContent(content);
      setText(content);
      setExternalChange(false);
      setLoadError(null);
    } catch (e) {
      setLoadError(String(e));
      setDiskContent(null);
      setText("");
    }
  }, []);

  useEffect(() => {
    setSelection("");
    setRawMode(false);
    if (file) void load();
    else {
      setDiskContent(null);
      setText("");
      setLoadError(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file?.path]);

  // An AI job finished — check whether the file changed under us.
  useEffect(() => {
    const f = fileRef.current;
    if (!f || reloadToken === 0) return;
    void (async () => {
      try {
        const onDisk = await readSkillFile(f.path);
        if (onDisk === textRef.current) {
          setDiskContent(onDisk);
          setExternalChange(false);
          return;
        }
        if (!dirtyRef.current) {
          setDiskContent(onDisk);
          setText(onDisk);
          onStatus("Reloaded — file was updated on disk");
        } else if (onDisk !== diskContent) {
          setExternalChange(true);
        }
      } catch {
        // File may have been deleted by the job; leave the buffer as-is.
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadToken]);

  const save = useCallback(async () => {
    const f = fileRef.current;
    if (!f) return;
    if (!f.skill.editable) {
      onStatus(f.kind && f.kind !== "skill" ? "This file is read-only" : "This skill is read-only (plugin cache)");
      return;
    }
    try {
      await writeSkillFile(f.path, textRef.current);
      setDiskContent(textRef.current);
      setExternalChange(false);
      onStatus("Saved");
      onSaved?.(f);
    } catch (e) {
      onStatus(`Save failed: ${e}`);
      throw e;
    }
  }, [onStatus, onSaved]);

  useImperativeHandle(ref, () => ({
    save,
    isDirty: () => dirtyRef.current,
  }));

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void save().catch(() => {});
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [save]);

  const saveKeymap: Extension = keymap.of([
    {
      key: "Mod-s",
      run: () => {
        void save().catch(() => {});
        return true;
      },
    },
  ]);

  const trackSelection = EditorView.updateListener.of((update) => {
    if (update.selectionSet || update.docChanged) {
      const sel = update.state.selection.main;
      setSelection(update.state.sliceDoc(sel.from, sel.to));
    }
  });

  if (!file) {
    return (
      <div className="editor-pane empty">
        <div className="empty-hint">
          <h2>Skills Editor</h2>
          <p>Pick a skill or hook on the left, or install new skills from the catalog.</p>
        </div>
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="editor-pane empty">
        <div className="empty-hint error">
          <p>Could not open {file.path}</p>
          <pre>{loadError}</pre>
        </div>
      </div>
    );
  }

  const isSkillMd = file.path.replace(/\\/g, "/").endsWith("/SKILL.md");
  const isMemoryNote = file.kind === "memory" && !file.path.replace(/\\/g, "/").endsWith("/MEMORY.md");
  const { frontmatter, body } = splitFrontmatter(text);
  const fields = readFields(frontmatter, isMemoryNote ? ["type"] : []);
  const memoryType = isMemoryNote ? readMemoryType(frontmatter) : "";
  const structured = (isSkillMd || isMemoryNote) && !rawMode && frontmatter !== null;
  const readOnly = !file.skill.editable;
  const standIn = !!file.kind && file.kind !== "skill";
  const skillActions = !readOnly && !standIn;
  const relPath = relativeTo(file.path, file.skill.dir);

  return (
    <div className="editor-pane">
      <div className="editor-header">
        <div className="editor-title">
          <span className="editor-skill">{standIn ? KIND_TITLE[file.kind!] : file.skill.name}</span>
          <span className="editor-file">{file.kind === "instruction" ? file.skill.name : relPath}</span>
          {standIn && <span className="badge">{file.group.label}</span>}
          {dirty && <span className="dot-dirty">●</span>}
          {file.skill.disabled && <span className="badge disabled">disabled</span>}
          {readOnly && <span className="badge readonly">read-only</span>}
        </div>
        <div className="editor-actions">
          {selection.length > 0 && (
            <button
              className="btn accent"
              onClick={() => onAiSelection(file, selection)}
            >
              AI edit selection
            </button>
          )}
          {(isSkillMd || isMemoryNote) && frontmatter !== null && (
            <button className="btn" onClick={() => setRawMode(!rawMode)}>
              {rawMode ? "Structured view" : "Raw view"}
            </button>
          )}
          {!readOnly && (
            <button className="btn" disabled={!dirty} onClick={() => void save().catch(() => {})}>
              Save
            </button>
          )}
          {skillActions && (
            <button className="btn" onClick={() => onToggleDisabled(file.skill)}>
              {file.skill.disabled ? "Enable skill" : "Disable skill"}
            </button>
          )}
          {skillActions && (
            <button className="btn danger" onClick={() => onDeleteSkill(file.skill)}>
              Delete skill
            </button>
          )}
        </div>
      </div>
      <div className="editor-path">{file.path}</div>
      {info}

      {externalChange && (
        <div className="banner warn">
          <span>
            This file changed on disk (likely an AI edit) and you have unsaved
            changes.
          </span>
          <button className="btn" onClick={() => void load()}>
            Reload from disk
          </button>
          <button className="btn" onClick={() => setExternalChange(false)}>
            Keep my version
          </button>
        </div>
      )}

      {structured && (
        <div className="frontmatter-panel">
          <label className="fm-field">
            <span>name</span>
            <input
              value={fields.name}
              readOnly={readOnly}
              onChange={(e) =>
                setText(
                  joinFrontmatter(
                    updateFields(frontmatter, e.target.value, fields.description),
                    body,
                  ),
                )
              }
            />
          </label>
          <label className="fm-field">
            <span>description</span>
            <textarea
              value={fields.description}
              readOnly={readOnly}
              rows={3}
              onChange={(e) =>
                setText(
                  joinFrontmatter(
                    updateFields(frontmatter, fields.name, e.target.value),
                    body,
                  ),
                )
              }
            />
          </label>
          {isMemoryNote && (
            <label className="fm-field">
              <span>type</span>
              <select
                value={memoryType}
                disabled={readOnly}
                onChange={(e) =>
                  setText(joinFrontmatter(updateMemoryType(frontmatter, e.target.value), body))
                }
              >
                {!MEMORY_TYPES.includes(memoryType) && (
                  <option value={memoryType}>{memoryType || "(none)"}</option>
                )}
                {MEMORY_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
          )}
          {fields.restSummary && (
            <div className="fm-rest">
              <span>other frontmatter (edit in raw view)</span>
              <pre>{fields.restSummary}</pre>
            </div>
          )}
        </div>
      )}

      <div className="editor-cm">
        <CodeMirror
          value={structured ? body : text}
          readOnly={readOnly}
          theme="dark"
          height="100%"
          extensions={[
            ...languageFor(structured ? "body.md" : file.path),
            saveKeymap,
            trackSelection,
            EditorView.lineWrapping,
          ]}
          onChange={(value) => {
            if (structured) setText(joinFrontmatter(frontmatter, value));
            else setText(value);
          }}
        />
      </div>
    </div>
  );
});

export default EditorPane;
