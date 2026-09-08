import { createStore, initialState, sessionsRanked } from './store.js';
import { connect } from './ws.js';
import { createRouter } from './router.js';
import { mountLayout } from './ui/layout.js';
import * as workspacesScreen from './ui/workspaces.js';
import * as workspaceScreen from './ui/workspace.js';
import * as sessionsScreen from './ui/sessions.js';
import * as terminalScreen from './ui/terminal_view.js';
import * as settingsScreen from './ui/settings.js';

const PROTOCOL_VERSION = 1;
const TOAST_MS = 4000;
const HIDDEN_DETACH_MS = 30000;
const DEFAULT_TITLE = 'Spirit Remote Control';

const SCREENS = {
  workspaces: workspacesScreen,
  workspace: workspaceScreen,
  sessions: sessionsScreen,
  terminal: terminalScreen,
  settings: settingsScreen,
};

const store = createStore(initialState());
let ws = null;
let router = null;
let layout = null;
let ctx = null;
let activeScreenName = null;
let renderHandle = null;
let hiddenTimer = null;
let toastSeed = 1;
let audioContext = null;
let previousBlocked = new Set();

function viewState() {
  const state = store.get();
  return { ...state, indexes: store.indexes() };
}

function scheduleRender() {
  if (renderHandle !== null) return;
  renderHandle = window.requestAnimationFrame(() => {
    renderHandle = null;
    renderNow();
  });
}

function renderNow() {
  const state = viewState();
  layout.render(state);
  const name = SCREENS[state.route.name] ? state.route.name : 'workspaces';
  if (activeScreenName && activeScreenName !== name) {
    const previous = SCREENS[activeScreenName];
    if (previous && previous.leave) previous.leave(ctx);
  }
  activeScreenName = name;
  SCREENS[name].render(layout.contentHost, ctx, state);
}

