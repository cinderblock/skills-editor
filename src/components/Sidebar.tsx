import { useMemo, useState } from "react";
import type { OpenFile, Skill, SkillGroup } from "../types";

interface TreeNode {
  name: string;
  path: string;
  children: TreeNode[];
  isDir: boolean;
}

/** Build a nested tree from forward-slash relative paths. */
function buildTree(files: string[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", children: [], isDir: true };
  for (const file of files) {
    const parts = file.split("/");
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const isDir = i < parts.length - 1;
      const path = parts.slice(0, i + 1).join("/");
      let child = node.children.find((c) => c.name === part && c.isDir === isDir);
      if (!child) {
        child = { name: part, path, children: [], isDir };
        node.children.push(child);
      }
      node = child;
    }
  }
  const sortRec = (nodes: TreeNode[]) => {
    nodes.sort((a, b) =>
      a.isDir !== b.isDir ? (a.isDir ? -1 : 1) : a.name.localeCompare(b.name),
    );
    nodes.forEach((n) => sortRec(n.children));
  };
  sortRec(root.children);
  return root.children;
}

const KIND_BADGE: Record<string, string> = {
  user: "user",
  project: "project",
  plugin: "plugin",
  extra: "extra",
};

function FileTree({
  nodes,
  skill,
  group,
  depth,
  selectedPath,
  onOpen,
}: {
  nodes: TreeNode[];
  skill: Skill;
  group: SkillGroup;
  depth: number;
  selectedPath: string | null;
  onOpen: (file: OpenFile) => void;
}) {
  return (
    <>
      {nodes.map((node) => (
        <FileNode
          key={node.path + (node.isDir ? "/" : "")}
          node={node}
          skill={skill}
          group={group}
          depth={depth}
          selectedPath={selectedPath}
          onOpen={onOpen}
        />
      ))}
    </>
  );
}

function FileNode({
  node,
  skill,
  group,
  depth,
  selectedPath,
  onOpen,
}: {
  node: TreeNode;
  skill: Skill;
  group: SkillGroup;
  depth: number;
  selectedPath: string | null;
  onOpen: (file: OpenFile) => void;
}) {
  const [open, setOpen] = useState(true);
  const abs = `${skill.dir}\\${node.path.replace(/\//g, "\\")}`;
  if (node.isDir) {
    return (
      <div>
        <button
          className="tree-row dir"
          style={{ paddingLeft: 12 + depth * 14 }}
          onClick={() => setOpen(!open)}
        >
          <span className="chevron">{open ? "▾" : "▸"}</span>
          <span className="tree-name">{node.name}/</span>
        </button>
        {open && (
          <FileTree
            nodes={node.children}
            skill={skill}
            group={group}
            depth={depth + 1}
            selectedPath={selectedPath}
            onOpen={onOpen}
          />
        )}
      </div>
    );
  }
  const isSelected = selectedPath === abs;
  return (
    <button
      className={`tree-row file${isSelected ? " selected" : ""}`}
      style={{ paddingLeft: 12 + depth * 14 }}
      onClick={() => onOpen({ path: abs, skill, group })}
    >
      <span className="tree-name">{node.name}</span>
    </button>
  );
}

function SkillNode({
  skill,
  group,
  selectedPath,
  onOpen,
}: {
  skill: Skill;
  group: SkillGroup;
  selectedPath: string | null;
  onOpen: (file: OpenFile) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const tree = useMemo(() => buildTree(skill.files), [skill.files]);
  const isSelected =
    selectedPath !== null && selectedPath.startsWith(skill.dir + "\\");
  return (
    <div className="skill-node">
      <div className={`skill-row${isSelected ? " selected" : ""}`}>
        {!skill.single_file ? (
          <button className="chevron-btn" onClick={() => setExpanded(!expanded)}>
            {expanded ? "▾" : "▸"}
          </button>
        ) : (
          <span className="chevron-spacer" />
        )}
        <button
          className="skill-main"
          onClick={() => onOpen({ path: skill.skill_md, skill, group })}
        >
          <span className="skill-name">
            {skill.name}
            {!skill.editable && <span className="badge readonly">read-only</span>}
          </span>
          {skill.description && (
            <span className="skill-desc">{skill.description}</span>
          )}
        </button>
      </div>
      {expanded && !skill.single_file && (
        <div className="skill-files">
          <FileTree
            nodes={tree}
            skill={skill}
            group={group}
            depth={1}
            selectedPath={selectedPath}
            onOpen={onOpen}
          />
        </div>
      )}
    </div>
  );
}

export default function Sidebar({
  groups,
  selectedPath,
  onOpen,
}: {
  groups: SkillGroup[];
  selectedPath: string | null;
  onOpen: (file: OpenFile) => void;
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  return (
    <div className="sidebar">
      {groups.map((group) => {
        const isCollapsed = collapsed[group.key] ?? false;
        return (
          <div className="group" key={group.key}>
            <button
              className="group-header"
              onClick={() =>
                setCollapsed({ ...collapsed, [group.key]: !isCollapsed })
              }
            >
              <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
              <span className="group-label">{group.label}</span>
              <span className={`badge kind-${group.kind}`}>
                {KIND_BADGE[group.kind] ?? group.kind}
              </span>
              <span className="group-count">{group.skills.length}</span>
            </button>
            {!isCollapsed && (
              <>
                <div className="group-path">{group.detail}</div>
                {group.skills.length === 0 && (
                  <div className="group-empty">no skills</div>
                )}
                {group.skills.map((skill) => (
                  <SkillNode
                    key={skill.id}
                    skill={skill}
                    group={group}
                    selectedPath={selectedPath}
                    onOpen={onOpen}
                  />
                ))}
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
