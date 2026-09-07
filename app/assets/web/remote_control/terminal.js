import { h, clear } from './dom.js';
import { decodeBase64 } from './ws.js';

const MIN_FONT_SIZE = 8;
const MAX_FONT_SIZE = 20;
const HARD_MAX_FONT_SIZE = 28;
const SCROLLBACK = 5000;
const MAX_INPUT_CHUNK = 4096;
const MEASURE_SAMPLE = 'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM';

const ANSI_TOKENS = [
  ['black', '--ansi-black'],
  ['red', '--ansi-red'],
  ['green', '--ansi-green'],
  ['yellow', '--ansi-yellow'],
  ['blue', '--ansi-blue'],
  ['magenta', '--ansi-magenta'],
  ['cyan', '--ansi-cyan'],
  ['white', '--ansi-white'],
  ['brightBlack', '--ansi-bright-black'],
  ['brightRed', '--ansi-bright-red'],
  ['brightGreen', '--ansi-bright-green'],
  ['brightYellow', '--ansi-bright-yellow'],
  ['brightBlue', '--ansi-bright-blue'],
  ['brightMagenta', '--ansi-bright-magenta'],
  ['brightCyan', '--ansi-bright-cyan'],
  ['brightWhite', '--ansi-bright-white'],
];

const FALLBACK_THEME = {
  background: '#0b0e13',
  foreground: '#dfe6ee',
  cursor: '#6ea8fe',
  cursorAccent: '#0b0e13',
  selectionBackground: 'rgba(110, 168, 254, 0.32)',
  black: '#1c2128',
  red: '#f2777a',
  green: '#6cc58c',
  yellow: '#e8b45c',
  blue: '#6ea8fe',
  magenta: '#c792ea',
  cyan: '#7fd3e8',
  white: '#cdd6e0',
  brightBlack: '#5b6673',
  brightRed: '#ff9295',
  brightGreen: '#8fe0ab',
  brightYellow: '#ffd08a',
  brightBlue: '#9cc4ff',
  brightMagenta: '#e2b4ff',
  brightCyan: '#a6e6f6',
  brightWhite: '#f2f6fa',
};

export function readTerminalTheme() {
  const styles = window.getComputedStyle(document.documentElement);
  const token = (name, fallback) => {
    const value = styles.getPropertyValue(name).trim();
    return value || fallback;
  };
  const theme = {
    background: token('--terminal-bg', FALLBACK_THEME.background),
    foreground: token('--terminal-fg', FALLBACK_THEME.foreground),
    cursor: token('--terminal-cursor', FALLBACK_THEME.cursor),
    cursorAccent: token('--terminal-bg', FALLBACK_THEME.cursorAccent),
    selectionBackground: token('--terminal-selection', FALLBACK_THEME.selectionBackground),
  };
  for (const [key, name] of ANSI_TOKENS) theme[key] = token(name, FALLBACK_THEME[key]);
  return theme;
}

function monoFontFamily() {
  const styles = window.getComputedStyle(document.documentElement);
  return styles.getPropertyValue('--font-mono').trim() || 'monospace';
}

export function terminalAssetsAvailable() {
  return typeof window.Terminal === 'function';
}

function logFailure(error) {
  if (window.console && typeof window.console.warn === 'function') {
    window.console.warn('remote control terminal', error);
  }
}

