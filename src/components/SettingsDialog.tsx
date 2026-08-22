import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "../api";

export default function SettingsDialog({
  onClose,
  onSaved,
}: {
  onClose: () => void;
  onSaved: () => void;
}) {
  const [repoPath, setRepoPath] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [extraRoots, setExtraRoots] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    void getSettings()
      .then((s) => {
        setRepoPath(s.repo_path ?? "");
        setRemoteUrl(s.remote_url ?? "");
        setExtraRoots(s.extra_roots.join("\n"));
        setLoaded(true);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const save = async () => {
    try {
      await saveSettings({
        repo_path: repoPath.trim() || null,
        remote_url: remoteUrl.trim() || null,
        extra_roots: extraRoots
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean),
      });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Settings</h2>
        <label className="fm-field">
          <span>Tracking repo path (default: ~/.claude-skills-repo)</span>
          <input
            value={repoPath}
            onChange={(e) => setRepoPath(e.target.value)}
            placeholder="C:\Users\me\.claude-skills-repo"
          />
        </label>
        <label className="fm-field">
          <span>Tracking repo remote URL (for sharing between hosts)</span>
          <input
            value={remoteUrl}
            onChange={(e) => setRemoteUrl(e.target.value)}
            placeholder="git@github.com:me/my-skills.git"
          />
        </label>
        <label className="fm-field">
          <span>Extra skills roots to scan (one directory per line)</span>
          <textarea
            rows={4}
            value={extraRoots}
            onChange={(e) => setExtraRoots(e.target.value)}
            placeholder="D:\some\skills-folder"
          />
        </label>
        {error && <div className="modal-error">{error}</div>}
        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn accent" disabled={!loaded} onClick={() => void save()}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
