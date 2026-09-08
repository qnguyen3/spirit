import { h, clear, icon } from '../dom.js';
import { encodeBase64 } from '../ws.js';
import { createTerminalController } from '../terminal.js';
import {
  KEY_PAD_LEFT_ROWS,
  KEY_PAD_RIGHT_IDS,
  STICKY_MODIFIERS,
  applyModifiers,
  encodeInput,
  keyDefinition,
  REPEAT_DELAY_MS,
  REPEAT_INTERVAL_MS,
} from '../keys.js';

let controller = null;
let root = null;
let bodyHost = null;
let footHost = null;
let footSignature = null;
let attachTarget = null;
let attachFailure = null;
const modifiers = { ctrl: false, alt: false };
let keysVisible = false;

export function terminalController(ctx) {
  if (!controller) controller = createTerminalController(ctx);
  return controller;
}

export function leave(ctx) {
  if (controller) {
    controller.detach();
    controller.dispose();
    controller = null;
  }
  root = null;
  bodyHost = null;
  footHost = null;
  footSignature = null;
  attachTarget = null;
  attachFailure = null;
}

export function handleServerEvent(message) {
  if (!controller) return;
  if (message.name === 'terminal.resync') controller.handleResync(message);
  else if (message.name === 'terminal.closed') controller.handleClosed(message);
}

export function applyTheme() {
  if (controller) controller.applyTheme();
}

export function detachForBackground() {
  if (!controller || !controller.isAttached()) return;
  attachTarget = null;
  controller.detach();
}

export function handleDisconnect() {
  if (!controller) return;
  attachTarget = null;
  attachFailure = null;
  controller.forgetAttachment();
}

export function render(host, ctx, state) {
  const terminalId = state.route.params.id;
  ensureRoot(ctx);
  if (host.firstChild !== root) {
    clear(host);
    host.appendChild(root);
  }
  ensureAttached(ctx, terminalId);
  controller.syncFromSnapshot(state);
  renderFoot(ctx, terminalId, footKind(state, terminalId));
}

function ensureRoot(ctx) {
  if (root) return;
  footHost = h('div', { class: 'terminal-foot' });
  bodyHost = terminalController(ctx).element;
  root = h('div', { class: 'terminal-screen' }, bodyHost, footHost);
}

function ensureAttached(ctx, terminalId) {
  if (!terminalId) return;
  const sameTarget = attachTarget === terminalId;
  if (sameTarget && (attachFailure || controller.isClosed() || controller.isAttached())) return;
  attachTarget = terminalId;
  attachFailure = null;
  controller.attach(terminalId).catch((error) => {
    attachFailure = error && error.message ? error.message : 'Could not attach to this terminal.';
    ctx.store.patchUi({ terminalError: attachFailure });
  });
}

function footKind(state, terminalId) {
  if (controller && controller.isClosed() && attachTarget === terminalId) return 'closed';
  const found = state.indexes.terminals.get(terminalId);
  if (!found) return 'gone';
  const summary = found.terminal;
  if (summary.read_only) return 'read-only';
  return 'keys';
}

function renderFoot(ctx, terminalId, kind) {
  const signature = `${terminalId}|${kind}`;
  if (signature === footSignature) return;
  footSignature = signature;
  clear(footHost);
  buildFoot(ctx, terminalId, kind);
}

function keyboardHasFocus() {
  return Boolean(controller) && controller.element.contains(document.activeElement);
}

function keepKeyboard(button) {
  button.addEventListener('pointerdown', (event) => event.preventDefault());
}

function buildFoot(ctx, terminalId, kind) {
  if (kind === 'closed' || kind === 'gone') {
    footHost.appendChild(
      h(
        'div',
        { class: 'composer' },
        h(
          'p',
          { class: 'composer-readonly' },
          kind === 'closed' ? 'This terminal was closed.' : 'This terminal is no longer available.',
        ),
        h(
          'button',
          { class: 'button', type: 'button', onClick: () => ctx.navigate('sessions', {}) },
          'Back to Sessions',
        ),
      ),
    );
    return;
  }

  if (kind === 'read-only') {
    footHost.appendChild(h('p', { class: 'composer-readonly' }, 'This terminal is read only.'));
    return;
  }

  const pad = keyPad(ctx, terminalId);
  pad.hidden = !keysVisible;
  const keysToggle = h(
    'button',
    {
      class: 'icon-button strip-toggle',
      type: 'button',
      'aria-label': 'Keys',
      title: 'Show or hide the key pad',
      'aria-pressed': keysVisible ? 'true' : 'false',
      onClick: () => {
        keysVisible = !keysVisible;
        pad.hidden = !keysVisible;
        keysToggle.setAttribute('aria-pressed', keysVisible ? 'true' : 'false');
      },
    },
    icon('keyboard', 'icon-sm'),
  );
  keepKeyboard(keysToggle);
  const scrollToggle = h('button', {
    class: 'button button-small mobile-terminal-control', type: 'button',
    'aria-label': 'Scroll terminal', 'aria-pressed': controller.touchScrollEnabled() ? 'true' : 'false',
    onClick: () => {
      const enabled = !controller.touchScrollEnabled();
      controller.setTouchScroll(enabled);
      scrollToggle.setAttribute('aria-pressed', String(enabled));
    },
  }, 'Scroll terminal');
  keepKeyboard(scrollToggle);
  const keyboardToggle = h('button', {
    class: 'button button-small mobile-terminal-control', type: 'button',
    'aria-label': 'Show keyboard', 'aria-pressed': 'false',
    onClick: () => controller.toggleKeyboard(),
  }, icon('keyboard', 'icon-sm'), 'Keyboard');
  keepKeyboard(keyboardToggle);
  controller.onKeyboardChange((visible) => {
    keyboardToggle.setAttribute('aria-pressed', String(visible));
    keyboardToggle.setAttribute('aria-label', visible ? 'Hide keyboard' : 'Show keyboard');
  });
  footHost.appendChild(h('div', { class: 'terminal-controls' }, keysToggle, scrollToggle, keyboardToggle));
  footHost.appendChild(pad);
}

