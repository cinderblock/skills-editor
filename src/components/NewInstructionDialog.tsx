import { useMemo, useState } from "react";
import { instructionsCreate } from "../api";
import { createOptions, existingFor } from "../instructionsModel";
import type { InstrOverview } from "../types";

const USER = "__user__";

export default function NewInstructionDialog({
  overview,
  initialProject,
  onClose,
  onCreated,
}: {
  overview: InstrOverview;
  /** Preselected project dir (from the current selection). */
  initialProject: string | null;
  onClose: () => void;
  onCreated: (path: string, note: string | null) => void;
}) {
  const [scope, setScope] = useState<string>(initialProject ?? USER);
  const options = useMemo(() => createOptions(scope !== USER), [scope]);
  const group = overview.groups.find((g) =>
    scope === USER ? g.kind === "user" : g.kind === "project" && g.project_dir === scope,
  );
  const firstFree = options.find((o) => !existingFor(group, o)) ?? options[0];
  const [kind, setKind] = useState(firstFree.kind);
  const option = options.find((o) => o.kind === kind) ?? firstFree;
  const [name, setName] = useState("");
  const [paths, setPaths] = useState("");
  const [gitignore, setGitignore] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const clash = existingFor(group, option);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const res = await instructionsCreate({
        project: scope === USER ? null : scope,
        kind: option.kind,
        name: option.kind === "rule" ? name.trim() : null,
        paths: option.kind === "rule" ? paths.split("\n").map((p) => p.trim()).filter(Boolean) : [],
        gitignore: option.kind === "local" && gitignore,
      });
      onCreated(res.path, res.note);
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
        <h2>New instructions file</h2>
        <label className="fm-field">
          <span>Where</span>
          <select
            value={scope}
            onChange={(e) => {
              setScope(e.target.value);
              setKind(createOptions(e.target.value !== USER)[0].kind);
            }}
          >
            <option value={USER}>Your user instructions (~/.claude)</option>
            <optgroup label="Projects">
              {overview.projects.map((p) => (
                <option key={p.dir} value={p.dir}>
                  {p.label} — {p.dir}
                </option>
              ))}
            </optgroup>
          </select>
        </label>
        <label className="fm-field">
          <span>File</span>
          <select value={option.kind} onChange={(e) => setKind(e.target.value as typeof kind)}>
            {options.map((o) => (
              <option key={o.kind} value={o.kind}>
                {o.label}
                {existingFor(group, o) ? " (exists)" : ""}
              </option>
            ))}
          </select>
        </label>
        {option.kind === "rule" && (
          <>
            <label className="fm-field">
              <span>Rule name</span>
              <input value={name} placeholder="testing" onChange={(e) => setName(e.target.value)} autoFocus />
            </label>
            <label className="fm-field">
              <span>Only load for these files — globs, one per line (empty = always load)</span>
              <textarea
                className="mono"
                rows={3}
                value={paths}
                placeholder={"src/**/*.test.ts\ndocs/**"}
                onChange={(e) => setPaths(e.target.value)}
              />
            </label>
          </>
        )}
        {option.kind === "local" && (
          <label className="check-label">
            <input type="checkbox" checked={gitignore} onChange={(e) => setGitignore(e.target.checked)} />
            Add CLAUDE.local.md to the project's .gitignore
          </label>
        )}
        {clash && <div className="modal-error">{clash.label} already exists — open it from the sidebar instead.</div>}
        {error && <div className="modal-error">{error}</div>}
        <div className="modal-actions">
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            className="btn accent"
            disabled={busy || !!clash || (option.kind === "rule" && !name.trim())}
            onClick={() => void create()}
          >
            {busy ? "Creating…" : "Create"}
          </button>
        </div>
      </div>
    </div>
  );
}
