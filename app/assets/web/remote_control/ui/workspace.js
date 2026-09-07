import { h, clear, icon, shortenPath } from '../dom.js';
import { screenForProject } from '../store.js';
import { attachLongPress } from './sheets.js';

const TAB_ICONS = {
  terminal: 'terminal',
  code: 'code',
  file: 'file',
  agent_picker: 'robot',
  settings: 'settings',
  mixed: 'terminal',
  other: 'terminal',
};

const SUMMARY_DOTS = {
  none: { class: 'dot-idle', label: 'Idle' },
  working: { class: 'dot-working', label: 'Agent working' },
  needs_attention: { class: 'dot-needs-attention', label: 'Needs attention' },
};

export function agentSummaryDot(summary) {
  const info = SUMMARY_DOTS[summary] || SUMMARY_DOTS.none;
  return h('span', { class: `dot ${info.class}`, role: 'img', 'aria-label': info.label });
}

export function render(host, ctx, state) {
  clear(host);
  const pad = h('div', { class: 'screen-pad' });
  host.appendChild(h('div', { class: 'screen' }, pad));

  const projectId = state.route.params.id;
  const project = projectId === 'home' ? null : state.indexes.projects.get(projectId);
  const found = screenForProject(state, projectId);

  if (!found && !project) {
    pad.appendChild(
      h(
        'div',
        { class: 'empty-state' },
        icon('warning', 'icon-lg'),
        h('p', { class: 'empty-title' }, 'Workspace not found'),
        h('p', { class: 'empty-text' }, 'It may have been removed on the desktop.'),
        h(
          'button',
          { class: 'button', type: 'button', onClick: () => ctx.navigate('workspaces', {}) },
          'Back to Workspaces',
        ),
      ),
    );
    return;
  }

  if (project && !found) {
    pad.appendChild(
      h(
        'div',
        { class: 'banner banner-info' },
        icon('desktop', 'icon-sm'),
        h('span', { class: 'banner-text' }, 'This Workspace is not open on the desktop yet.'),
        h(
          'button',
          {
            class: 'banner-action',
            type: 'button',
            onClick: () => ctx.command('project.open', { project_id: project.id }),
          },
          'Open',
        ),
      ),
    );
    pad.appendChild(worktreeOverview(ctx, project));
    return;
  }

  const screen = found.screen;
  pad.appendChild(headerActions(ctx, state, project, screen));

  if (!screen.sections.length) {
    pad.appendChild(
      h(
        'div',
        { class: 'empty-state' },
        icon('terminal', 'icon-lg'),
        h('p', { class: 'empty-title' }, 'No tabs yet'),
        h('p', { class: 'empty-text' }, 'Create a terminal or launch an agent to get started.'),
      ),
    );
  }

  for (const section of screen.sections) {
    pad.appendChild(sectionBlock(ctx, state, project, screen, section, false));
  }

  if (project && project.kind === 'git' && state.snapshot.features.ade_workspaces) {
    pad.appendChild(
      h(
        'button',
        {
          class: 'button button-block',
          type: 'button',
          onClick: () => ctx.openSheet('new-worktree', { projectId: project.id }),
        },
        icon('git-branch'),
        'New worktree',
      ),
    );
  }
}

function headerActions(ctx, state, project, screen) {
  const actions = h('div', { class: 'button-row' });
  actions.appendChild(
    h(
      'button',
      {
        class: 'button button-small',
        type: 'button',
        onClick: () => ctx.openSheet('new-terminal', { screenId: screen.id }),
      },
      icon('terminal', 'icon-sm'),
      'New terminal',
    ),
  );
  actions.appendChild(
    h(
      'button',
      {
        class: 'button button-small',
        type: 'button',
        onClick: () => ctx.openSheet('launch-agent', { screenId: screen.id }),
      },
      icon('robot', 'icon-sm'),
      'Launch agent',
    ),
  );
  if (project) {
    actions.appendChild(
      h(
        'button',
        {
          class: 'button button-small',
          type: 'button',
          onClick: () => ctx.command('project.open', { project_id: project.id }),
        },
        icon('desktop', 'icon-sm'),
        'Show on desktop',
      ),
    );
  } else {
    actions.appendChild(
      h(
        'button',
        {
          class: 'button button-small',
          type: 'button',
          onClick: () => ctx.command('home.activate', {}),
        },
        icon('desktop', 'icon-sm'),
        'Show on desktop',
      ),
    );
  }
  return actions;
}

