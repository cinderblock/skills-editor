import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import {
  aiListJobs,
  deleteSkill,
  discoverSkills,
  hooksOverview,
  instructionsDelete,
  instructionsOverview,
  memoryOverview,
  setSkillEnabled,
} from "./api";
import InstructionInfo from "./components/InstructionInfo";
import InstructionsSidebar from "./components/InstructionsSidebar";
import MemoryInfo from "./components/MemoryInfo";
import MemorySidebar from "./components/MemorySidebar";
import MemoryStoreView from "./components/MemoryStoreView";
import NewInstructionDialog from "./components/NewInstructionDialog";
import NewMemoryDialog from "./components/NewMemoryDialog";
import StartupContext from "./components/StartupContext";
import { findFile } from "./instructionsModel";
import { findEntry, findIndex } from "./memoryModel";
import AiTaskDialog, { type AiTarget } from "./components/AiTaskDialog";
import EditorPane, { type EditorPaneHandle } from "./components/EditorPane";
import HookEditor, { type HookEditorHandle } from "./components/HookEditor";
import HooksSidebar from "./components/HooksSidebar";
import InstallDialog from "./components/InstallDialog";
import JobsPanel from "./components/JobsPanel";
import NewSkillDialog from "./components/NewSkillDialog";
import SettingsDialog from "./components/SettingsDialog";
import Sidebar from "./components/Sidebar";
import SyncPanel from "./components/SyncPanel";
import UpdateBanner from "./components/UpdateBanner";
import type {
  HookGroup,
  HookSelection,
  HooksOverview,
  InstrFile,
  InstrGroup,
  InstrOverview,
  InstrSelection,
  JobInfo,
  MemoryOverview,
  MemorySelection,
  MemoryStore,
  OpenFile,
  Skill,
  SkillGroup,
} from "./types";
import { useUpdater } from "./updates";

interface Confirm {
  message: string;
  actions: { label: string; kind?: "accent" | "danger"; run: () => void }[];
}

type View = "skills" | "hooks" | "config" | "memory";

const VIEW_LABEL: Record<View, string> = {
  skills: "Skills",
  hooks: "Hooks",
  config: "Config",
  memory: "Memory",
};

/**
 * A stand-in skill so hook scripts and instruction files can use the file
 * editor (and AI selection edits, which run in the file's folder).
 */
function standInFile(
  path: string,
  editable: boolean,
  kind: "script" | "instruction" | "memory",
  group: { key: string; label: string; detail: string } | null,
  name?: string,
): OpenFile {
  const sep = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  const dir = path.slice(0, sep);
  const base = path.slice(sep + 1);
  const skill: Skill = {
    id: path,
    name: name ?? base,
    description: "",
    dir,
    skill_md: "",
    files: [base],
    single_file: true,
    editable,
    disabled: false,
  };
  return {
    path,
    skill,
    kind,
    group: {
      key: `${kind}:${group?.key ?? dir}`,
      kind: "extra",
      label: group?.label ?? (kind === "script" ? "hooks" : "instructions"),
      detail: group?.detail ?? dir,
      skills: [],
    },
  };
}

function instructionFile(group: InstrGroup, file: InstrFile): OpenFile {
  return standInFile(file.path, file.editable, "instruction", group, file.label);
}

