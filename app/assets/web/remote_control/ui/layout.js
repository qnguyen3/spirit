import { h, clear, icon } from '../dom.js';
import { needsInputCount, terminalTitle, screenForProject } from '../store.js';
import { renderSheetHost } from './sheets.js';
import { sidebarSectionNodes } from './workspace.js';
import { sessionSummaryNodes } from './sessions.js';
import { terminalController } from './terminal_view.js';

const CONNECTION_LABELS = {
  open: { text: 'Live', dot: 'dot-done', label: 'Connected to Spirit' },
  connecting: { text: 'Connecting', dot: 'dot-working', label: 'Connecting to Spirit' },
  reconnecting: { text: 'Reconnecting', dot: 'dot-working', label: 'Reconnecting to Spirit' },
  error: { text: 'Offline', dot: 'dot-failed', label: 'Connection error' },
  closed: { text: 'Offline', dot: 'dot-offline', label: 'Disconnected from Spirit' },
};

const MODE_LABELS = {
  prompt: 'shell',
  running: 'running',
  alt_screen: 'full screen',
};

export function mountLayout(root, ctx) {
  const railNode = h('nav', { class: 'rail', 'aria-label': 'Workspace switcher' });
  const sidebarScroll = h('div', { class: 'sidebar-scroll' });
  const sidebarFoot = h('div', { class: 'sidebar-foot' });
  const sidebarNode = h('aside', { class: 'sidebar', 'aria-label': 'Navigation' }, sidebarScroll, sidebarFoot);
  const topbarNode = h('header', { class: 'topbar' });
  const bannerSlot = h('div', { class: 'banner-slot' });
  const contentHost = h('main', { class: 'content', id: 'content', tabindex: '-1' });
  const bottomNav = h('nav', { class: 'bottom-nav', 'aria-label': 'Sections' });
  const mainColumn = h('div', { class: 'main-column' }, topbarNode, bannerSlot, contentHost, bottomNav);
  const frame = h('div', { class: 'frame' }, railNode, sidebarNode, mainColumn);
  const toastStack = h('div', {
    class: 'toast-stack',
    role: 'status',
    'aria-live': 'polite',
    'aria-atomic': 'false',
  });
  const sheetHost = h('div', { class: 'sheet-host' });

  clear(root);
  root.appendChild(frame);
  root.appendChild(toastStack);
  root.appendChild(sheetHost);

  function render(state) {
    frame.dataset.route = state.route.name;
    contentHost.dataset.scroll = state.route.name === 'terminal' ? 'none' : 'auto';
    toastStack.dataset.compact = state.route.name === 'terminal' ? 'true' : 'false';
    renderTopbar(topbarNode, ctx, state);
    renderBanners(bannerSlot, ctx, state);
    renderBottomNav(bottomNav, ctx, state);
    renderRail(railNode, ctx, state);
    renderSidebar(sidebarScroll, sidebarFoot, ctx, state);
    renderToasts(toastStack, ctx, state);
    renderSheetHost(sheetHost, ctx, state);
  }

  return { render, contentHost, frame };
}

function connectionIndicator(state) {
  const info = CONNECTION_LABELS[state.connection.status] || CONNECTION_LABELS.closed;
  return h(
    'span',
    { class: 'conn', title: info.label },
    h('span', { class: `dot ${info.dot}`, role: 'img', 'aria-label': info.label }),
    h('span', { class: 'conn-text' }, info.text),
  );
}

function backTarget(state) {
  if (state.route.name === 'terminal') {
    const found = state.indexes.terminals.get(state.route.params.id);
    if (found) {
      const projectId = found.screen.project_id || 'home';
      return { name: 'workspace', params: { id: projectId }, label: 'Back to Workspace' };
    }
    return { name: 'sessions', params: {}, label: 'Back to Sessions' };
  }
  if (state.route.name === 'workspace') {
    return { name: 'workspaces', params: {}, label: 'Back to Workspaces' };
  }
  return null;
}

