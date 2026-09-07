import { h, clear, icon, shortenPath } from '../dom.js';
import { encodeBase64 } from '../ws.js';
import { createTerminalController, terminalAssetsAvailable } from '../terminal.js';
import { statusInfo } from './sessions.js';
import {
  KEY_DEFINITIONS,
  STICKY_MODIFIERS,
  applyModifiers,
  encodeInput,
  REPEAT_DELAY_MS,
  REPEAT_INTERVAL_MS,
} from '../keys.js';

let controller = null;
let root = null;
let bodyHost = null;
let footHost = null;
let footSignature = null;
let stripText = null;
let attachTarget = null;
let attachFailure = null;
const drafts = new Map();
const modifiers = { ctrl: false, alt: false };

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
  stripText = null;
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
  if (!terminalAssetsAvailable()) {
    renderFoot(ctx, state, terminalId, 'unavailable');
    return;
  }
  ensureAttached(ctx, terminalId);
  controller.syncFromSnapshot(state);
  renderFoot(ctx, state, terminalId, footKind(state, terminalId));
}

function ensureRoot(ctx) {
  if (root) return;
  footHost = h('div', { class: 'terminal-foot' });
  if (terminalAssetsAvailable()) {
    bodyHost = terminalController(ctx).element;
  } else {
    bodyHost = h(
      'div',
      { class: 'terminal-body' },
      h(
        'div',
        { class: 'terminal-placeholder' },
        icon('warning', 'icon-lg'),
        h('p', {}, 'Terminal viewer assets are not vendored in this build'),
        h(
          'p',
          {},
          'Run script/vendor_remote_control_assets and rebuild Spirit to view terminal output here.',
        ),
      ),
    );
  }
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
  if (summary.agent && summary.agent.input_open) return 'agent';
  const mode = state.terminal.terminalId === terminalId ? state.terminal.mode || summary.mode : summary.mode;
  if (mode === 'prompt') return 'command';
  return 'keys';
}

function renderFoot(ctx, state, terminalId, kind) {
  const found = state.indexes.terminals.get(terminalId);
  const agent = found && found.terminal.agent ? found.terminal.agent : null;
  const signature = [terminalId, kind, agent ? 'agent' : 'plain'].join('|');
  if (signature !== footSignature) {
    footSignature = signature;
    clear(footHost);
    buildFoot(ctx, state, terminalId, kind, agent);
  }
  updateStrip(agent, found);
}

function updateStrip(agent, found) {
  if (!stripText) return;
  if (agent) {
    const info = statusInfo(agent.status);
    const detail = agent.tool_name
      ? [agent.tool_name, agent.tool_input_preview].filter(Boolean).join(': ')
      : agent.summary || agent.status_message || info.label;
    stripText.textContent = `${agent.display_name} · ${info.label}${detail ? ` · ${detail}` : ''}`;
    return;
  }
  const cwd = found && found.terminal.cwd ? shortenPath(found.terminal.cwd, 40) : '';
  stripText.textContent = cwd;
}

function buildFoot(ctx, state, terminalId, kind, agent) {
  stripText = null;
  if (kind === 'unavailable') {
    footHost.appendChild(
      h('p', { class: 'composer-readonly' }, 'Input is unavailable until the viewer assets are vendored.'),
    );
    return;
  }
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

  const strip = h('div', { class: 'agent-strip' });
  stripText = h('span', { class: 'agent-strip-text' });
  strip.appendChild(icon(agent ? 'robot' : 'folder', 'icon-sm'));
  strip.appendChild(stripText);
  strip.appendChild(
    h(
      'button',
      {
        class: 'icon-button',
        type: 'button',
        'aria-label': 'Show keyboard',
        onClick: () => controller && controller.focus(),
      },
      icon('keyboard', 'icon-sm'),
    ),
  );
  footHost.appendChild(strip);

  if (kind === 'read-only') {
    footHost.appendChild(h('p', { class: 'composer-readonly' }, 'This terminal is read only.'));
    return;
  }

  if (kind === 'keys') {
    footHost.appendChild(keyToolbar(ctx, terminalId));
    footHost.appendChild(rawLine(ctx, terminalId));
    return;
  }

  if (kind === 'agent') {
    footHost.appendChild(agentComposer(ctx, terminalId, agent));
    return;
  }

  footHost.appendChild(commandComposer(ctx, terminalId));
}

function sendKeys(ctx, terminalId, value) {
  if (!value) return;
  if (controller && controller.isAttached()) {
    controller.sendText(value);
    return;
  }
  ctx.command('terminal.input', { terminal_id: terminalId, bytes: encodeBase64(encodeInput(value)) });
}

