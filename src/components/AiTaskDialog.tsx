import { useMemo, useState } from "react";
import { aiStartJob } from "../api";
import type { OpenFile, Skill, SkillGroup } from "../types";

export interface AiTarget {
  mode: "selection" | "skills";
  file?: OpenFile;
  selection?: string;
}

function selectionPrompt(file: OpenFile, selection: string, request: string): string {
  return [
    `In the file at this exact path: ${file.path}`,
    "",
    "Find this exact text (verbatim, including whitespace):",
    "<<<SELECTION>>>",
    selection,
    "<<<END SELECTION>>>",
    "",
    `Rewrite ONLY that selected region according to this request: ${request}`,
    "",
    "Rules: edit the file in place; do not change anything outside the selected region;",
    "preserve the file's indentation style and, if the file has YAML frontmatter, keep it valid.",
    "If you cannot find the exact text (it may have changed), find the closest match and apply the request there.",
  ].join("\n");
}

function skillPrompt(skill: Skill, request: string): string {
  return [
    `You are improving the AI skill "${skill.name}" located in the current working directory.`,
    "Its entry point is SKILL.md (YAML frontmatter + markdown instructions).",
    "",
    `Request: ${request}`,
    "",
    "Rules: apply the changes directly to the files in this directory.",
    "Keep the SKILL.md frontmatter valid YAML with accurate name and description fields.",
    "Keep the writing style consistent with the existing content.",
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