function topbarTitle(state) {
  switch (state.route.name) {
    case 'workspaces':
      return { heading: 'Workspaces', sub: workspacesSubtitle(state) };
    case 'workspace': {
      const id = state.route.params.id;
      if (id === 'home') return { heading: 'Home', sub: 'Unbound tabs' };
      const project = state.indexes.projects.get(id);
      if (!project) return { heading: 'Workspace', sub: null };
      return { heading: project.name, sub: project.primary_branch || project.root_path };
    }
    case 'sessions': {
      const count = needsInputCount(state);
      return { heading: 'Sessions', sub: count ? `${count} waiting for input` : 'Live agent sessions' };
    }
    case 'terminal': {
      const id = state.route.params.id;
      const found = state.indexes.terminals.get(id);
      const agent = found && found.terminal.agent ? found.terminal.agent.display_name : null;
      const mode = state.terminal.terminalId === id ? state.terminal.mode : found && found.terminal.mode;
      return {
        heading: terminalTitle(state, id),
        sub: [agent, MODE_LABELS[mode] || null].filter(Boolean).join(' · ') || null,
      };
    }
    case 'settings':
      return { heading: 'Settings', sub: 'This device' };
    default:
      return { heading: 'Spirit', sub: null };
  }
}

function workspacesSubtitle(state) {
  const projects = (state.snapshot && state.snapshot.projects) || [];
  if (!projects.length) return 'No Workspaces yet';
  return `${projects.length} Workspace${projects.length === 1 ? '' : 's'}`;
}

function renderTopbar(node, ctx, state) {
  clear(node);
  const back = backTarget(state);
  if (back) {
    node.appendChild(
      h(
        'button',
        {
          class: 'icon-button',
          type: 'button',
          'aria-label': back.label,
          onClick: () => ctx.navigate(back.name, back.params),
        },
        icon('chevron-left'),
      ),
    );
  }
  const titles = topbarTitle(state);
  node.appendChild(
    h(
      'div',
      { class: 'topbar-title' },
      h('h1', { class: 'topbar-heading' }, titles.heading),
      titles.sub ? h('p', { class: 'topbar-sub' }, titles.sub) : null,
    ),
  );
  node.appendChild(connectionIndicator(state));
  node.appendChild(
    h(
      'button',
      {
        class: 'icon-button',
        type: 'button',
        'aria-label': 'More actions',
        onClick: () => ctx.openSheet('menu', { title: 'Actions', items: overflowItems(ctx, state) }),
      },
      icon('kebab'),
    ),
  );
}

function overflowItems(ctx, state) {
  const items = [];
  if (state.route.name === 'terminal') {
    const terminalId = state.route.params.id;
    items.push({
      label: 'Show on desktop',
      icon: 'desktop',
      onSelect: () => ctx.command('desktop.reveal', { terminal_id: terminalId }),
    });
    items.push({ label: 'Font size', icon: 'edit', onSelect: () => ctx.openSheet('font-size', {}) });
    items.push({
      label: 'Paste',
      icon: 'clipboard',
      onSelect: () => ctx.openSheet('paste', { terminalId }),
    });
    items.push({
      label: 'Copy selection',
      icon: 'clipboard',
      onSelect: () => {
        const selection = terminalController(ctx).selection();
        ctx.openSheet('copy', { title: 'Copy selection', text: selection || '' });
      },
    });
    items.push({
      label: 'Detach terminal',
      icon: 'stop',
      onSelect: () => {
        terminalController(ctx).detach();
        ctx.navigate('sessions', {});
      },
    });
    items.push({ separator: true });
  }
  items.push({
    label: 'Reconnect now',
    icon: 'refresh',
    onSelect: () => ctx.ws.retry(),
    disabled: state.connection.status === 'open',
  });
  items.push({ label: 'Settings', icon: 'settings', onSelect: () => ctx.navigate('settings', {}) });
  return items;
}

