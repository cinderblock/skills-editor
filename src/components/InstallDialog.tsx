import { useEffect, useMemo, useState } from "react";
import { installSkill } from "../api";
import {
  CATALOG_SOURCES,
  fetchSkillMd,
  listSkills,
  type CatalogEntry,
} from "../catalog";
import { readFields, splitFrontmatter } from "../frontmatter";
import type { SkillGroup } from "../types";

export default function InstallDialog({
  groups,
  onClose,
  onInstalled,
}: {
  groups: SkillGroup[];
  onClose: () => void;
  onInstalled: (msg: string) => void;
}) {
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<CatalogEntry | null>(null);
  const [previewText, setPreviewText] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [busyPath, setBusyPath] = useState<string | null>(null);

  const installedNames = useMemo(() => {
    const user = groups.find((g) => g.kind === "user");
    return new Set(
      (user?.skills ?? []).map((s) => s.dir.split(/[\\/]/).pop() ?? ""),
    );
  }, [groups]);

  useEffect(() => {
    void (async () => {
      const found: CatalogEntry[] = [];
      const errors: string[] = [];
      for (const source of CATALOG_SOURCES) {
        for (const root of source.skillRoots) {
          try {
            found.push(...(await listSkills(source, root)));
          } catch (e) {
            errors.push(String(e));
          }
        }
      }
      setEntries(found);
      if (errors.length && found.length === 0) setError(errors.join("; "));
      setLoading(false);
    })();
  }, []);

  useEffect(() => {
    if (!preview) return;
    setPreviewText("loading…");
    void fetchSkillMd(preview)
      .then(setPreviewText)
      .catch((e) => setPreviewText(`Could not load SKILL.md: ${e}`));
  }, [preview]);

  const previewFields = useMemo(() => {
    if (!previewText || previewText.startsWith("loading")) return null;
    const { frontmatter } = splitFrontmatter(previewText);
    return readFields(frontmatter);
  }, [previewText]);

  const install = async (entry: CatalogEntry) => {
    setBusyPath(entry.path);
    try {
      const msg = await installSkill(
        entry.source.cloneUrl,
        entry.path,
        entry.name,
        overwrite,
      );
      onInstalled(msg);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyPath(null);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <h2>Install skills</h2>
        <p className="modal-hint">
          Installs into your user skills (<code>~/.claude/skills</code>) via a
          shallow git clone.
        </p>
        {loading && <div className="modal-loading">Loading catalog…</div>}
        {error && <div className="modal-error">{error}</div>}
        <div className="install-columns">
          <div className="install-list">
            {CATALOG_SOURCES.map((source) => {
              const sourceEntries = entries.filter((e) => e.source === source);
              if (!loading && sourceEntries.length === 0) return null;
              return (
                <div key={source.repo}>
                  <div className="install-source">
                    <strong>{source.label}</strong>
                    <span className="install-note">{source.note}</span>
                  </div>
                  {sourceEntries.map((entry) => {
                    const installed = installedNames.has(entry.name);
                    return (
                      <div
                        key={entry.source.repo + entry.path}
                        className={`install-row${preview === entry ? " selected" : ""}`}
                      >
                        <button
                          className="install-name"
                          onClick={() => setPreview(entry)}
                        >
                          {entry.name}
                          {installed && <span className="badge installed">installed</span>}
                        </button>
                        <button
                          className="btn small accent"
                          disabled={busyPath !== null || (installed && !overwrite)}
                          onClick={() => void install(entry)}
                        >
                          {busyPath === entry.path ? "Installing…" : "Install"}
                        </button>
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
          <div className="install-preview">
            {preview ? (
              <>
                <div className="preview-title">
                  {previewFields?.name || preview.name}
                </div>
                {previewFields?.description && (
                  <div className="preview-desc">{previewFields.description}</div>
                )}
                <pre className="preview-body">{previewText}</pre>
              </>
            ) : (
              <div className="preview-empty">Select a skill to preview its SKILL.md</div>
            )}
          </div>
        </div>
        <div className="modal-actions space-between">
          <label className="check-label">
            <input
              type="checkbox"
              checked={overwrite}
              onChange={(e) => setOverwrite(e.target.checked)}
            />
            Overwrite if already installed
          </label>
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
