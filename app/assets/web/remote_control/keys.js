export const ESC = '\x1b';

export const STICKY_MODIFIERS = [
  { id: 'ctrl', label: 'Ctrl', description: 'Control modifier for the next key' },
  { id: 'alt', label: 'Alt', description: 'Alt or Meta modifier for the next key' },
];

export const KEY_DEFINITIONS = [
  { id: 'esc', label: 'Esc', bytes: ESC, description: 'Escape' },
  { id: 'tab', label: 'Tab', bytes: '\x09', description: 'Tab' },
  { id: 'enter', label: 'Enter', glyph: '⏎', bytes: '\r', description: 'Enter' },
  { id: 'up', icon: 'arrow-up', bytes: `${ESC}[A`, description: 'Arrow up', repeatable: true },
  { id: 'down', icon: 'arrow-down', bytes: `${ESC}[B`, description: 'Arrow down', repeatable: true },
  { id: 'left', icon: 'arrow-left', bytes: `${ESC}[D`, description: 'Arrow left', repeatable: true },
  { id: 'right', icon: 'arrow-right', bytes: `${ESC}[C`, description: 'Arrow right', repeatable: true },
  { id: 'home', label: 'Home', bytes: `${ESC}[H`, description: 'Home' },
  { id: 'end', label: 'End', bytes: `${ESC}[F`, description: 'End' },
  { id: 'pgup', label: 'PgUp', bytes: `${ESC}[5~`, description: 'Page up', repeatable: true },
  { id: 'pgdn', label: 'PgDn', bytes: `${ESC}[6~`, description: 'Page down', repeatable: true },
  { id: 'ctrl-c', label: '^C', bytes: '\x03', description: 'Control C, interrupt' },
  { id: 'ctrl-d', label: '^D', bytes: '\x04', description: 'Control D, end of input' },
  { id: 'ctrl-z', label: '^Z', bytes: '\x1a', description: 'Control Z, suspend' },
  { id: 'ctrl-l', label: '^L', bytes: '\x0c', description: 'Control L, clear screen' },
  { id: 'slash', label: '/', bytes: '/', description: 'Slash' },
  { id: 'dash', label: '-', bytes: '-', description: 'Hyphen' },
  { id: 'pipe', label: '|', bytes: '|', description: 'Pipe' },
  { id: 'tilde', label: '~', bytes: '~', description: 'Tilde' },
];

export const KEY_PAD_LEFT_ROWS = [
  ['esc', 'tab', 'ctrl', 'alt'],
  ['ctrl-c', 'ctrl-d', 'ctrl-z', 'ctrl-l'],
  ['slash', 'dash', 'pipe', 'tilde'],
];

export const KEY_PAD_RIGHT_IDS = ['pgup', 'up', 'pgdn', 'left', 'down', 'right', 'home', 'end', 'enter'];

export const REPEAT_DELAY_MS = 420;
export const REPEAT_INTERVAL_MS = 90;

export function keyDefinition(id) {
  return KEY_DEFINITIONS.find((key) => key.id === id) || null;
}

export function controlByte(character) {
  const upper = String(character || '').toUpperCase();
  if (upper.length !== 1) return null;
  const code = upper.charCodeAt(0);
  if (code >= 64 && code <= 95) return String.fromCharCode(code - 64);
  if (code === 63) return '\x7f';
  return null;
}

export function applyModifiers(bytes, modifiers) {
  let result = String(bytes);
  const active = modifiers || {};
  if (active.ctrl) {
    const control = controlByte(result);
    if (control !== null) result = control;
  }
  if (active.alt) result = ESC + result;
  return result;
}

export function encodeInput(value) {
  return new TextEncoder().encode(value);
}