function worktreeOverview(ctx, project) {
  const rows = h('div', { class: 'row-group' });
  for (const worktree of project.worktrees) {
    rows.appendChild(
      h(
        'div',
        { class: 'row' },
        h('span', { class: 'row-lead' }, icon('git-branch'), agentSummaryDot(worktree.agent_summary)),
        h(
          'div',
          { class: 'row-texts' },
          h('div', { class: 'row-title' }, worktree.name),
          h('div', { class: 'row-sub row-sub-mono' }, worktree.branch || shortenPath(worktree.path, 32)),
        ),
        h('span', { class: 'row-trail' }, `${worktree.open_tab_count} tabs`),
      ),
    );
  }
  return h(
    'div',
    { class: 'section' },
    h('div', { class: 'section-head' }, h('h2', { class: 'section-title' }, 'Worktrees')),
    project.worktrees.length ? rows : h('p', { class: 'section-empty' }, 'No worktrees recorded.'),
  );
}

function sectionBlock(ctx, state, project, screen, section, compact) {
  const worktree = section.worktree_id ? state.indexes.worktrees.get(section.worktree_id) : null;
  const head = h(
    'div',
    { class: 'section-head' },
    h('h2', { class: 'section-title section-title-strong' }, section.title),
    worktree && worktree.worktree.branch
      ? h('span', { class: 'pill pill-mono' }, worktree.worktree.branch)
      : null,
    worktree ? agentSummaryDot(worktree.worktree.agent_summary) : null,
    h('span', { class: 'section-spacer' }),
    h(
      'button',
      {
        class: 'icon-button',
        type: 'button',
        'aria-label': `Add to ${section.title}`,
        onClick: () => openSectionMenu(ctx, state, project, screen, section),
      },
      icon('plus'),
    ),
  );
  const rows = h('div', { class: 'row-group' });
  if (!section.tabs.length) {
    rows.appendChild(h('p', { class: 'section-empty' }, 'No tabs in this section.'));
  }
  for (const tab of section.tabs) rows.appendChild(tabRow(ctx, state, screen, tab, compact));
  return h('div', { class: 'section' }, head, rows);
}

function tabRow(ctx, state, screen, tab, compact) {
  const active = screen.active_tab_id === tab.id;
  const terminalPane = tab.panes.find((pane) => pane.terminal);
  const focusedPane = tab.panes.find((pane) => pane.id === tab.focused_pane_id) || terminalPane;
  const targetTerminal = focusedPane && focusedPane.terminal ? focusedPane.terminal.terminal_id : null;
  const agent = focusedPane && focusedPane.terminal ? focusedPane.terminal.agent : null;

  const row = h(
    'button',
    {
      class: 'row row-button',
      type: 'button',
      'data-active': active ? 'true' : null,
      onClick: async () => {
        try {
          await ctx.command('tab.activate', { tab_id: tab.id });
        } finally {
          if (targetTerminal) ctx.navigate('terminal', { id: targetTerminal });
        }
      },
    },
    h(
      'span',
      { class: 'row-lead' },
      icon(TAB_ICONS[tab.kind] || 'terminal'),
      agentSummaryDot(tab.agent_summary),
    ),
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, tab.title),
      compact
        ? null
        : h(
            'div',
            { class: 'row-sub' },
            [
              tab.group_title,
              agent ? agent.display_name : null,
              tab.panes.length > 1 ? `${tab.panes.length} panes` : null,
              tab.pinned ? 'Pinned' : null,
            ]
              .filter(Boolean)
              .join(' · '),
          ),
    ),
    h('span', { class: 'row-trail' }, icon('chevron-right', 'icon-sm')),
  );
  attachLongPress(row, () => openTabMenu(ctx, tab, targetTerminal));

  const menuButton = h(
    'button',
    {
      class: 'icon-button',
      type: 'button',
      'aria-label': `Actions for ${tab.title}`,
      onClick: () => openTabMenu(ctx, tab, targetTerminal),
    },
    icon('kebab'),
  );

  const wrapper = h('div', { class: 'row-with-menu' }, row, menuButton);
  if (compact || tab.panes.length < 2) return wrapper;

  const paneRows = h('div', { class: 'row-group pane-group' });
  for (const pane of tab.panes) {
    paneRows.appendChild(paneRow(ctx, pane));
  }
  return h('div', { class: 'tab-block' }, wrapper, paneRows);
}