function keyToolbar(ctx, terminalId) {
  const bar = h('div', { class: 'key-toolbar', role: 'toolbar', 'aria-label': 'Terminal keys' });
  const modifierButtons = new Map();

  function paintModifiers() {
    for (const [id, button] of modifierButtons) {
      button.setAttribute('aria-pressed', modifiers[id] ? 'true' : 'false');
    }
  }

  for (const modifier of STICKY_MODIFIERS) {
    const button = h(
      'button',
      {
        class: 'key-button',
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
    modifierButtons.set(modifier.id, button);
    bar.appendChild(button);
  }

  for (const key of KEY_DEFINITIONS) {
    const button = h(
      'button',
      { class: 'key-button', type: 'button', 'aria-label': key.description },
      key.label,
    );
    const fire = () => {
      sendKeys(ctx, terminalId, applyModifiers(key.bytes, modifiers));
      if (modifiers.ctrl || modifiers.alt) {
        modifiers.ctrl = false;
        modifiers.alt = false;
        paintModifiers();
      }
    };
    button.addEventListener('click', fire);
    if (key.repeatable) attachRepeat(button, fire);
    bar.appendChild(button);
  }
  paintModifiers();
  return bar;
}

function attachRepeat(button, fire) {
  let delayTimer = null;
  let repeatTimer = null;

  function stop() {
    if (delayTimer !== null) window.clearTimeout(delayTimer);
    if (repeatTimer !== null) window.clearInterval(repeatTimer);
    delayTimer = null;
    repeatTimer = null;
  }

  button.addEventListener('pointerdown', () => {
    stop();
    delayTimer = window.setTimeout(() => {
      repeatTimer = window.setInterval(fire, REPEAT_INTERVAL_MS);
    }, REPEAT_DELAY_MS);
  });
  button.addEventListener('pointerup', stop);
  button.addEventListener('pointercancel', stop);
  button.addEventListener('pointerleave', stop);
}

function autoGrow(textarea) {
  textarea.style.setProperty('height', 'auto');
  const max = 7.5 * 16;
  textarea.style.setProperty('height', `${Math.min(textarea.scrollHeight, max)}px`);
}

function coarsePointer() {
  return window.matchMedia && window.matchMedia('(pointer: coarse)').matches;
}

function draftKey(terminalId, kind) {
  return `${terminalId}|${kind}`;
}

function composerShell(textarea, sendButton) {
  return h('div', { class: 'composer' }, textarea, sendButton);
}

function commandComposer(ctx, terminalId) {
  const key = draftKey(terminalId, 'command');
  const textarea = h('textarea', {
    class: 'composer-input',
    rows: 1,
    placeholder: 'Run a command',
    autocapitalize: 'none',
    autocorrect: 'off',
    spellcheck: false,
    'aria-label': 'Command to run',
    value: drafts.get(key) || '',
  });
  const send = h(
    'button',
    { class: 'composer-send', type: 'button', 'aria-label': 'Run command' },
    icon('play'),
  );

  async function submit() {
    const value = textarea.value.trim();
    if (!value) return;
    send.disabled = true;
    try {
      await ctx.command('terminal.run_command', { terminal_id: terminalId, text: value });
      textarea.value = '';
      drafts.delete(key);
      autoGrow(textarea);
    } finally {
      send.disabled = false;
    }
  }

  textarea.addEventListener('input', () => {
    drafts.set(key, textarea.value);
    autoGrow(textarea);
  });
  textarea.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' || event.shiftKey) return;
    event.preventDefault();
    submit();
  });
  send.addEventListener('click', submit);
  autoGrow(textarea);
  return h(
    'div',
    {},
    composerShell(textarea, send),
    h('p', { class: 'composer-hint' }, 'Runs at the shell prompt on the desktop.'),
  );
}

function agentComposer(ctx, terminalId, agent) {
  const key = draftKey(terminalId, 'agent');
  const textarea = h('textarea', {
    class: 'composer-input',
    rows: 2,
    placeholder: `Message ${agent ? agent.display_name : 'the agent'}`,
    'aria-label': 'Message for the agent',
    value: drafts.get(key) || '',
  });
  const send = h(
    'button',
    { class: 'composer-send', type: 'button', 'aria-label': 'Send to agent' },
    icon('send'),
  );

  async function submit() {
    const value = textarea.value;
    if (!value.trim()) return;
    send.disabled = true;
    try {
      await ctx.command('terminal.agent_submit', { terminal_id: terminalId, text: value });
      textarea.value = '';
      drafts.delete(key);
      autoGrow(textarea);
    } finally {
      send.disabled = false;
    }
  }

  textarea.addEventListener('input', () => {
    drafts.set(key, textarea.value);
    autoGrow(textarea);
  });
  textarea.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' || event.shiftKey || coarsePointer()) return;
    event.preventDefault();
    submit();
  });
  send.addEventListener('click', submit);
  autoGrow(textarea);
  return h(
    'div',
    {},
    composerShell(textarea, send),
    h(
      'p',
      { class: 'composer-hint' },
      coarsePointer()
        ? 'Enter adds a newline. Tap send to submit. Prefixes such as / and @ work as they do on the desktop.'
        : 'Enter sends, Shift+Enter adds a newline. Prefixes such as / and @ work as they do on the desktop.',
    ),
  );
}

function rawLine(ctx, terminalId) {
  const key = draftKey(terminalId, 'raw');
  const input = h('input', {
    class: 'composer-input',
    type: 'text',
    placeholder: 'Send text to the running program',
    autocapitalize: 'none',
    autocorrect: 'off',
    spellcheck: false,
    'aria-label': 'Raw input for the running program',
    value: drafts.get(key) || '',
  });
  const send = h(
    'button',
    { class: 'composer-send', type: 'button', 'aria-label': 'Send text' },
    icon('send'),
  );

  function submit() {
    const value = input.value;
    if (!value) return;
    sendKeys(ctx, terminalId, `${value}\r`);
    input.value = '';
    drafts.delete(key);
  }

  input.addEventListener('input', () => drafts.set(key, input.value));
  input.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    submit();
  });
  send.addEventListener('click', submit);
  return h('div', { class: 'composer' }, input, send);
}