function sendKeys(ctx, terminalId, value) {
  if (!value) return;
  if (controller && controller.isAttached()) {
    controller.sendText(value);
    return;
  }
  ctx.command('terminal.input', { terminal_id: terminalId, bytes: encodeBase64(encodeInput(value)) });
}

function keyPad(ctx, terminalId) {
  const modifierButtons = new Map();

  function paintModifiers() {
    for (const [id, button] of modifierButtons) {
      button.setAttribute('aria-pressed', modifiers[id] ? 'true' : 'false');
    }
  }

  function modifierButton(modifier) {
    const button = h(
      'button',
      {
        class: 'key-button key-modifier',
        type: 'button',
        'aria-pressed': modifiers[modifier.id] ? 'true' : 'false',
        'aria-label': modifier.description,
        onClick: () => {
          modifiers[modifier.id] = !modifiers[modifier.id];
          paintModifiers();
        },
      },
      modifier.label,
    );
    keepKeyboard(button);
    modifierButtons.set(modifier.id, button);
    return button;
  }

  function keyButton(key) {
    const button = h(
      'button',
      {
        class: ['key-button', key.icon ? 'key-icon' : null, key.id === 'enter' ? 'key-enter' : null],
        type: 'button',
        'aria-label': key.description,
        dataset: { key: key.id },
      },
      key.icon ? icon(key.icon, 'icon-sm') : null,
      key.glyph ? h('span', { class: 'key-glyph', 'aria-hidden': 'true' }, key.glyph) : null,
      key.label ? h('span', { class: 'key-label' }, key.label) : null,
    );
    const fire = () => {
      const refocus = keyboardHasFocus();
      sendKeys(ctx, terminalId, applyModifiers(key.bytes, modifiers));
      if (modifiers.ctrl || modifiers.alt) {
        modifiers.ctrl = false;
        modifiers.alt = false;
        paintModifiers();
      }
      if (refocus && controller) controller.focus();
    };
    attachPress(button, fire, key.repeatable === true);
    return button;
  }

  function buttonFor(id) {
    const modifier = STICKY_MODIFIERS.find((entry) => entry.id === id);
    if (modifier) return modifierButton(modifier);
    const key = keyDefinition(id);
    return key ? keyButton(key) : null;
  }

  const left = h('div', { class: 'key-cluster key-cluster-left' });
  for (const row of KEY_PAD_LEFT_ROWS) {
    for (const id of row) left.appendChild(buttonFor(id));
  }
  const right = h('div', { class: 'key-cluster key-dpad' });
  for (const id of KEY_PAD_RIGHT_IDS) right.appendChild(buttonFor(id));
  paintModifiers();
  return h('div', { class: 'key-pad', role: 'toolbar', 'aria-label': 'Terminal keys' }, left, right);
}

function attachPress(button, fire, repeatable) {
  let delayTimer = null;
  let repeatTimer = null;
  let repeated = false;

  function stop() {
    if (delayTimer !== null) window.clearTimeout(delayTimer);
    if (repeatTimer !== null) window.clearInterval(repeatTimer);
    delayTimer = null;
    repeatTimer = null;
  }

  button.addEventListener('pointerdown', (event) => {
    event.preventDefault();
    repeated = false;
    if (!repeatable) return;
    stop();
    delayTimer = window.setTimeout(() => {
      repeatTimer = window.setInterval(() => {
        repeated = true;
        fire();
      }, REPEAT_INTERVAL_MS);
    }, REPEAT_DELAY_MS);
  });
  button.addEventListener('click', () => {
    if (repeated) {
      repeated = false;
      return;
    }
    fire();
  });
  if (!repeatable) return;
  button.addEventListener('pointerup', stop);
  button.addEventListener('pointercancel', stop);
  button.addEventListener('pointerleave', stop);
}
