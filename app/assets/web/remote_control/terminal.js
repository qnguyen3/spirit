import { h, icon } from './dom.js';
import { encodeBase64 } from './ws.js';

const MIRROR_CHANNEL_FLAG = 0x80000000;
const MIRROR_FRAME_VERSION = 1;
const MIRROR_FRAME_KEY = 1;
const MIRROR_FRAME_PATCH = 2;
const MIRROR_FRAME_STATE = 3;
const MIRROR_STATE_UNAVAILABLE = 1;
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
const KEY_CHARS = { enter: '\r', tab: '\t', escape: '\x1b', backspace: '\x7f' };
const decoder = new TextDecoder();

export function mirrorChannel(mirrorId) {
  return (MIRROR_CHANNEL_FLAG | mirrorId) >>> 0;
}

export function parseMirrorPayload(bytes) {
  if (bytes.length < 12 || bytes[0] !== MIRROR_FRAME_VERSION) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const kind = bytes[1];
  const seq = view.getUint32(2, true);
  if (kind === MIRROR_FRAME_STATE) {
    const length = view.getUint16(7, true);
    return { kind, seq, state: bytes[6], message: decoder.decode(bytes.subarray(9, 9 + length)) };
  }
  if (kind !== MIRROR_FRAME_KEY && kind !== MIRROR_FRAME_PATCH) return null;
  const width = view.getUint16(6, true);
  const height = view.getUint16(8, true);
  const count = view.getUint16(10, true);
  const rects = [];
  let offset = 12;
  for (let index = 0; index < count; index += 1) {
    if (offset + 12 > bytes.length) return null;
    const rect = {
      x: view.getUint16(offset, true),
      y: view.getUint16(offset + 2, true),
      width: view.getUint16(offset + 4, true),
      height: view.getUint16(offset + 6, true),
    };
    const length = view.getUint32(offset + 8, true);
    offset += 12;
    if (offset + length > bytes.length) return null;
    rect.png = bytes.subarray(offset, offset + length);
    offset += length;
    rects.push(rect);
  }
  return { kind, seq, width, height, rects };
}

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
  let attaching = null;
  let composing = false;
  let pointerDown = false;
  let hasFrame = false;
  let seenTerminal = false;
  let mirrorId = null;
  let unsubscribe = null;
  let drawQueue = Promise.resolve();
  let pausedTerminal = null;
  let touch = null;
  let swiped = false;
  let touchScroll = false;
  let keyboardChanged = () => {};
  const touchDevice = () => window.matchMedia('(any-pointer: coarse)').matches;
  keyboard.inputMode = touchDevice() ? 'none' : 'text';
  keyboard.addEventListener('focus', () => keyboardChanged(keyboard.inputMode !== 'none'));
  keyboard.addEventListener('blur', () => keyboardChanged(false));
  const resizeObserver = new ResizeObserver(() => refit());
  resizeObserver.observe(body);
  document.addEventListener('visibilitychange', handleVisibility);

  function showStatus(message, retry = false) {
    statusText.textContent = message;
    status.hidden = false;
    resume.hidden = !retry;
    canvas.classList.toggle('terminal-mirror-stale', hasFrame);
  }

  function current(epoch) {
    return !disposed && attached && epoch === generation;
  }

  function stopMirror() {
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    if (mirrorId === null) return;
    const id = mirrorId;
    mirrorId = null;
    ctx.ws.command('terminal.mirror_stop', { mirror_id: id }).catch(() => {});
  }

  function handlePayload(bytes, epoch) {
    if (!current(epoch)) return;
    const payload = parseMirrorPayload(bytes);
    if (!payload) return;
    if (payload.kind === MIRROR_FRAME_STATE) {
      if (payload.state === MIRROR_STATE_UNAVAILABLE) showStatus(payload.message || 'Waiting for the desktop…', true);
      else resume.hidden = true;
      return;
    }
    drawQueue = drawQueue.then(() => drawFrame(payload, epoch)).catch(() => {});
  }

  async function drawFrame(frame, epoch) {
    const bitmaps = await Promise.all(
      frame.rects.map((rect) => createImageBitmap(new Blob([rect.png], { type: 'image/png' }))),
    );
    if (!current(epoch)) {
      for (const bitmap of bitmaps) if (bitmap.close) bitmap.close();
      return;
    }
    if (canvas.width !== frame.width || canvas.height !== frame.height) {
      canvas.width = frame.width;
      canvas.height = frame.height;
    }
    const context = canvas.getContext('2d');
    frame.rects.forEach((rect, index) => {
      context.clearRect(rect.x, rect.y, rect.width, rect.height);
      context.drawImage(bitmaps[index], rect.x, rect.y);
      if (bitmaps[index].close) bitmaps[index].close();
    });
    if (!hasFrame || frame.kind === MIRROR_FRAME_KEY) {
      const background = context.getImageData(0, 0, 1, 1).data;
      body.style.backgroundColor = `rgb(${background[0]}, ${background[1]}, ${background[2]})`;
    }
    hasFrame = true;
    status.hidden = true;
    canvas.classList.remove('terminal-mirror-stale');
    refit();
  }

  async function attach(id) {
    if (disposed || !id) return;
    if (terminalId === id && attached) return;
    if (attaching && terminalId === id) return attaching;
    if (document.hidden) {
      terminalId = id;
      pausedTerminal = id;
      return;
    }
    stopMirror();
    const epoch = ++generation;
    terminalId = id;
    attached = false;
    closed = false;
    hasFrame = false;
    seenTerminal = false;
    pausedTerminal = null;
    canvas.getContext('2d').clearRect(0, 0, canvas.width, canvas.height);
    showStatus('Connecting to the desktop terminal…');
    const request = ctx.command('terminal.mirror', { terminal_id: id });
    attaching = request;
    try {
      const data = await request;
      if (disposed || epoch !== generation) {
        if (data && data.mirror_id) ctx.ws.command('terminal.mirror_stop', { mirror_id: data.mirror_id }).catch(() => {});
        return;
      }
      mirrorId = data.mirror_id;
      unsubscribe = ctx.ws.onBinary(mirrorChannel(mirrorId), (bytes) => handlePayload(bytes, epoch));
      attached = true;
      const summary = ctx.store.indexes().terminals.get(id)?.terminal;
      ctx.store.patchTerminal({ terminalId: id, mode: summary?.mode, closed: false });
      applyTheme();
    } catch (error) {
      if (epoch === generation) showStatus(error.message || 'Could not open this terminal.', true);
      throw error;
    } finally {
      if (epoch === generation) attaching = null;
    }
  }

  function detach() {
    ++generation;
    stopMirror();
    attached = false;
    attaching = null;
    pointerDown = false;
    terminalId = null;
    pausedTerminal = null;
    ctx.store.patchTerminal({ terminalId: null, attachId: null, mode: null });
    return Promise.resolve();
  }

  async function restart() {
    const id = terminalId;
    await detach();
    return attach(id);
  }

  function handleVisibility() {
    if (disposed) return;
    if (document.hidden) {
      if (!attached || terminalId === null) return;
      pausedTerminal = terminalId;
      ++generation;
      stopMirror();
      attached = false;
      showStatus('Paused while this page is in the background.');
      return;
    }
    const id = pausedTerminal;
    if (!id || terminalId !== id) return;
    pausedTerminal = null;
    attach(id).catch(() => {});
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
      if (Math.hypot(event.clientX - touch.x, event.clientY - touch.y) > 8) swiped = true;
      if (touchScroll && swiped && event.clientY !== touch.lastY) {
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
    if (!touchDevice() || keyboard.inputMode !== 'none') keyboard.focus({ preventScroll: true });
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
    interact({ kind: 'key', key, chars: printable ? event.key : KEY_CHARS[key] || '', modifiers: modifiers(event) });
  });
  keyboard.addEventListener('beforeinput', (event) => {
    if (composing || event.isComposing) return;
    const key = { deleteContentBackward: 'backspace', deleteContentForward: 'delete', insertLineBreak: 'enter', insertParagraph: 'enter' }[event.inputType];
    if (!key) return;
    event.preventDefault();
    interact({ kind: 'key', key, chars: KEY_CHARS[key] || '' });
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
    if (key) return interact({ kind: 'key', key, chars: KEY_CHARS[key] || '' });
    if (value.length === 1 && value.charCodeAt(0) > 0 && value.charCodeAt(0) < 27) {
      return interact({ kind: 'key', key: String.fromCharCode(value.charCodeAt(0) + 96), modifiers: { ctrl: true } });
    }
    if (value.startsWith('\x1b')) return sendBytes(new TextEncoder().encode(value));
    const submit = value.endsWith('\r');
    const text = submit ? value.slice(0, -1) : value;
    const typed = text ? interact({ kind: 'text', text }) : Promise.resolve();
    if (!submit) return typed;
    return typed.then(() => interact({ kind: 'key', key: 'enter', chars: '\r' }));
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
    const mobile = touchDevice() || window.matchMedia('(max-width: 1023px)').matches;
    const fitted = mobile ? Math.max(body.clientWidth, 800) : Math.min(body.clientWidth, heightFit);
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
        stopMirror();
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
    touchScrollEnabled: () => touchScroll,
    setTouchScroll(enabled) {
      touchScroll = enabled;
      body.classList.toggle('terminal-touch-scroll', enabled);
    },
    onKeyboardChange(callback) {
      keyboardChanged = callback;
      callback(document.activeElement === keyboard && keyboard.inputMode !== 'none');
    },
    toggleKeyboard() {
      if (document.activeElement === keyboard && keyboard.inputMode !== 'none') {
        keyboard.inputMode = 'none';
        keyboard.blur();
      } else {
        keyboard.blur();
        keyboard.inputMode = 'text';
        keyboard.focus({ preventScroll: true });
      }
    },
    focus() { keyboard.focus({ preventScroll: true }); },
    blur() { keyboard.blur(); },
    scrollToBottom() { sendText('\x1b[F'); },
    dispose() {
      detach();
      resizeObserver.disconnect();
      document.removeEventListener('visibilitychange', handleVisibility);
      disposed = true;
    },
  };
}
