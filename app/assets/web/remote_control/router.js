const ROUTES = [
  { name: 'workspaces', pattern: /^#?\/workspaces$/, params: () => ({}) },
  { name: 'workspace', pattern: /^#?\/w\/([^/]+)$/, params: (match) => ({ id: decodeURIComponent(match[1]) }) },
  { name: 'sessions', pattern: /^#?\/sessions$/, params: () => ({}) },
  { name: 'terminal', pattern: /^#?\/t\/([^/]+)$/, params: (match) => ({ id: decodeURIComponent(match[1]) }) },
  { name: 'settings', pattern: /^#?\/settings$/, params: () => ({}) },
];

export function hashFor(name, params) {
  switch (name) {
    case 'workspace':
      return `#/w/${encodeURIComponent(params.id)}`;
    case 'terminal':
      return `#/t/${encodeURIComponent(params.id)}`;
    case 'sessions':
      return '#/sessions';
    case 'settings':
      return '#/settings';
    case 'workspaces':
    default:
      return '#/workspaces';
  }
}

export function parse(hash) {
  const raw = hash || '#/workspaces';
  for (const route of ROUTES) {
    const match = raw.match(route.pattern);
    if (match) return { name: route.name, params: route.params(match) };
  }
  return { name: 'workspaces', params: {} };
}

export function createRouter(onRoute) {
  let current = parse(window.location.hash);
  let sheetDepth = 0;

  function emit() {
    onRoute(current);
  }

  function handleLocationChange() {
    const next = parse(window.location.hash);
    const state = window.history.state;
    if (state && state.sheet) return;
    if (sheetDepth > 0) {
      sheetDepth = 0;
      onRoute(current, { closedSheet: true });
      if (next.name === current.name && sameParams(next.params, current.params)) return;
    }
    current = next;
    emit();
  }

  return {
    start() {
      window.addEventListener('hashchange', handleLocationChange);
      window.addEventListener('popstate', handleLocationChange);
      emit();
    },
    current() {
      return current;
    },
    navigate(name, params, options) {
      const target = hashFor(name, params || {});
      if (window.location.hash === target) {
        current = parse(target);
        emit();
        return;
      }
      if (options && options.replace) {
        window.history.replaceState(null, '', target);
        current = parse(target);
        emit();
      } else {
        window.location.hash = target;
      }
    },
    pushSheet() {
      sheetDepth += 1;
      window.history.pushState({ sheet: sheetDepth }, '');
    },
    popSheet() {
      if (sheetDepth === 0) return;
      sheetDepth -= 1;
      window.history.back();
    },
    hasSheet() {
      return sheetDepth > 0;
    },
  };
}

function sameParams(left, right) {
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  if (leftKeys.length !== rightKeys.length) return false;
  return leftKeys.every((key) => left[key] === right[key]);
}
