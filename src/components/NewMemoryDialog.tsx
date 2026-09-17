import { useState } from "react";
import { memoryCreate } from "../api";
import { TYPE_HELP } from "../memoryModel";
import type { MemoryOverview, MemoryStore } from "../types";

export default function NewMemoryDialog({
  overview,
  store,
  onClose,
  onCreated,
}: {
  overview: MemoryOverview;
  store: MemoryStore;
  onClose: () => void;
  onCreated: (path: string) => void;
}) {
  const [dir, setDir] = useState(store.dir);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [kind, setKind] = useState(overview.types[0] ?? "project");
  const [body, setBody] = useState("");
  const [index, setIndex] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      onCreated(await memoryCreate({ dir, name, description, kind, body, index }));
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
        <h2>New memory note</h2>
        <p className="modal-hint">
          Normally Claude writes these itself. Adding one by hand is for things you want it to know from the
          next session on.
        </p>
        <label className="fm-field">
          <span>Folder</span>
          <select value={dir} onChange={(e) => setDir(e.target.value)}>
            {overview.stores.map((s) => (
              <option key={s.key} value={s.dir}>
                {s.label}
                {s.kind === "agent" ? ` (subagent, ${s.agent?.scope})` : ""} — {s.dir}
              </option>
            ))}
          </select>
        </label>
        <label className="fm-field">
          <span>Name</span>
          <input
            value={name}
            placeholder="Deploy needs the VPN"
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </label>
        <label className="fm-field">
          <span>Description — the one line Claude sees in the index</span>
          <input
            value={description}
            placeholder="connect the VPN before deploying, or the host refuses the key"
            onChange={(e) => setDescription(e.target.value)}
          />
        </label>
        <label className="fm-field">
          <span>Type</span>
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            {overview.types.map((t) => (
              <option key={t} value={t}>
                {t} — {TYPE_HELP[t] ?? ""}
              </option>
            ))}
          </select>
        </label>
        <label className="fm-field">
          <span>What Claude should remember</span>
          <textarea rows={6} value={body} onChange={(e) => setBody(e.target.value)} />
        </label>
        <label className="check-label">
          <input type="checkbox" checked={index} onChange={(e) => setIndex(e.target.checked)} />
          Add it to MEMORY.md (without this, Claude probably won't find it)
        </label>
        {error && <div className="modal-error">{error}</div>}
        <div className="modal-actions">
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn accent" disabled={busy || !name.trim()} onClick={() => void create()}>
            {busy ? "Saving…" : "Save note"}
          </button>
        </div>
      </div>
    </div>
  );
}
