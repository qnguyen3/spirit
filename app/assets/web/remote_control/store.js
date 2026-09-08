export function createStore(initial) {
  let state = initial;
  const listeners = new Set();
  let indexes = buildIndexes(state.snapshot);

  return {
    get() {
      return state;
    },
    indexes() {
      return indexes;
    },
    patch(partial) {
      const next = { ...state, ...partial };
      if (partial && Object.prototype.hasOwnProperty.call(partial, 'snapshot')) {
        indexes = buildIndexes(next.snapshot);
      }
      state = next;
      for (const listener of Array.from(listeners)) listener(state);
    },
    patchUi(partial) {
      this.patch({ ui: { ...state.ui, ...partial } });
    },
    patchTerminal(partial) {
      this.patch({ terminal: { ...state.terminal, ...partial } });
    },
    patchConnection(partial) {
      this.patch({ connection: { ...state.connection, ...partial } });
    },
    patchPrefs(partial) {
      const prefs = { ...state.prefs, ...partial };
      savePrefs(prefs);
      this.patch({ prefs });
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

function buildIndexes(snapshot) {
  const screens = new Map();
  const tabs = new Map();
  const panes = new Map();
  const terminals = new Map();
  const projects = new Map();
  const worktrees = new Map();
  if (!snapshot) return { screens, tabs, panes, terminals, projects, worktrees };

  for (const window of snapshot.windows || []) {
    for (const screen of window.screens || []) {
      screens.set(screen.id, { screen, window });
      for (const section of screen.sections || []) {
        for (const tab of section.tabs || []) {
          tabs.set(tab.id, { tab, section, screen, window });
          for (const pane of tab.panes || []) {
            panes.set(pane.id, { pane, tab, section, screen, window });
            if (pane.terminal) {
              terminals.set(pane.terminal.terminal_id, {
                terminal: pane.terminal,
                pane,
                tab,
                section,
                screen,
                window,
              });
            }
          }
        }
      }
    }
  }
  for (const project of snapshot.projects || []) {
    projects.set(project.id, project);
    for (const worktree of project.worktrees || []) {
      worktrees.set(worktree.id, { worktree, project });
    }
  }
  return { screens, tabs, panes, terminals, projects, worktrees };
}

export function activeWindow(state) {
  const snapshot = state.snapshot;
  if (!snapshot || !snapshot.windows || snapshot.windows.length === 0) return null;
  return (
    snapshot.windows.find((window) => window.id === snapshot.active_window_id) || snapshot.windows[0]
  );
}

export function screenForProject(state, projectId) {
  const snapshot = state.snapshot;
  if (!snapshot) return null;
  for (const window of snapshot.windows || []) {
    for (const screen of window.screens || []) {
      if (projectId === 'home' ? !screen.project_id : screen.project_id === projectId) {
        return { screen, window };
      }
    }
  }
  return null;
}

export function sessionsRanked(state) {
  const sessions = (state.snapshot && state.snapshot.sessions) || [];
  return sessions.slice().sort((left, right) => {
    if (left.rank !== right.rank) return left.rank - right.rank;
    return left.workspace_name.localeCompare(right.workspace_name);
  });
}

export function needsInputCount(state) {
  const sessions = (state.snapshot && state.snapshot.sessions) || [];
  return sessions.filter((entry) => entry.session.status === 'blocked').length;
}

export function terminalTitle(state, terminalId) {
  const found = state.snapshot ? state.indexes.terminals.get(terminalId) : null;
  if (!found) return 'Terminal';
  return found.terminal.title || found.tab.title || 'Terminal';
}

const PREFS_KEY = 'spirit.remote.prefs';

export function loadPrefs() {
  const defaults = { fontScale: 1, sound: false, vibrate: true, theme: 'system', richInput: false };
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    return raw ? { ...defaults, ...JSON.parse(raw) } : defaults;
  } catch (error) {
    return defaults;
  }
}

function savePrefs(prefs) {
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch (error) {
    return;
  }
}

export function initialState() {
  return {
    connection: { status: 'connecting', instanceId: null, clientId: null, lastError: null, reconnectAt: null },
    hello: null,
    snapshot: null,
    version: 0,
    route: { name: 'workspaces', params: {} },
    terminal: { attachId: null, terminalId: null, mode: null, cols: 0, rows: 0, pendingResync: false },
    ui: { sheet: null, toasts: [], pending: {}, history: [], historyState: 'idle', devices: [] },
    prefs: loadPrefs(),
  };
}