export default function App() {
  const [view, setView] = useState<View>("skills");
  const [groups, setGroups] = useState<SkillGroup[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [hooks, setHooks] = useState<HooksOverview | null>(null);
  const [hooksError, setHooksError] = useState<string | null>(null);
  const [file, setFile] = useState<OpenFile | null>(null);
  const [hookSel, setHookSel] = useState<HookSelection | null>(null);
  const [instr, setInstr] = useState<InstrOverview | null>(null);
  const [instrError, setInstrError] = useState<string | null>(null);
  const [instrLoading, setInstrLoading] = useState(false);
  const [instrSel, setInstrSel] = useState<InstrSelection | null>(null);
  const [showNewInstr, setShowNewInstr] = useState(false);
  const [mem, setMem] = useState<MemoryOverview | null>(null);
  const [memError, setMemError] = useState<string | null>(null);
  const [memLoading, setMemLoading] = useState(false);
  const [memSel, setMemSel] = useState<MemorySelection | null>(null);
  const [newMemoryStore, setNewMemoryStore] = useState<MemoryStore | null>(null);
  /** Which editor owns the main pane. */
  const [main, setMain] = useState<"file" | "hook" | "startup" | "store">("file");
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState("");
  const [jobs, setJobs] = useState<JobInfo[]>([]);
  const [reloadToken, setReloadToken] = useState(0);
  const [showInstall, setShowInstall] = useState(false);
  const [showSync, setShowSync] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showNewSkill, setShowNewSkill] = useState(false);
  const [aiTarget, setAiTarget] = useState<AiTarget | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const editorRef = useRef<EditorPaneHandle>(null);
  const hookEditorRef = useRef<HookEditorHandle>(null);
  const statusTimer = useRef<number | undefined>(undefined);
  const runningIds = useRef<Set<number>>(new Set());
  const updater = useUpdater();

  const say = useCallback((msg: string) => {
    setStatus(msg);
    window.clearTimeout(statusTimer.current);
    statusTimer.current = window.setTimeout(() => setStatus(""), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const found = await discoverSkills();
      setGroups(found);
      setLoadError(null);
      // Keep the open file's skill object fresh (files list may have changed).
      setFile((current) => {
        if (!current || current.kind === "script") return current;
        for (const g of found) {
          const skill = g.skills.find((s) => s.id === current.skill.id);
          if (skill) return { path: current.path, skill, group: g };
        }
        return current;
      });
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);

  const refreshHooks = useCallback(async (): Promise<HooksOverview | null> => {
    try {
      const ov = await hooksOverview();
      setHooks(ov);
      setHooksError(null);
      return ov;
    } catch (e) {
      setHooksError(String(e));
      return null;
    }
  }, []);

  const instrRequest = useRef(0);
  const refreshInstr = useCallback(async (force: boolean): Promise<InstrOverview | null> => {
    // The scan can take seconds; only the newest request may update state.
    const id = ++instrRequest.current;
    setInstrLoading(true);
    try {
      const ov = await instructionsOverview(force);
      if (id === instrRequest.current) {
        setInstr(ov);
        setInstrError(null);
      }
      return ov;
    } catch (e) {
      if (id === instrRequest.current) setInstrError(String(e));
      return null;
    } finally {
      if (id === instrRequest.current) setInstrLoading(false);
    }
  }, []);

  const refreshMem = useCallback(async (): Promise<MemoryOverview | null> => {
    setMemLoading(true);
    try {
      const ov = await memoryOverview();
      setMem(ov);
      setMemError(null);
      return ov;
    } catch (e) {
      setMemError(String(e));
      return null;
    } finally {
      setMemLoading(false);
    }
  }, []);

  const refreshAll = useCallback(
    (force = false) => {
      void refresh();
      void refreshHooks();
      void refreshInstr(force);
      void refreshMem();
    },
    [refresh, refreshHooks, refreshInstr, refreshMem],
  );

  useEffect(() => {
    const onFocus = () => refreshAll(false);
    refreshAll(false);
    // Claude Code and other tools edit these files too — rescan on focus.
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshAll]);

  // Poll AI jobs; when one finishes, reload the editor and rescan.
  useEffect(() => {
    const tick = async () => {
      try {
        const list = await aiListJobs();
        setJobs(list);
        const nowRunning = new Set(list.filter((j) => j.status === "running").map((j) => j.id));
        const finished = [...runningIds.current].filter((id) => !nowRunning.has(id));
        if (finished.length > 0) {
          setReloadToken((t) => t + 1);
          refreshAll();
          const failed = list.filter(
            (j) => finished.includes(j.id) && j.status === "failed",
          );
          say(
            failed.length > 0
              ? `AI job${failed.length > 1 ? "s" : ""} failed — see the jobs panel`
              : "AI job finished — files updated",
          );
        }
        runningIds.current = nowRunning;
      } catch {
        // Backend not ready yet; ignore.
      }
    };
    void tick();
    const interval = window.setInterval(() => void tick(), 2000);
    return () => window.clearInterval(interval);
  }, [refreshAll, say]);

  /** Run `next` once unsaved work in whichever editor is active is dealt with. */
  const guard = useCallback(
    (next: () => void) => {
      const active = main === "hook" ? hookEditorRef.current : editorRef.current;
      const proceed = () => {
        setDirty(false);
        next();
      };
      if (!dirty || !active?.isDirty()) {
        next();
        return;
      }
      setConfirm({
        message: "You have unsaved changes. Save them first?",
        actions: [
          {
            label: "Save & continue",
            kind: "accent",
            run: () => {
              void active
                .save()
                .then(() => {
                  if (!active.isDirty()) proceed();
                })
                .catch(() => {});
            },
          },
          { label: "Discard changes", kind: "danger", run: proceed },
          { label: "Cancel", run: () => {} },
        ],
      });
    },
    [dirty, main],
  );

  const openFile = useCallback(
    (next: OpenFile) =>
      guard(() => {
        setFile(next);
        setMain("file");
      }),
    [guard],
  );

  const selectHook = useCallback(
    (sel: HookSelection) =>
      guard(() => {
        setHookSel(sel);
        setMain("hook");
      }),
    [guard],
  );

  const openScript = useCallback(
    (path: string, editable: boolean, group: HookGroup | null) =>
      openFile(standInFile(path, editable, "script", group)),
    [openFile],
  );

  const selectInstr = useCallback(
    (sel: InstrSelection) => {
      if (sel.kind === "startup") {
        guard(() => {
          setInstrSel(sel);
          setMain("startup");
        });
        return;
      }
      const hit = findFile(instr, sel.path);
      guard(() => {
        setInstrSel(sel);
        // Imported files aren't listed themselves; they still open (the
        // backend admits resolved imports).
        setFile(hit ? instructionFile(hit.group, hit.file) : standInFile(sel.path, true, "instruction", null));
        setMain("file");
      });
    },
    [guard, instr],
  );

  const openMemoryFile = useCallback(
    (path: string, store: MemoryStore | null, name?: string) =>
      guard(() => {
        setMemSel({ kind: "note", path });
        setFile(
          standInFile(
            path,
            true,
            "memory",
            store ? { key: store.key, label: store.label, detail: store.dir } : null,
            name,
          ),
        );
        setMain("file");
      }),
    [guard],
  );

  const selectMemory = useCallback(
    (sel: MemorySelection) => {
      if (sel.kind === "store") {
        guard(() => {
          setMemSel(sel);
          setMain("store");
        });
        return;
      }
      const hit = findEntry(mem?.stores ?? [], sel.path);
      openMemoryFile(sel.path, hit?.store ?? null, hit?.entry.name);
    },
    [guard, mem, openMemoryFile],
  );

  const deleteInstr = useCallback(
    (f: InstrFile) => {
      void instructionsDelete(f.path)
        .then(() => {
          say(`Deleted ${f.label}`);
          setDirty(false);
          setFile((cur) => (cur?.path === f.path ? null : cur));
          setInstrSel(null);
          return refreshInstr(false);
        })
        .catch((e) => say(String(e)));
    },
    [refreshInstr, say],
  );

  const requestDelete = useCallback(
    (skill: Skill) => {
      setConfirm({
        message: `Delete the skill "${skill.name}" and all its files?\n${skill.dir}`,
        actions: [
          {
            label: "Delete",
            kind: "danger",
            run: () => {
              void deleteSkill(skill.dir)
                .then(() => {
                  setFile((f) => (f?.skill.id === skill.id ? null : f));
                  say(`Deleted ${skill.name}`);
                  return refresh();
                })
                .catch((e) => say(String(e)));
            },
          },
          { label: "Cancel", run: () => {} },
        ],
      });
    },
    [refresh, say],
  );

  const toggleDisabled = useCallback(
    (skill: Skill) => {
      void setSkillEnabled(skill.dir, skill.disabled)
        .then((msg) => {
          say(msg);
          return refresh();
        })
        .catch((e) => say(String(e)));
    },
    [refresh, say],
  );

  const openCreated = useCallback(
    async (skillMdPath: string) => {
      await refresh();
      const found = await discoverSkills();
      for (const g of found) {
        const skill = g.skills.find((s) => s.skill_md === skillMdPath);
        if (skill) {
          openFile({ path: skill.skill_md, skill, group: g });
          break;
        }
      }
    },
    [refresh, openFile],
  );

  const startupGroup =
    instrSel?.kind === "startup" ? instr?.groups.find((g) => g.key === instrSel.group) ?? null : null;
  const openInstr = main === "file" && file?.kind === "instruction" ? findFile(instr, file.path) : null;
  const memStore = memSel?.kind === "store" ? mem?.stores.find((s) => s.key === memSel.store) ?? null : null;
  const openNote = main === "file" && file?.kind === "memory" ? findEntry(mem?.stores ?? [], file.path) : null;
  const openMemIndex =
    main === "file" && file?.kind === "memory" ? findIndex(mem?.stores ?? [], file.path) : null;
  const newInstrProject =
    instrSel?.kind === "startup"
      ? startupGroup?.project_dir ?? null
      : openInstr?.group.kind === "project"
        ? openInstr.group.project_dir
        : null;

  const running = jobs.filter((j) => j.status === "running").length;
  const unsafeToRestart =
    [
      dirty ? "you have unsaved changes" : null,
      running > 0 ? `${running} AI job${running > 1 ? "s are" : " is"} still running` : null,
    ]
      .filter(Boolean)
      .join(" and ") || null;

  return (
    <div className="app">
      <div className="topbar">
        <span className="app-title">Skills Editor</span>
        <div className="topbar-actions">
          <button className="btn" onClick={() => setShowNewSkill(true)}>
            New skill
          </button>
          <button className="btn" onClick={() => setShowInstall(true)}>
            Install
          </button>
          <button
            className="btn"
            onClick={() =>
              setAiTarget({ mode: "skills", file: file && (file.kind ?? "skill") === "skill" ? file : undefined })
            }
          >
            AI task
          </button>
          <button className="btn" onClick={() => setShowSync(true)}>
            Sync
          </button>
          <button className="btn" onClick={() => setShowSettings(true)}>
            Settings
          </button>
          <button className="btn" onClick={() => refreshAll(true)}>
            Refresh
          </button>
        </div>
        <span className="statusline">{status}</span>
      </div>

      <UpdateBanner updater={updater} unsafeToRestart={unsafeToRestart} />

      <div className="main">
        <div className="sidebar-shell">
          <div className="sidebar-tabs">
            {(Object.keys(VIEW_LABEL) as View[]).map((v) => (
              <button
                key={v}
                className={`sidebar-tab${view === v ? " active" : ""}`}
                onClick={() => setView(v)}
              >
                {VIEW_LABEL[v]}
              </button>
            ))}
          </div>
          {view === "skills" ? (
            loadError ? (
              <div className="sidebar">
                <div className="modal-error sidebar-error">{loadError}</div>
              </div>
            ) : (
              <Sidebar
                groups={groups}
                selectedPath={main === "file" ? (file?.path ?? null) : null}
                onOpen={openFile}
              />
            )
          ) : view === "memory" ? (
            <MemorySidebar
              overview={mem}
              error={memError}
              loading={memLoading}
              selection={main === "store" || (main === "file" && file?.kind === "memory") ? memSel : null}
              onSelect={selectMemory}
              onNew={setNewMemoryStore}
            />
          ) : view === "config" ? (
            <InstructionsSidebar
              overview={instr}
              error={instrError}
              loading={instrLoading}
              selection={
                main === "startup" ? instrSel : main === "file" && file ? { kind: "file", path: file.path } : null
              }
              onSelect={selectInstr}
              onNew={() => setShowNewInstr(true)}
            />
          ) : (
            <HooksSidebar
              overview={hooks}
              error={hooksError}
              selection={main === "hook" ? hookSel : null}
              selectedPath={main === "file" ? (file?.path ?? null) : null}
              onSelect={selectHook}
              onOpenScript={(group, s) => openScript(s.path, s.editable, group)}
              onNew={() => {
                const current =
                  hookSel?.kind === "handler" ? hookSel.file : null;
                selectHook({ kind: "new", file: current });
              }}
            />
          )}
        </div>
        {main === "hook" && hookSel ? (
          <HookEditor
            key={JSON.stringify(hookSel)}
            ref={hookEditorRef}
            overview={hooks}
            selection={hookSel}
            reload={refreshHooks}
            onSelect={(sel) => {
              setHookSel(sel);
              if (!sel) setMain("file");
            }}
            onStatus={say}
            onDirtyChange={setDirty}
            onOpenScript={openScript}
          />
        ) : main === "store" && memStore ? (
          <MemoryStoreView
            store={memStore}
            onOpenNote={(path) => selectMemory({ kind: "note", path })}
            onOpenIndex={(path) => openMemoryFile(path, memStore, "MEMORY.md")}
            onNew={setNewMemoryStore}
            onStatus={say}
            onChanged={() => void refreshMem()}
          />
        ) : main === "startup" && startupGroup ? (
          <StartupContext
            group={startupGroup}
            onOpenPath={(path) => selectInstr({ kind: "file", path })}
            onStatus={say}
            onChanged={() => void refreshInstr(false)}
          />
        ) : (
          <EditorPane
            ref={editorRef}
            file={main === "file" ? file : null}
            reloadToken={reloadToken}
            onStatus={say}
            onDirtyChange={setDirty}
            onDeleteSkill={requestDelete}
            onToggleDisabled={toggleDisabled}
            onAiSelection={(f, selection) =>
              setAiTarget({ mode: "selection", file: f, selection })
            }
            onSaved={(f) => {
              if (f.kind === "instruction") void refreshInstr(false);
              if (f.kind === "memory") void refreshMem();
            }}
            info={
              openInstr ? (
                <InstructionInfo
                  group={openInstr.group}
                  file={openInstr.file}
                  onOpenPath={(path) => selectInstr({ kind: "file", path })}
                  onDelete={deleteInstr}
                />
              ) : openNote ? (
                <MemoryInfo
                  store={openNote.store}
                  entry={openNote.entry}
                  onStatus={say}
                  onChanged={() => void refreshMem()}
                  onClosed={() => {
                    setDirty(false);
                    setFile(null);
                    setMemSel(null);
                  }}
                />
              ) : openMemIndex ? (
                <div className="instr-info">
                  <div className="instr-info-row">
                    <span>
                      The memory index for <strong>{openMemIndex.label}</strong>. Only its first 200 lines
                      (or 25 KB) load at session start.
                    </span>
                  </div>
                </div>
              ) : null
            }
          />
        )}
      </div>

      <JobsPanel jobs={jobs} onChanged={() => void aiListJobs().then(setJobs)} />

      {showInstall && (
        <InstallDialog
          groups={groups}
          onClose={() => setShowInstall(false)}
          onInstalled={(msg) => {
            say(msg);
            void refresh();
          }}
        />
      )}
      {showSync && <SyncPanel onClose={() => setShowSync(false)} onStatus={say} />}
      {showSettings && (
        <SettingsDialog
          updater={updater}
          onClose={() => setShowSettings(false)}
          onSaved={() => {
            say("Settings saved");
            refreshAll();
          }}
        />
      )}
      {newMemoryStore && mem && (
        <NewMemoryDialog
          overview={mem}
          store={newMemoryStore}
          onClose={() => setNewMemoryStore(null)}
          onCreated={(path) => {
            say("Note saved");
            void refreshMem().then((ov) => {
              const hit = findEntry(ov?.stores ?? [], path);
              openMemoryFile(path, hit?.store ?? null, hit?.entry.name);
            });
          }}
        />
      )}
      {showNewInstr && instr && (
        <NewInstructionDialog
          overview={instr}
          initialProject={newInstrProject}
          onClose={() => setShowNewInstr(false)}
          onCreated={(path, note) => {
            say(note ? `Created — ${note}` : "Created");
            void refreshInstr(false).then((ov) => {
              const hit = findFile(ov, path);
              guard(() => {
                setInstrSel({ kind: "file", path });
                setFile(hit ? instructionFile(hit.group, hit.file) : standInFile(path, true, "instruction", null));
                setMain("file");
              });
            });
          }}
        />
      )}
      {showNewSkill && (
        <NewSkillDialog
          groups={groups}
          onClose={() => setShowNewSkill(false)}
          onCreated={(path) => void openCreated(path)}
        />
      )}
      {aiTarget && (
        <AiTaskDialog
          target={aiTarget}
          groups={groups}
          onClose={() => setAiTarget(null)}
          onStarted={(count) =>
            say(`Started ${count} AI job${count > 1 ? "s" : ""}`)
          }
        />
      )}
      {confirm && (
        <div className="modal-backdrop" onClick={() => setConfirm(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <p className="confirm-message">{confirm.message}</p>
            <div className="modal-actions">
              {confirm.actions.map((a) => (
                <button
                  key={a.label}
                  className={`btn ${a.kind ?? ""}`}
                  onClick={() => {
                    setConfirm(null);
                    a.run();
                  }}
                >
                  {a.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
