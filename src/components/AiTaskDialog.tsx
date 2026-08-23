import { useMemo, useState } from "react";
import { aiStartJob } from "../api";
import type { OpenFile, Skill, SkillGroup } from "../types";

export interface AiTarget {
  mode: "selection" | "skills";
  file?: OpenFile;
  selection?: string;
}

/**
 * Jobs run with read-only tools (the claude CLI cannot write inside
 * ~/.claude, a protected directory) and answer with structured JSON that the
 * app applies itself. Keep this format in sync with parse_response in ai.rs.
 */
const RESPONSE_FORMAT = [
  "Respond with ONLY a JSON object — no prose before or after it — shaped exactly like:",
  '{"files": [{"path": "<path relative to the current directory>", "content": "<the COMPLETE new file content>"}], "notes": "<one or two sentences on what you changed and why>"}',
  "Include every file you want changed, each with its full new content (never a diff or fragment).",
  'If no change is warranted, respond {"files": [], "notes": "<why>"}.',
  "Your tools are read-only; do not attempt to write or edit files — the app applies your response.",
].join("\n");

function selectionPrompt(file: OpenFile, selection: string, request: string): string {
  const rel = file.path.startsWith(file.skill.dir)
    ? file.path.slice(file.skill.dir.length + 1).replace(/\\/g, "/")
    : file.path;
  return [
    `Read the file ${rel} in the current working directory.`,
    "",
    "Find this exact text (verbatim, including whitespace):",
    "<<<SELECTION>>>",
    selection,
    "<<<END SELECTION>>>",
    "",
    `Rewrite ONLY that selected region according to this request: ${request}`,
    "",
    "Leave everything outside the selected region byte-identical. Preserve the",
    "file's indentation and newline style; if it has YAML frontmatter, keep it valid.",
    "If you cannot find the exact text (it may have changed), apply the request to the closest match.",
    "",
    RESPONSE_FORMAT,
  ].join("\n");
}

function skillPrompt(skill: Skill, request: string): string {
  return [
    `You are improving the AI skill "${skill.name}" located in the current working directory.`,
    "Its entry point is SKILL.md (YAML frontmatter + markdown instructions).",
    "Explore the existing files with your read tools before deciding what to change.",
    "",
    `Request: ${request}`,
    "",
    "Keep the SKILL.md frontmatter valid YAML with accurate name and description fields,",
    "and keep the writing style consistent with the existing content.",
    "",
    RESPONSE_FORMAT,
  ].join("\n");
}

export default function AiTaskDialog({
  target,
  groups,
  onClose,
  onStarted,
}: {
  target: AiTarget;
  groups: SkillGroup[];
  onClose: () => void;
  onStarted: (count: number) => void;
}) {
  const [request, setRequest] = useState("");
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const editableSkills = useMemo(
    () =>
      groups
        .filter((g) => g.kind !== "plugin")
        .flatMap((g) => g.skills.filter((s) => s.editable).map((s) => ({ g, s }))),
    [groups],
  );
  const [checked, setChecked] = useState<Record<string, boolean>>(() => {
    const init: Record<string, boolean> = {};
    if (target.mode === "skills" && target.file) init[target.file.skill.id] = true;
    return init;
  });

  const run = async () => {
    if (!request.trim()) {
      setError("Describe what to do first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      let count = 0;
      const chosenModel = model || null;
      if (target.mode === "selection" && target.file && target.selection) {
        await aiStartJob(
          `Selection in ${target.file.skill.name}`,
          target.file.skill.dir,
          selectionPrompt(target.file, target.selection, request.trim()),
          chosenModel,
        );
        count = 1;
      } else {
        const seen = new Set<string>();
        const chosen = editableSkills.filter(
          ({ s }) => checked[s.id] && !seen.has(s.id) && (seen.add(s.id), true),
        );
        if (chosen.length === 0) {
          setError("Pick at least one skill.");
          setBusy(false);
          return;
        }
        for (const { s } of chosen) {
          await aiStartJob(s.name, s.dir, skillPrompt(s, request.trim()), chosenModel);
          count++;
        }
      }
      onStarted(count);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>AI task</h2>
        {target.mode === "selection" ? (
          <>
            <p className="modal-hint">
              Rewrites the selected text in{" "}
              <code>
                {target.file?.skill.name}
              </code>{" "}
              using <code>claude -p</code>. The file updates in place when the
              job finishes.
            </p>
            <pre className="selection-preview">{target.selection}</pre>
          </>
        ) : (
          <>
            <p className="modal-hint">
              Runs <code>claude -p</code> once per selected skill, in parallel.
              Files update in place as jobs finish.
            </p>
            <div className="skill-picker">
              {editableSkills.map(({ g, s }) => (
                <label key={s.id} className="pick-row">
                  <input
                    type="checkbox"
                    checked={checked[s.id] ?? false}
                    onChange={(e) =>
                      setChecked({ ...checked, [s.id]: e.target.checked })
                    }
                  />
                  <span className="pick-name">{s.name}</span>
                  <span className="pick-group">{g.label}</span>
                </label>
              ))}
            </div>
          </>
        )}
        <textarea
          className="prompt-input"
          rows={4}
          placeholder="What should Claude do? e.g. “Tighten the description and add a Troubleshooting section”"
          value={request}
          onChange={(e) => setRequest(e.target.value)}
          autoFocus
        />
        {error && <div className="modal-error">{error}</div>}
        <div className="modal-actions space-between">
          <label className="model-pick">
            <span>model</span>
            <select value={model} onChange={(e) => setModel(e.target.value)}>
              <option value="">default (your claude config)</option>
              <option value="haiku">haiku — fastest</option>
              <option value="sonnet">sonnet</option>
              <option value="opus">opus</option>
            </select>
          </label>
          <div className="modal-actions">
            <button className="btn" onClick={onClose} disabled={busy}>
              Cancel
            </button>
            <button className="btn accent" onClick={() => void run()} disabled={busy}>
              {busy ? "Starting…" : "Run"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
