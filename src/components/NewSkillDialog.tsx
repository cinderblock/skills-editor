import { useMemo, useState } from "react";
import { createSkill } from "../api";
import type { SkillGroup } from "../types";

export default function NewSkillDialog({
  groups,
  onClose,
  onCreated,
}: {
  groups: SkillGroup[];
  onClose: () => void;
  onCreated: (skillMdPath: string) => void;
}) {
  const roots = useMemo(
    () =>
      groups
        .filter((g) => g.kind === "user" || g.kind === "project" || g.kind === "extra")
        .map((g) => ({ label: g.label, kind: g.kind, root: g.detail })),
    [groups],
  );
  const [root, setRoot] = useState(roots[0]?.root ?? "");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await createSkill(root, name.trim(), description.trim());
      onCreated(path);
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
        <h2>New skill</h2>
        <label className="fm-field">
          <span>Location</span>
          <select value={root} onChange={(e) => setRoot(e.target.value)}>
            {roots.map((r) => (
              <option key={r.root} value={r.root}>
                {r.kind === "user" ? "User skills" : r.label} — {r.root}
              </option>
            ))}
          </select>
        </label>
        <label className="fm-field">
          <span>Name (directory name, kebab-case)</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-new-skill"
            autoFocus
          />
        </label>
        <label className="fm-field">
          <span>Description (when should the agent use it?)</span>
          <textarea
            rows={3}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </label>
        {error && <div className="modal-error">{error}</div>}
        <div className="modal-actions">
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            className="btn accent"
            disabled={busy || !name.trim() || !root}
            onClick={() => void create()}
          >
            {busy ? "Creating…" : "Create"}
          </button>
        </div>
      </div>
    </div>
  );
}
