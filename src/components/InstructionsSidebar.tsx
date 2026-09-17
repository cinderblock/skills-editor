import { useMemo, useState } from "react";
import {
  SECTION_TITLE,
  fileBadges,
  filterGroups,
  formatBytes,
  roughTokens,
  sectionOf,
  startupTotal,
  type Section,
} from "../instructionsModel";
import type { InstrGroup, InstrOverview, InstrSelection } from "../types";

const KIND_BADGE: Record<InstrGroup["kind"], string> = {
  managed: "managed",
  user: "user",
  project: "project",
  parents: "parents",
};

const COLLAPSED_BY_DEFAULT = new Set<InstrGroup["kind"]>(["parents"]);
const SECTION_ORDER: Section[] = ["instructions", "rules", "nested", "other"];
/** Sections with many files start folded inside an open group. */
const FOLDED_SECTIONS = new Set<Section>(["nested"]);

function GroupBody({
  group,
  selection,
  onSelect,
  forceOpen,
}: {
  group: InstrGroup;
  selection: InstrSelection | null;
  onSelect: (sel: InstrSelection) => void;
  forceOpen: boolean;
}) {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const bySection = useMemo(() => {
    const m = new Map<Section, typeof group.files>();
    for (const f of group.files) {
      const s = sectionOf(f);
      m.set(s, [...(m.get(s) ?? []), f]);
    }
    return m;
  }, [group.files]);
  const total = startupTotal(group.startup);

  return (
    <>
      {group.startup.length > 0 && (
        <button
          className={`instr-row startup${selection?.kind === "startup" && selection.group === group.key ? " selected" : ""}`}
          onClick={() => onSelect({ kind: "startup", group: group.key })}
        >
          <span className="instr-name">Startup context</span>
          <span className="instr-meta">
            {group.startup.length} · {roughTokens(total)}
          </span>
        </button>
      )}
      {group.notes.map((n) => (
        <div key={n} className="group-note">
          {n}
        </div>
      ))}
      {SECTION_ORDER.map((section) => {
        const files = bySection.get(section);
        if (!files?.length) return null;
        const foldable = FOLDED_SECTIONS.has(section);
        const isOpen = forceOpen || !foldable || (open[section] ?? false);
        return (
          <div key={section}>
            {SECTION_TITLE[section] &&
              (foldable ? (
                <button
                  className="instr-section toggle"
                  onClick={() => setOpen({ ...open, [section]: !isOpen })}
                >
                  <span className="chevron">{isOpen ? "▾" : "▸"}</span>
                  {SECTION_TITLE[section]}
                  <span className="instr-section-count">{files.length}</span>
                </button>
              ) : (
                <div className="instr-section">{SECTION_TITLE[section]}</div>
              ))}
            {isOpen &&
              files.map((f) => (
                <button
                  key={f.path}
                  className={`instr-row${selection?.kind === "file" && selection.path === f.path ? " selected" : ""}${f.loads === "never" ? " dim" : ""}`}
                  onClick={() => onSelect({ kind: "file", path: f.path })}
                >
                  <span className="instr-name">{f.label}</span>
                  {fileBadges(f).map((b) => (
                    <span key={b.text} className={`badge ${b.tone}`}>
                      {b.text}
                    </span>
                  ))}
                  <span className="instr-meta">{formatBytes(f.bytes)}</span>
                </button>
              ))}
          </div>
        );
      })}
    </>
  );
}

export default function InstructionsSidebar({
  overview,
  error,
  loading,
  selection,
  onSelect,
  onNew,
}: {
  overview: InstrOverview | null;
  error: string | null;
  loading: boolean;
  selection: InstrSelection | null;
  onSelect: (sel: InstrSelection) => void;
  onNew: () => void;
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [query, setQuery] = useState("");
  const groups = useMemo(() => filterGroups(overview?.groups ?? [], query), [overview, query]);
  const filtering = query.trim().length > 0;

  return (
    <div className="sidebar">
      <div className="sidebar-actions">
        <input
          className="sidebar-filter"
          placeholder="Filter projects and files"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="btn small accent" onClick={onNew}>
          New file
        </button>
      </div>
      {error && <div className="modal-error sidebar-error">{error}</div>}
      {!overview && !error && <div className="group-empty">Scanning projects…</div>}
      {overview && loading && <div className="group-note">Rescanning…</div>}
      {filtering && groups.length === 0 && <div className="group-empty">Nothing matches.</div>}
      {groups.map((group) => {
        const isCollapsed = !filtering && (collapsed[group.key] ?? COLLAPSED_BY_DEFAULT.has(group.kind));
        const warn = group.files.some((f) => f.warnings.length > 0);
        return (
          <div className="group" key={group.key}>
            <button
              className="group-header"
              onClick={() => setCollapsed({ ...collapsed, [group.key]: !isCollapsed })}
            >
              <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
              <span className="group-label">{group.label}</span>
              <span className={`badge kind-${group.kind}`}>{KIND_BADGE[group.kind]}</span>
              {warn && <span className="badge warn">⚠</span>}
              <span className="group-count">{group.files.length}</span>
            </button>
            {!isCollapsed && (
              <>
                <div className="group-path">{group.detail}</div>
                <GroupBody group={group} selection={selection} onSelect={onSelect} forceOpen={filtering} />
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