export function createTerminalController(ctx) {
  const host = h('div', { class: 'terminal-host' });
  const measurer = h('span', {
    class: 'visually-hidden',
    'aria-hidden': 'true',
    style: { 'white-space': 'pre', position: 'absolute', visibility: 'hidden' },
  });
  const body = h('div', { class: 'terminal-body' }, host, measurer);

  let term = null;
  let attachId = null;
  let terminalId = null;
  let unbindBinary = null;
  let readOnly = false;
  let closed = false;
  let attaching = null;
  let disposed = false;
  let pendingChunks = [];
  let syncedVersion = null;
  let flushHandle = null;
  let resizeObserver = null;
  let fontFamily = monoFontFamily();
  const cellWidths = new Map();

  function terminalIsReadOnly(id) {
    const found = ctx.store.indexes().terminals.get(id);
    return Boolean(found && found.terminal.read_only);
  }

  function measureCellWidth(fontSize) {
    const cached = cellWidths.get(fontSize);
    if (cached) return cached;
    measurer.style.setProperty('font-family', fontFamily);
    measurer.style.setProperty('font-size', `${fontSize}px`);
    measurer.textContent = MEASURE_SAMPLE;
    const width = measurer.getBoundingClientRect().width / MEASURE_SAMPLE.length;
    const safe = width > 0 ? width : fontSize * 0.6;
    cellWidths.set(fontSize, safe);
    return safe;
  }

  function fittedFontSize(cols, available) {
    if (!cols || available <= 0) return MAX_FONT_SIZE;
    for (let size = MAX_FONT_SIZE; size > MIN_FONT_SIZE; size -= 1) {
      if (cols * measureCellWidth(size) <= available) return size;
    }
    return MIN_FONT_SIZE;
  }

  function scaledFontSize(base) {
    const scale = (ctx.store.get().prefs || {}).fontScale || 1;
    const scaled = Math.round(base * scale);
    return Math.min(HARD_MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, scaled));
  }

  function ensureTerminal() {
    if (term || !terminalAssetsAvailable()) return term;
    term = new window.Terminal({
      scrollback: SCROLLBACK,
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      macOptionIsMeta: true,
      fontFamily,
      fontSize: 13,
      disableStdin: true,
      theme: readTerminalTheme(),
    });
    term.open(host);
    if (typeof window.WebLinksAddon === 'function') {
      try {
        term.loadAddon(new window.WebLinksAddon());
      } catch (error) {
        logFailure(error);
      }
    } else if (window.WebLinksAddon && typeof window.WebLinksAddon.WebLinksAddon === 'function') {
      try {
        term.loadAddon(new window.WebLinksAddon.WebLinksAddon());
      } catch (error) {
        logFailure(error);
      }
    }
    term.onData(handleData);
    term.onBinary(handleBinaryString);
    host.addEventListener('paste', handlePaste, true);
    return term;
  }

  function handleData(value) {
    sendText(value);
  }

  function handleBinaryString(value) {
    const bytes = new Uint8Array(value.length);
    for (let index = 0; index < value.length; index += 1) bytes[index] = value.charCodeAt(index) & 0xff;
    sendBytes(bytes);
  }

  function handlePaste(event) {
    if (!terminalId) return;
    event.preventDefault();
    event.stopPropagation();
    const clipboard = event.clipboardData;
    const value = clipboard ? clipboard.getData('text') : '';
    if (value) pasteText(value);
  }

  function sendBytes(bytes) {
    if (attachId === null || readOnly || !bytes.length) return;
    for (let offset = 0; offset < bytes.length; offset += MAX_INPUT_CHUNK) {
      ctx.ws.sendBinary(attachId, bytes.subarray(offset, offset + MAX_INPUT_CHUNK));
    }
  }

  function sendText(value) {
    if (!value) return;
    sendBytes(new TextEncoder().encode(value));
  }

  function pasteText(value) {
    if (!terminalId) return Promise.resolve();
    return ctx.command('terminal.paste', { terminal_id: terminalId, text: value });
  }

  function queueBytes(bytes) {
    pendingChunks.push(bytes);
    if (flushHandle !== null) return;
    flushHandle = window.requestAnimationFrame(flushQueue);
  }

  function flushQueue() {
    flushHandle = null;
    if (!term || !pendingChunks.length) {
      pendingChunks = [];
      return;
    }
    let total = 0;
    for (const chunk of pendingChunks) total += chunk.length;
    const merged = new Uint8Array(total);
    let offset = 0;
    for (const chunk of pendingChunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    pendingChunks = [];
    term.write(merged);
  }

  function applySnapshot(encoded) {
    if (!term) return;
    pendingChunks = [];
    if (flushHandle !== null) {
      window.cancelAnimationFrame(flushHandle);
      flushHandle = null;
    }
    term.reset();
    if (encoded) term.write(decodeBase64(encoded));
  }

  function applySize(cols, rows) {
    if (!term || !cols || !rows) return;
    if (term.cols !== cols || term.rows !== rows) term.resize(cols, rows);
  }

  function refit() {
    if (!term) return;
    const cols = term.cols || 80;
    const available = body.clientWidth - 8;
    const size = scaledFontSize(fittedFontSize(cols, available));
    if (term.options.fontSize !== size) term.options.fontSize = size;
    const overflows = cols * measureCellWidth(size) > available;
    body.dataset.overflow = overflows ? 'true' : 'false';
  }

  function applyTheme() {
    const nextFamily = monoFontFamily();
    if (nextFamily !== fontFamily) {
      fontFamily = nextFamily;
      cellWidths.clear();
      if (term) term.options.fontFamily = fontFamily;
    }
    if (term) term.options.theme = readTerminalTheme();
    refit();
  }

  function startObserving() {
    if (resizeObserver || typeof window.ResizeObserver !== 'function') return;
    resizeObserver = new window.ResizeObserver(() => refit());
    resizeObserver.observe(body);
  }

  function stopObserving() {
    if (!resizeObserver) return;
    resizeObserver.disconnect();
    resizeObserver = null;
  }

  function releaseAttachment() {
    if (unbindBinary) {
      unbindBinary();
      unbindBinary = null;
    }
    attachId = null;
    terminalId = null;
    readOnly = false;
    syncedVersion = null;
  }

  async function attach(nextTerminalId) {
    if (disposed || !nextTerminalId) return null;
    if (terminalId === nextTerminalId && attachId !== null) return attachId;
    if (attaching) {
      try {
        await attaching;
      } catch (error) {
        logFailure(error);
      }
    }
    if (terminalId === nextTerminalId && attachId !== null) return attachId;
    if (attachId !== null) await detach();
    closed = false;
    if (!ensureTerminal()) return null;
    const request = ctx.command('terminal.attach', { terminal_id: nextTerminalId });
    attaching = request;
    let data;
    try {
      data = await request;
    } finally {
      attaching = null;
    }
    if (disposed) {
      ctx.ws.command('terminal.detach', { attach_id: data.attach_id }).catch(logFailure);
      return null;
    }
    attachId = data.attach_id;
    terminalId = nextTerminalId;
    syncedVersion = ctx.store.get().version;
    readOnly = terminalIsReadOnly(nextTerminalId);
    unbindBinary = ctx.ws.onBinary(attachId, queueBytes);
    applySize(data.cols, data.rows);
    applySnapshot(data.snapshot);
    term.options.disableStdin = readOnly;
    ctx.store.patchTerminal({
      attachId,
      terminalId,
      mode: data.mode,
      cols: data.cols,
      rows: data.rows,
      pendingResync: false,
      closed: false,
    });
    startObserving();
    refit();
    return attachId;
  }

  async function detach() {
    const previous = attachId;
    releaseAttachment();
    stopObserving();
    ctx.store.patchTerminal({
      attachId: null,
      terminalId: null,
      mode: null,
      cols: 0,
      rows: 0,
      pendingResync: false,
    });
    if (previous === null) return;
    try {
      await ctx.ws.command('terminal.detach', { attach_id: previous });
    } catch (error) {
      logFailure(error);
    }
  }

  function handleResync(event) {
    if (event.attach_id !== attachId || !term) return;
    applySize(event.cols, event.rows);
    applySnapshot(event.snapshot);
    ctx.store.patchTerminal({
      mode: event.mode,
      cols: event.cols,
      rows: event.rows,
      pendingResync: false,
    });
    refit();
  }

  function handleClosed(event) {
    if (event.attach_id !== attachId) return;
    releaseAttachment();
    stopObserving();
    closed = true;
    ctx.store.patchTerminal({
      attachId: null,
      terminalId: null,
      mode: null,
      cols: 0,
      rows: 0,
      closed: true,
    });
  }

  function syncFromSnapshot(state) {
    if (!terminalId || !term) return;
    if (state.version === syncedVersion) return;
    syncedVersion = state.version;
    const found = state.indexes.terminals.get(terminalId);
    if (!found) return;
    const summary = found.terminal;
    const current = state.terminal;
    readOnly = Boolean(summary.read_only);
    term.options.disableStdin = readOnly;
    if (current.mode !== summary.mode || current.cols !== summary.cols || current.rows !== summary.rows) {
      applySize(summary.cols, summary.rows);
      ctx.store.patchTerminal({ mode: summary.mode, cols: summary.cols, rows: summary.rows });
      refit();
    }
  }

  return {
    element: body,
    isSupported: terminalAssetsAvailable,
    isAttached() {
      return attachId !== null;
    },
    isClosed() {
      return closed;
    },
    attachedTerminalId() {
      return terminalId;
    },
    attach,
    detach,
    forgetAttachment() {
      releaseAttachment();
      stopObserving();
      ctx.store.patchTerminal({
        attachId: null,
        terminalId: null,
        mode: null,
        cols: 0,
        rows: 0,
        pendingResync: false,
      });
    },
    handleResync,
    handleClosed,
    syncFromSnapshot,
    applyTheme,
    refit,
    pasteText,
    sendText,
    sendBytes,
    selection() {
      return term ? term.getSelection() : '';
    },
    focus() {
      if (term) term.focus();
    },
    blur() {
      if (term) term.blur();
    },
    scrollToBottom() {
      if (term) term.scrollToBottom();
    },
    dispose() {
      disposed = true;
      releaseAttachment();
      stopObserving();
      host.removeEventListener('paste', handlePaste, true);
      if (flushHandle !== null) window.cancelAnimationFrame(flushHandle);
      flushHandle = null;
      pendingChunks = [];
      if (term) {
        term.dispose();
        term = null;
      }
      clear(host);
    },
  };
}
