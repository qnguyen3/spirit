const EVENT_PROPS = {
  onClick: 'click',
  onInput: 'input',
  onChange: 'change',
  onSubmit: 'submit',
  onKeyDown: 'keydown',
  onKeyUp: 'keyup',
  onFocus: 'focus',
  onBlur: 'blur',
  onPointerDown: 'pointerdown',
  onPointerUp: 'pointerup',
  onPointerCancel: 'pointercancel',
  onScroll: 'scroll',
  onPaste: 'paste',
  onContextMenu: 'contextmenu',
};

const SVG_NS = 'http://www.w3.org/2000/svg';
const SVG_TAGS = new Set(['svg', 'use', 'path', 'circle', 'rect', 'g']);

export function h(tag, props, ...children) {
  const node = SVG_TAGS.has(tag)
    ? document.createElementNS(SVG_NS, tag)
    : document.createElement(tag);
  applyProps(node, props || {});
  appendChildren(node, children);
  return node;
}

function applyProps(node, props) {
  for (const [key, value] of Object.entries(props)) {
    if (value === null || value === undefined || value === false) continue;
    const eventName = EVENT_PROPS[key];
    if (eventName) {
      node.addEventListener(eventName, value);
      continue;
    }
    if (key === 'text') {
      node.textContent = String(value);
      continue;
    }
    if (key === 'class') {
      node.setAttribute('class', Array.isArray(value) ? value.filter(Boolean).join(' ') : value);
      continue;
    }
    if (key === 'style' && typeof value === 'object') {
      for (const [property, setting] of Object.entries(value)) {
        node.style.setProperty(property, setting);
      }
      continue;
    }
    if (key === 'dataset' && typeof value === 'object') {
      for (const [property, setting] of Object.entries(value)) {
        if (setting !== null && setting !== undefined) node.dataset[property] = String(setting);
      }
      continue;
    }
    if (key === 'value' && (node instanceof HTMLInputElement || node instanceof HTMLTextAreaElement)) {
      node.value = String(value);
      continue;
    }
    if (value === true) {
      node.setAttribute(key, '');
      continue;
    }
    node.setAttribute(key, String(value));
  }
}

function appendChildren(node, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    node.appendChild(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}

export function text(value) {
  return document.createTextNode(value === null || value === undefined ? '' : String(value));
}

export function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
  return node;
}

export function replace(node, ...children) {
  clear(node);
  appendChildren(node, children);
  return node;
}

export function on(node, eventName, handler, options) {
  node.addEventListener(eventName, handler, options);
  return () => node.removeEventListener(eventName, handler, options);
}

export function icon(name, className) {
  const symbol = h('use');
  symbol.setAttributeNS('http://www.w3.org/1999/xlink', 'href', `#icon-${name}`);
  symbol.setAttribute('href', `#icon-${name}`);
  const svg = h('svg', { class: className ? `icon ${className}` : 'icon', 'aria-hidden': 'true' }, symbol);
  return svg;
}

export function relativeTime(timestampMs) {
  if (!timestampMs) return '';
  const seconds = Math.round((Date.now() - timestampMs) / 1000);
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.round(months / 12)}y ago`;
}

export function basename(path) {
  if (!path) return '';
  const parts = String(path).split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

export function shortenPath(path, maxLength = 42) {
  if (!path) return '';
  if (path.length <= maxLength) return path;
  return `…${path.slice(path.length - maxLength + 1)}`;
}