function applyTheme(prefs) {
  const theme = prefs.theme || 'system';
  if (theme === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

function toast(message, kind) {
  const id = `toast-${toastSeed++}`;
  const entry = { id, message, kind: kind || 'info' };
  store.patchUi({ toasts: store.get().ui.toasts.concat([entry]) });
  if (kind !== 'error') {
    window.setTimeout(() => {
      const remaining = store.get().ui.toasts.filter((item) => item.id !== id);
      store.patchUi({ toasts: remaining });
    }, TOAST_MS);
  }
}

function openSheet(kind, props) {
  store.patchUi({ sheet: { kind, props: props || {} } });
  router.pushSheet();
}

function closeSheet() {
  const sheet = store.get().ui.sheet;
  if (!sheet) return;
  store.patchUi({ sheet: null });
  if (sheet.props && typeof sheet.props.onDismiss === 'function') sheet.props.onDismiss();
  if (router.hasSheet()) router.popSheet();
}

function confirmSheet(props) {
  return new Promise((resolve) => {
    openSheet('confirm', {
      ...props,
      onConfirm: () => resolve(true),
      onCancel: () => resolve(false),
      onDismiss: () => resolve(false),
    });
  });
}

function describeError(error) {
  if (!error) return 'Something went wrong.';
  if (error.message) return error.message;
  if (error.code) return error.code;
  return 'Something went wrong.';
}

async function command(name, params, options) {
  try {
    return await ws.command(name, params, options);
  } catch (error) {
    toast(describeError(error), 'error');
    throw error;
  }
}

function navigate(name, params) {
  router.navigate(name, params || {});
}

function beep() {
  try {
    const Constructor = window.AudioContext || window.webkitAudioContext;
    if (!Constructor) return;
    if (!audioContext) audioContext = new Constructor();
    const oscillator = audioContext.createOscillator();
    const gain = audioContext.createGain();
    oscillator.type = 'sine';
    oscillator.frequency.value = 880;
    gain.gain.value = 0.0001;
    gain.gain.exponentialRampToValueAtTime(0.12, audioContext.currentTime + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, audioContext.currentTime + 0.35);
    oscillator.connect(gain);
    gain.connect(audioContext.destination);
    oscillator.start();
    oscillator.stop(audioContext.currentTime + 0.36);
  } catch (error) {
    audioContext = null;
  }
}

function updateTitleBadge(count) {
  document.title = count > 0 ? `(${count}) ${DEFAULT_TITLE}` : DEFAULT_TITLE;
}

function alertForBlocked(entry) {
  const prefs = store.get().prefs;
  if (prefs.vibrate && navigator.vibrate) navigator.vibrate([30, 60, 30]);
  if (prefs.sound) beep();
  store.patchUi({
    needsInputAlert: {
      terminalId: entry.terminal_id,
      title: entry.session.title || entry.session.display_name,
    },
  });
}

function reviewNeedsInput(state) {
  const blocked = new Set();
  let newest = null;
  for (const entry of sessionsRanked(state)) {
    if (entry.session.status !== 'blocked') continue;
    blocked.add(entry.terminal_id);
    if (!previousBlocked.has(entry.terminal_id)) newest = entry;
  }
  const count = blocked.size;
  updateTitleBadge(count);
  if (newest) alertForBlocked(newest);
  if (count === 0 && store.get().ui.needsInputAlert) store.patchUi({ needsInputAlert: null });
  previousBlocked = blocked;
}

function onHello(message) {
  const previousInstance = store.get().connection.instanceId;
  const restarted = Boolean(previousInstance) && previousInstance !== message.instance_id;
  store.patch({
    hello: message,
    connection: {
      ...store.get().connection,
      status: 'open',
      instanceId: message.instance_id,
      clientId: message.client_id,
      lastError: null,
    },
  });
  store.patchUi({ protocolMismatch: message.protocol !== PROTOCOL_VERSION });
  if (restarted) {
    terminalScreen.handleDisconnect();
    store.patch({ snapshot: null, version: 0 });
    store.patchUi({ history: [], historyState: 'idle', devices: [], devicesState: 'idle', clone: null });
    previousBlocked = new Set();
    toast('Spirit restarted', 'info');
    navigate('workspaces', {});
  }
}

function onState(message) {
  store.patch({ snapshot: message.snapshot, version: message.version });
  reviewNeedsInput(viewState());
}

function onEvent(message) {
  switch (message.name) {
    case 'terminal.resync':
    case 'terminal.closed':
      terminalScreen.handleServerEvent(message);
      break;
    case 'session.status':
      if (message.status === 'blocked') {
        alertForBlocked({
          terminal_id: message.terminal_id,
          session: { title: message.title, display_name: message.agent },
        });
      }
      break;
    case 'project.clone_progress':
      store.patchUi({
        clone: {
          job_id: message.job_id,
          phase: message.phase,
          percent: message.percent,
          message: message.message,
        },
      });
      if (message.phase === 'failed') toast(message.message || 'Clone failed', 'error');
      break;
    case 'server.shutting_down':
      toast('Spirit turned Remote Control off.', 'error');
      ws.stop('server shutting down');
      break;
    default:
      break;
  }
}

function onStatus(update) {
  const previous = store.get().connection.status;
  store.patchConnection({
    status: update.status,
    reconnectAt: update.reconnectAt || null,
    lastError: update.status === 'error' ? 'connection error' : store.get().connection.lastError,
  });
  if (previous === 'open') terminalScreen.handleDisconnect();
}

function onRoute(route, options) {
  if (options && options.closedSheet) store.patchUi({ sheet: null });
  const patch = {};
  if (route.name === 'terminal' && route.params.id) patch.lastTerminalId = route.params.id;
  if (route.name === 'workspace' && route.params.id) patch.lastProjectId = route.params.id;
  if (Object.keys(patch).length) store.patchUi(patch);
  store.patch({ route });
}

function handleVisibility() {
  if (hiddenTimer !== null) {
    window.clearTimeout(hiddenTimer);
    hiddenTimer = null;
  }
  if (document.hidden) {
    hiddenTimer = window.setTimeout(() => {
      hiddenTimer = null;
      terminalScreen.detachForBackground();
    }, HIDDEN_DETACH_MS);
    return;
  }
  scheduleRender();
}

function handleGlobalKeys(event) {
  if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== 'k') return;
  event.preventDefault();
  const state = viewState();
  const items = [
    { label: 'Workspaces', icon: 'workspaces', onSelect: () => navigate('workspaces', {}) },
    { label: 'Sessions', icon: 'sessions', onSelect: () => navigate('sessions', {}) },
    { label: 'Home', icon: 'home', onSelect: () => navigate('workspace', { id: 'home' }) },
    { separator: true },
  ];
  for (const project of (state.snapshot && state.snapshot.projects) || []) {
    items.push({
      label: project.name,
      icon: 'folder',
      onSelect: () => navigate('workspace', { id: project.id }),
    });
  }
  items.push({ separator: true });
  items.push({ label: 'Settings', icon: 'settings', onSelect: () => navigate('settings', {}) });
  openSheet('menu', { title: 'Go to', items });
}

async function loadIconSprite() {
  const target = document.getElementById('icon-sprite');
  if (!target) return;
  try {
    const response = await window.fetch(new URL('icons.svg', import.meta.url).href, {
      credentials: 'same-origin',
    });
    if (!response.ok) return;
    target.innerHTML = await response.text();
  } catch (error) {
    target.textContent = '';
  }
}

function watchColorScheme() {
  if (!window.matchMedia) return;
  const query = window.matchMedia('(prefers-color-scheme: dark)');
  const onChange = () => terminalScreen.applyTheme();
  if (query.addEventListener) query.addEventListener('change', onChange);
  else if (query.addListener) query.addListener(onChange);
}

function start() {
  const root = document.getElementById('app');
  applyTheme(store.get().prefs);

  ws = connect({ onHello, onState, onEvent, onStatus });
  router = createRouter(onRoute);
  ctx = {
    store,
    ws,
    router,
    command,
    toast,
    openSheet,
    closeSheet,
    confirm: confirmSheet,
    navigate,
  };
  layout = mountLayout(root, ctx);

  let previousPrefs = store.get().prefs;
  store.subscribe((state) => {
    if (state.prefs !== previousPrefs) {
      if (state.prefs.theme !== previousPrefs.theme) applyTheme(state.prefs);
      previousPrefs = state.prefs;
      terminalScreen.applyTheme();
    }
    scheduleRender();
  });

  document.addEventListener('visibilitychange', handleVisibility);
  window.addEventListener('keydown', handleGlobalKeys);
  watchColorScheme();

  router.start();
  ws.start();
  renderNow();
  loadIconSprite().then(scheduleRender);
}

start();
