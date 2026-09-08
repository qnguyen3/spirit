import { h, icon } from './dom.js';
import { encodeBase64 } from './ws.js';

const FRAME_DELAY_MS = 100;
const RETRY_DELAY_MS = 1000;
const KEY_NAMES = {
  Enter: 'enter', Escape: 'escape', Backspace: 'backspace', Delete: 'delete',
  Tab: 'tab', ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right',
  Home: 'home', End: 'end', PageUp: 'pageup', PageDown: 'pagedown', Insert: 'insert',
};
const CONTROL_KEYS = {
  '\r': 'enter', '\t': 'tab', '\x1b': 'escape', '\x7f': 'backspace',
  '\x1b[A': 'up', '\x1b[B': 'down', '\x1b[C': 'right', '\x1b[D': 'left',
  '\x1b[H': 'home', '\x1b[F': 'end', '\x1b[5~': 'pageup', '\x1b[6~': 'pagedown',
};

export function createTerminalController(ctx) {
  const canvas = h('canvas', { class: 'terminal-mirror', role: 'img', 'aria-label': 'Live desktop terminal' });
  const keyboard = h('textarea', {
    class: 'terminal-keyboard', 'aria-label': 'Type in the desktop terminal',
    autocapitalize: 'none', autocorrect: 'off', spellcheck: false,
  });
  const statusText = h('p', { role: 'status' }, 'Connecting to the desktop terminal…');
  const resume = h('button', {
    class: 'button', type: 'button', hidden: true,
    onClick: () => restart().catch(() => {}),
  }, icon('refresh'), 'Show terminal again');
  const status = h('div', { class: 'terminal-mirror-status' }, statusText, resume);
  const body = h('div', { class: 'terminal-body terminal-mirror-body' }, canvas, keyboard, status);
  let terminalId = null;
  let generation = 0;
  let attached = false;
  let disposed = false;
  let closed = false;
  let timer = null;
  let attaching = null;
  let composing = false;
  let pointerDown = false;
  let hasFrame = false;
  let seenTerminal = false;
  let previousFrame = null;
  let touch = null;
  let swiped = false;
  const resizeObserver = new ResizeObserver(() => refit());
  resizeObserver.observe(body);

  function clearTimer() {
    if (timer !== null) window.clearTimeout(timer);
    timer = null;
  }

  function showStatus(message, retry = false) {
    statusText.textContent = message;
    status.hidden = false;
    resume.hidden = !retry;
    canvas.classList.toggle('terminal-mirror-stale', hasFrame);
  }

  function current(epoch) {
    return !disposed && attached && epoch === generation;
  }

  async function frame(epoch) {
    if (!current(epoch)) return;
    if (document.hidden) {
      timer = window.setTimeout(() => frame(epoch), RETRY_DELAY_MS);
      return;
    }
    let delay = FRAME_DELAY_MS;
    try {
      const data = await ctx.ws.command('terminal.frame', { terminal_id: terminalId, previous_frame: previousFrame }, { timeoutMs: 5000 });
      if (!current(epoch)) return;
      if (data.image) {
        const image = new Image();
        image.src = `data:image/png;base64,${data.image}`;
        await image.decode();
        if (!current(epoch)) return;
        if (canvas.width !== data.width || canvas.height !== data.height) {
          canvas.width = data.width;
          canvas.height = data.height;
        }
        canvas.getContext('2d').drawImage(image, 0, 0);
        previousFrame = data.fingerprint || null;
        hasFrame = true;
        const background = canvas.getContext('2d').getImageData(0, 0, 1, 1).data;
        body.style.backgroundColor = `rgb(${background[0]}, ${background[1]}, ${background[2]})`;
        refit();
      }
      status.hidden = true;
      canvas.classList.remove('terminal-mirror-stale');
    } catch (error) {
      if (!current(epoch)) return;
      if (error.code !== 'rate_limited') showStatus(error.message || 'Waiting for the desktop…', true);
      delay = error.code === 'rate_limited' ? FRAME_DELAY_MS : RETRY_DELAY_MS;
    }
    if (current(epoch)) timer = window.setTimeout(() => frame(epoch), delay);
  }

  async function attach(id) {
    if (disposed || !id) return;
    if (terminalId === id && attached) return;
    if (attaching && terminalId === id) return attaching;
    clearTimer();
    const epoch = ++generation;
    terminalId = id;
    attached = false;
    closed = false;
    hasFrame = false;
    seenTerminal = false;
    previousFrame = null;
    canvas.getContext('2d').clearRect(0, 0, canvas.width, canvas.height);
    showStatus('Connecting to the desktop terminal…');
    const request = ctx.command('terminal.mirror', { terminal_id: id });
    attaching = request;
    try {
      await request;
      if (disposed || epoch !== generation) return;
      attached = true;
      const summary = ctx.store.indexes().terminals.get(id)?.terminal;
      ctx.store.patchTerminal({ terminalId: id, mode: summary?.mode, closed: false });
      applyTheme();
      frame(epoch);
    } catch (error) {
      if (epoch === generation) showStatus(error.message || 'Could not open this terminal.', true);
      throw error;
    } finally {
      if (epoch === generation) attaching = null;
    }
  }

  function detach() {
    ++generation;
    clearTimer();
    attached = false;
    attaching = null;
    pointerDown = false;
    terminalId = null;
    ctx.store.patchTerminal({ terminalId: null, attachId: null, mode: null });
    return Promise.resolve();
  }

  async function restart() {
    const id = terminalId;
    await detach();
    return attach(id);
  }

  function interact(interaction) {
    if (!attached || !hasFrame || !status.hidden) return Promise.resolve();
    const id = terminalId;
    const epoch = generation;
    return ctx.ws.command('terminal.interact', { terminal_id: id, ...interaction }).catch((error) => {
      if (current(epoch)) showStatus(error.message || 'Could not send input to the desktop.', true);
    });
  }

  function modifiers(event) {
    return { ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey, cmd: event.metaKey };
  }

  function position(event) {
    const rect = canvas.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)),
      y: Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)),
    };
  }

  canvas.addEventListener('pointerdown', (event) => {
    if (event.pointerType === 'touch') {
      touch = { x: event.clientX, y: event.clientY, lastY: event.clientY };
      swiped = false;
      return;
    }
    if (event.button !== 0) return;
    event.preventDefault();
    keyboard.focus({ preventScroll: true });
    canvas.setPointerCapture(event.pointerId);
    pointerDown = true;
    interact({ kind: 'pointer', phase: 'down', ...position(event), modifiers: modifiers(event) });
  });
  canvas.addEventListener('pointermove', (event) => {
    if (event.pointerType === 'touch' && touch) {
      if (Math.abs(event.clientY - touch.y) > 8 && Math.abs(event.clientY - touch.y) > Math.abs(event.clientX - touch.x)) {
        swiped = true;
        interact({ kind: 'scroll', ...position(event), delta_x: 0, delta_y: touch.lastY - event.clientY });
      }
      touch.lastY = event.clientY;
      return;
    }
    if (pointerDown) interact({ kind: 'pointer', phase: 'drag', ...position(event), modifiers: modifiers(event) });
  });
  function pointerUp(event) {
    touch = null;
    if (!pointerDown) return;
    pointerDown = false;
    interact({ kind: 'pointer', phase: 'up', ...position(event), modifiers: modifiers(event) });
  }
  canvas.addEventListener('pointerup', pointerUp);
  canvas.addEventListener('pointercancel', pointerUp);
  canvas.addEventListener('click', (event) => {
    if (event.pointerType !== 'touch' || swiped) return;
    keyboard.focus({ preventScroll: true });
    interact({ kind: 'pointer', phase: 'down', ...position(event), modifiers: {} });
    interact({ kind: 'pointer', phase: 'up', ...position(event), modifiers: {} });
  });
  canvas.addEventListener('wheel', (event) => {
    if (event.ctrlKey || event.metaKey || event.shiftKey) return;
    event.preventDefault();
    const unit = event.deltaMode === 1 ? 20 : event.deltaMode === 2 ? canvas.clientHeight : 1;
    interact({ kind: 'scroll', ...position(event), delta_x: event.deltaX * unit, delta_y: event.deltaY * unit });
  }, { passive: false });

  keyboard.addEventListener('keydown', (event) => {
    if (event.isComposing || composing || ['Shift', 'Control', 'Alt', 'Meta', 'Dead', 'Process'].includes(event.key)) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') return;
    const printable = Array.from(event.key).length === 1;
    const key = KEY_NAMES[event.key] || (printable ? event.key.toLowerCase() : /^F\d+$/.test(event.key) ? event.key.toLowerCase() : null);
    if (!key) return;
    event.preventDefault();
    event.stopPropagation();
    interact({ kind: 'key', key, chars: printable ? event.key : '', modifiers: modifiers(event) });
  });
  keyboard.addEventListener('compositionstart', () => { composing = true; });
  keyboard.addEventListener('compositionend', () => {
    composing = false;
    if (keyboard.value) interact({ kind: 'text', text: keyboard.value });
    keyboard.value = '';
  });
  keyboard.addEventListener('input', () => {
    if (composing) return;
    if (keyboard.value) interact({ kind: 'text', text: keyboard.value });
    keyboard.value = '';
  });
  keyboard.addEventListener('paste', (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData('text');
    if (text) pasteText(text).catch(() => {});
  });

  function sendText(value) {
    const key = CONTROL_KEYS[value];
    if (key) return interact({ kind: 'key', key });
    if (value.length === 1 && value.charCodeAt(0) > 0 && value.charCodeAt(0) < 27) {
      return interact({ kind: 'key', key: String.fromCharCode(value.charCodeAt(0) + 96), modifiers: { ctrl: true } });
    }
    return sendBytes(new TextEncoder().encode(value));
  }

  function sendBytes(bytes) {
    if (!attached) return;
    return ctx.command('terminal.input', { terminal_id: terminalId, bytes: encodeBase64(bytes) }).catch(() => {});
  }

  function pasteText(text) {
    if (!attached) return Promise.resolve();
    return ctx.command('terminal.paste', { terminal_id: terminalId, text });
  }

  function refit() {
    if (!hasFrame || !body.clientWidth || !body.clientHeight) return;
    const scale = Math.min(3, Math.max(0.5, ctx.store.get().prefs.fontScale || 1));
    const heightFit = body.clientHeight * canvas.width / canvas.height;
    const mobile = window.matchMedia('(max-width: 767px)').matches;
    const fitted = mobile ? Math.max(body.clientWidth, heightFit) : Math.min(body.clientWidth, heightFit);
    canvas.style.width = `${Math.round(fitted * scale)}px`;
  }

  function applyTheme() { refit(); }

  return {
    element: body,
    isAttached: () => attached,
    isClosed: () => closed,
    attachedTerminalId: () => terminalId,
    attach, detach, sendText, sendBytes, pasteText, applyTheme,
    forgetAttachment() {
      detach();
      showStatus('Reconnecting to the desktop…');
    },
    handleResync() {},
    handleClosed() {},
    syncFromSnapshot(state) {
      if (!terminalId || !attached) return;
      const summary = state.indexes.terminals.get(terminalId)?.terminal;
      if (!summary && seenTerminal) {
        closed = true;
        attached = false;
        ++generation;
        clearTimer();
        showStatus('This terminal was closed.');
      } else if (summary) {
        seenTerminal = true;
        if (state.terminal.mode !== summary.mode) ctx.store.patchTerminal({ mode: summary.mode });
      }
    },
    refit,
    async selection() {
      if (!terminalId) return '';
      const result = await ctx.command('terminal.selection', { terminal_id: terminalId });
      return result.text || '';
    },
    focus() { keyboard.focus({ preventScroll: true }); },
    blur() { keyboard.blur(); },
    scrollToBottom() { sendText('\x1b[F'); },
    dispose() { detach(); resizeObserver.disconnect(); disposed = true; },
  };
}