function renderBanners(node, ctx, state) {
  clear(node);
  const status = state.connection.status;
  if (status === 'reconnecting' || status === 'connecting') {
    node.appendChild(
      banner('info', 'refresh', status === 'connecting' ? 'Connecting to Spirit…' : 'Reconnecting…', null),
    );
  } else if (status === 'closed' || status === 'error') {
    node.appendChild(
      banner('danger', 'warning', 'Not connected to Spirit.', {
        label: 'Retry',
        onSelect: () => ctx.ws.retry(),
      }),
    );
  }
  if (state.hello && state.hello.protocol !== undefined && state.ui.protocolMismatch) {
    node.appendChild(
      banner('danger', 'warning', 'This page is out of date. Update Spirit or reload.', {
        label: 'Reload',
        onSelect: () => window.location.reload(),
      }),
    );
  }
  if (
    state.snapshot &&
    state.snapshot.server.lan_access &&
    window.location.protocol === 'http:' &&
    !isLoopbackHost()
  ) {
    node.appendChild(
      banner('warn', 'warning', 'LAN access without TLS. Anyone on this network can read this traffic.', null),
    );
  }
  const alert = state.ui.needsInputAlert;
  if (alert && state.route.name !== 'terminal') {
    node.appendChild(
      banner('warn', 'robot', `${alert.title} needs input`, {
        label: 'Open',
        onSelect: () => ctx.navigate('terminal', { id: alert.terminalId }),
      }),
    );
  }
}

function isLoopbackHost() {
  const host = window.location.hostname;
  return host === 'localhost' || host === '127.0.0.1' || host === '::1' || host === '[::1]';
}

function banner(kind, iconName, message, action) {
  return h(
    'div',
    { class: `banner banner-${kind}` },
    icon(iconName, 'icon-sm'),
    h('span', { class: 'banner-text' }, message),
    action
      ? h('button', { class: 'banner-action', type: 'button', onClick: action.onSelect }, action.label)
      : null,
  );
}

function renderBottomNav(node, ctx, state) {
  clear(node);
  const needsInput = needsInputCount(state);
  const terminalId = state.route.name === 'terminal' ? state.route.params.id : state.ui.lastTerminalId;
  const items = [
    { name: 'workspaces', label: 'Workspaces', iconName: 'workspaces', params: {} },
    { name: 'sessions', label: 'Sessions', iconName: 'sessions', params: {}, badge: needsInput },
    {
      name: 'terminal',
      label: 'Terminal',
      iconName: 'terminal',
      params: { id: terminalId },
      disabled: !terminalId,
    },
  ];
  for (const item of items) {
    const active = state.route.name === item.name;
    node.appendChild(
      h(
        'button',
        {
          class: 'nav-item',
          type: 'button',
          'aria-current': active ? 'page' : null,
          disabled: item.disabled === true,
          onClick: () => ctx.navigate(item.name, item.params),
        },
        icon(item.iconName, 'icon-lg'),
        h('span', { class: 'nav-label' }, item.label),
        item.badge ? h('span', { class: 'badge nav-badge' }, String(item.badge)) : null,
      ),
    );
  }
}