function paneRow(ctx, pane) {
  const terminalId = pane.terminal ? pane.terminal.terminal_id : null;
  return h(
    'button',
    {
      class: 'row row-button row-nested',
      type: 'button',
      onClick: async () => {
        try {
          await ctx.command('pane.focus', { pane_id: pane.id });
        } finally {
          if (terminalId) ctx.navigate('terminal', { id: terminalId });
        }
      },
    },
    h('span', { class: 'row-lead' }, icon(TAB_ICONS[pane.kind] || 'terminal', 'icon-sm')),
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, pane.title),
      pane.terminal && pane.terminal.cwd
        ? h('div', { class: 'row-sub row-sub-mono' }, shortenPath(pane.terminal.cwd, 30))
        : null,
    ),
    terminalId ? h('span', { class: 'row-trail' }, icon('chevron-right', 'icon-sm')) : null,
  );
}

function openTabMenu(ctx, tab, terminalId) {
  const items = [];
  if (terminalId) {
    items.push({
      label: 'Open terminal',
      icon: 'terminal',
      onSelect: () => ctx.navigate('terminal', { id: terminalId }),
    });
    items.push({
      label: 'Show on desktop',
      icon: 'desktop',
      onSelect: () => ctx.command('desktop.reveal', { terminal_id: terminalId }),
    });
    items.push({ separator: true });
  }
  items.push({
    label: 'Close tab',
    icon: 'close',
    danger: true,
    onSelect: async () => {
      const confirmed = await ctx.confirm({
        title: `Close ${tab.title}?`,
        message: 'Anything running in this tab stops. Spirit keeps it on the undo stack.',
        confirmLabel: 'Close tab',
        danger: true,
      });
      if (!confirmed) return;
      await ctx.command('tab.close', { tab_id: tab.id, confirm: true });
      ctx.toast('Tab closed', 'ok');
    },
  });
  ctx.openSheet('menu', { title: tab.title, items });
}

function openSectionMenu(ctx, state, project, screen, section) {
  const worktreeId = section.worktree_id;
  const worktree = worktreeId ? state.indexes.worktrees.get(worktreeId) : null;
  const items = [
    {
      label: 'New terminal',
      icon: 'terminal',
      onSelect: () =>
        ctx.openSheet('new-terminal', {
          screenId: screen.id,
          worktreeId,
          worktreeName: section.title,
        }),
    },
    {
      label: 'Launch agent',
      icon: 'robot',
      onSelect: () =>
        ctx.openSheet('launch-agent', {
          screenId: screen.id,
          worktreeId,
          worktreeName: section.title,
        }),
    },
  ];
  if (worktree && worktree.worktree.kind === 'linked') {
    items.push({ separator: true });
    items.push({
      label: 'Rename worktree',
      icon: 'edit',
      onSelect: () =>
        ctx.openSheet('rename-worktree', { worktreeId, name: worktree.worktree.name }),
    });
    items.push({
      label: 'Delete worktree',
      icon: 'trash',
      danger: true,
      onSelect: () => deleteWorktree(ctx, worktree.worktree),
    });
  }
  if (project && project.kind === 'git') {
    items.push({ separator: true });
    items.push({
      label: 'New worktree',
      icon: 'git-branch',
      onSelect: () => ctx.openSheet('new-worktree', { projectId: project.id }),
    });
  }
  ctx.openSheet('menu', { title: section.title, items });
}

async function deleteWorktree(ctx, worktree) {
  let dirty = false;
  try {
    const status = await ctx.command('worktree.dirty_check', { worktree_id: worktree.id });
    dirty = Boolean(status && status.dirty);
  } catch (error) {
    dirty = false;
  }
  const confirmed = await ctx.confirm({
    title: `Delete ${worktree.name}?`,
    message: dirty
      ? 'This worktree has uncommitted changes. Deleting it discards them and closes its tabs.'
      : 'Spirit closes the tabs in this worktree and removes the checkout.',
    confirmLabel: dirty ? 'Discard and delete' : 'Delete',
    danger: true,
  });
  if (!confirmed) return;
  const result = await ctx.command(
    'worktree.delete',
    { worktree_id: worktree.id, confirm: true, force: dirty },
    { timeoutMs: 120000 },
  );
  ctx.toast(result && result.branch_kept ? 'Worktree deleted, branch kept' : 'Worktree deleted', 'ok');
}

export function sidebarSectionNodes(ctx, state, screen) {
  const projectId = screen.project_id;
  const project = projectId ? state.indexes.projects.get(projectId) : null;
  return screen.sections.map((section) => sectionBlock(ctx, state, project, screen, section, true));
}
