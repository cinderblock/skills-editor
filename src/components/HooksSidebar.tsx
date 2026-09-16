import { useState } from "react";
import { flatten, handlerSummary, parkedTitle } from "../hooksModel";
import type { HookGroup, HookSelection, HooksOverview, ScriptFile } from "../types";

const KIND_BADGE: Record<string, string> = {
  user: "user",
  project: "project",
  managed: "managed",
  plugin: "plugin",
  frontmatter: "frontmatter",
  orphaned: "orphaned",
};

/** Big, read-only groups start collapsed to keep the list short. */
const COLLAPSED_BY_DEFAULT = new Set(["plugin", "frontmatter", "orphaned"]);

function isSelected(sel: HookSelection | null, file: string, event: string, group: number, handler: number) {
  return (
    sel?.kind === "handler" &&
    sel.file === file &&
    sel.event === event &&
    sel.group === group &&
    sel.handler === handler
  );
}

function GroupBody({
  group,
  selection,
  selectedPath,
  onSelect,
  onOpenScript,
}: {
  group: HookGroup;
  selection: HookSelection | null;
  selectedPath: string | null;
  onSelect: (sel: HookSelection) => void;
  onOpenScript: (group: HookGroup, script: ScriptFile) => void;
}) {
  // Label rows with their file only when the hooks come from several files.
  const multiFile =
    group.sources.filter((s) => flatten(s.hooks).length > 0 || s.parked.length > 0).length > 1;
  const rows = group.sources.flatMap((src) => [
    ...flatten(src.hooks).map((h) => (
      <button
        key={`${src.file}|${h.event}|${h.group}|${h.handler}`}
        className={`hook-row${isSelected(selection, src.file, h.event, h.group, h.handler) ? " selected" : ""}${src.inactive_reason ? " inactive" : ""}`}
        onClick={() =>
          onSelect({ kind: "handler", file: src.file, event: h.event, group: h.group, handler: h.handler })
        }
      >
        <span className="hook-title">
          <span className="hook-event">{h.event}</span>
          {h.matcher && <span className="hook-matcher">{h.matcher}</span>}
          {multiFile && <span className="badge">{src.file_label}</span>}
          {src.inactive_reason && <span className="badge warn">inactive</span>}
        </span>
        <span className="hook-summary">{handlerSummary(h.handler_json)}</span>
      </button>
    )),
    ...src.parked.map((p) => (
      <button
        key={p.id}
        className={`hook-row parked${selection?.kind === "parked" && selection.id === p.id ? " selected" : ""}`}
        onClick={() => onSelect({ kind: "parked", id: p.id })}
      >
        <span className="hook-title">
          <span className="hook-event">{parkedTitle(p)}</span>
          {multiFile && <span className="badge">{src.file_label}</span>}
          <span className="badge disabled">disabled</span>
        </span>
        <span className="hook-summary">{handlerSummary(p.handler)}</span>
      </button>
    )),
  ]);
  const problems = group.sources.filter((s) => s.parse_error);

  return (
    <>
      {problems.map((s) => (
        <div key={s.file} className="group-problem">
          {s.file_label}: unreadable JSON — {s.parse_error}
        </div>
      ))}
      {rows.length === 0 && <div className="group-empty">no hooks</div>}
      {rows}
      {group.scripts.length > 0 && (
        <div className="hook-scripts">
          <div className="hook-scripts-title">scripts</div>
          {group.scripts.map((s) => (
            <button
              key={s.path}
              className={`tree-row file${selectedPath === s.path ? " selected" : ""}`}
              onClick={() => onOpenScript(group, s)}
            >
              <span className="tree-name">{s.name}</span>
              {!s.referenced && <span className="badge">unused</span>}
              {!s.editable && <span className="badge readonly">read-only</span>}
            </button>
          ))}
        </div>
      )}
    </>
  );
}

export default function HooksSidebar({
  overview,
  error,
  selection,
  selectedPath,
  onSelect,
  onOpenScript,
  onNew,
}: {
  overview: HooksOverview | null;
  error: string | null;
  selection: HookSelection | null;
  selectedPath: string | null;
  onSelect: (sel: HookSelection) => void;
  onOpenScript: (group: HookGroup, script: ScriptFile) => void;
  onNew: () => void;
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  return (
    <div className="sidebar">
      <div className="sidebar-actions">
        <button className="btn small accent" onClick={onNew}>
          New hook
        </button>
      </div>
      {error && <div className="modal-error sidebar-error">{error}</div>}
      {!overview && !error && <div className="group-empty">Loading hooks…</div>}
      {overview?.groups.map((group) => {
        const isCollapsed = collapsed[group.key] ?? COLLAPSED_BY_DEFAULT.has(group.kind);
        const count =
          group.sources.reduce((n, s) => n + flatten(s.hooks).length + s.parked.length, 0);
        const withHooks = group.sources.filter((s) => flatten(s.hooks).length > 0);
        const allOff = group.sources.some((s) => s.disable_all_hooks);
        // e.g. a disabled plugin: nothing in the group would run.
        const inactive = !allOff && withHooks.length > 0 && withHooks.every((s) => s.inactive_reason);
        return (
          <div className="group" key={group.key}>
            <button
              className="group-header"
              onClick={() => setCollapsed({ ...collapsed, [group.key]: !isCollapsed })}
            >
              <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
              <span className="group-label">{group.label}</span>
              <span className={`badge kind-${group.kind}`}>{KIND_BADGE[group.kind] ?? group.kind}</span>
              {allOff && <span className="badge disabled">all off</span>}
              {inactive && group.kind !== "frontmatter" && <span className="badge warn">inactive</span>}
              <span className="group-count">{count}</span>
            </button>
            {!isCollapsed && (
              <>
                <div className="group-path">{group.detail}</div>
                <GroupBody
                  group={group}
                  selection={selection}
                  selectedPath={selectedPath}
                  onSelect={onSelect}
                  onOpenScript={onOpenScript}
                />
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