function renderRail(node, ctx, state) {
  clear(node);
  const projects = (state.snapshot && state.snapshot.projects) || [];
  const currentId = state.route.name === 'workspace' ? state.route.params.id : null;
  node.appendChild(
    h(
      'button',
      {
        class: 'rail-item',
        type: 'button',
        'aria-label': 'Home',
        'aria-current': currentId === 'home' ? 'page' : null,
        onClick: () => ctx.navigate('workspace', { id: 'home' }),
      },
      icon('home'),
    ),
  );
  const showProjects = !state.snapshot || state.snapshot.features.ade_workspaces;
  if (showProjects) {
    for (const project of projects) {
      const attention = project.counts.needs_attention;
      node.appendChild(
        h(
          'button',
          {
            class: 'rail-item',
            type: 'button',
            'aria-label': project.name,
            title: project.name,
            'aria-current': currentId === project.id ? 'page' : null,
            onClick: () => ctx.navigate('workspace', { id: project.id }),
          },
          h('span', { 'aria-hidden': 'true' }, project.name.slice(0, 2).toUpperCase()),
          attention ? h('span', { class: 'badge rail-badge' }, String(attention)) : null,
        ),
      );
    }
    node.appendChild(
      h(
        'button',
        {
          class: 'rail-item',
          type: 'button',
          'aria-label': 'New Workspace',
          onClick: () => ctx.openSheet('new-workspace', {}),
        },
        icon('plus'),
      ),
    );
  }
  node.appendChild(h('div', { class: 'rail-spacer' }));
  node.appendChild(
    h(
      'button',
      {
        class: 'rail-item',
        type: 'button',
        'aria-label': 'Settings',
        'aria-current': state.route.name === 'settings' ? 'page' : null,
        onClick: () => ctx.navigate('settings', {}),
      },
      icon('settings'),
    ),
  );
}

function renderSidebar(scroll, foot, ctx, state) {
  clear(scroll);
  clear(foot);
  const projectId = currentProjectId(state);
  const project = projectId && projectId !== 'home' ? state.indexes.projects.get(projectId) : null;
  scroll.appendChild(
    h(
      'button',
      {
        class: 'row row-button',
        type: 'button',
        onClick: () => ctx.navigate('workspaces', {}),
      },
      h('span', { class: 'row-lead' }, icon(project ? 'folder' : 'home')),
      h(
        'span',
        { class: 'row-texts' },
        h('span', { class: 'row-title' }, project ? project.name : 'Home'),
        h('span', { class: 'row-sub' }, 'Switch Workspace'),
      ),
      h('span', { class: 'row-trail' }, icon('chevron-down', 'icon-sm')),
    ),
  );
  const found = screenForProject(state, projectId || 'home');
  if (found) {
    for (const node of sidebarSectionNodes(ctx, state, found.screen)) scroll.appendChild(node);
  }
  const sessionNodes = sessionSummaryNodes(ctx, state, 6);
  if (sessionNodes.length) {
    scroll.appendChild(
      h(
        'div',
        { class: 'section' },
        h(
          'div',
          { class: 'section-head' },
          h('h2', { class: 'section-title' }, 'Sessions'),
          h('span', { class: 'section-spacer' }),
          h(
            'button',
            {
              class: 'button button-quiet button-small',
              type: 'button',
              onClick: () => ctx.navigate('sessions', {}),
            },
            'All',
          ),
        ),
        h('div', { class: 'row-group' }, sessionNodes),
      ),
    );
  }
  foot.appendChild(
    h(
      'button',
      {
        class: 'button button-small',
        type: 'button',
        onClick: () => ctx.navigate('settings', {}),
      },
      icon('settings', 'icon-sm'),
      'Settings',
    ),
  );
}

function currentProjectId(state) {
  if (state.route.name === 'workspace') return state.route.params.id;
  if (state.route.name === 'terminal') {
    const found = state.indexes.terminals.get(state.route.params.id);
    if (found) return found.screen.project_id || 'home';
  }
  return state.ui.lastProjectId || 'home';
}

function renderToasts(node, ctx, state) {
  clear(node);
  for (const toast of state.ui.toasts) {
    node.appendChild(
      h(
        'button',
        {
          class: `toast toast-${toast.kind || 'info'}`,
          type: 'button',
          onClick: () => ctx.store.patchUi({ toasts: state.ui.toasts.filter((entry) => entry.id !== toast.id) }),
        },
        icon(toast.kind === 'error' ? 'warning' : toast.kind === 'ok' ? 'check' : 'terminal', 'icon-sm'),
        h('span', { class: 'toast-text' }, toast.message),
        icon('close', 'icon-sm'),
      ),
    );
  }
}
