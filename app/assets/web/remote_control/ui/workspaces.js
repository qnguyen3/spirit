import { h, clear, icon, relativeTime, shortenPath } from '../dom.js';
import { attachLongPress } from './sheets.js';

export function render(host, ctx, state) {
  clear(host);
  const pad = h('div', { class: 'screen-pad' });
  const screen = h('div', { class: 'screen' }, pad);
  host.appendChild(screen);

  if (!state.snapshot) {
    pad.appendChild(loadingCard());
    return;
  }

  const grid = h('div', { class: 'card-grid' });
  grid.appendChild(homeCard(ctx, state));

  const adeEnabled = state.snapshot.features.ade_workspaces;
  if (adeEnabled) {
    for (const project of state.snapshot.projects) grid.appendChild(projectCard(ctx, state, project));
    grid.appendChild(newWorkspaceCard(ctx));
  }
  pad.appendChild(grid);

  if (!adeEnabled) {
    pad.appendChild(
      h(
        'div',
        { class: 'empty-state' },
        icon('warning', 'icon-lg'),
        h('p', { class: 'empty-title' }, 'Workspaces are off'),
        h(
          'p',
          { class: 'empty-text' },
          'This build of Spirit has the Workspaces feature disabled, so only Home is available. Enable it on the desktop to manage projects and worktrees from here.',
        ),
      ),
    );
  }
}

function loadingCard() {
  return h(
    'div',
    { class: 'empty-state' },
    h('span', { class: 'spinner', role: 'img', 'aria-label': 'Loading' }),
    h('p', { class: 'empty-text' }, 'Waiting for the first state snapshot from Spirit…'),
  );
}

function homeCard(ctx, state) {
  const openTabs = countHomeTabs(state);
  const card = h(
    'button',
    {
      class: 'card card-button',
      type: 'button',
      onClick: async () => {
        await ctx.command('home.activate', {});
        ctx.navigate('workspace', { id: 'home' });
      },
    },
    h(
      'div',
      { class: 'card-head' },
      h('span', { class: 'card-icon' }, icon('home')),
      h(
        'div',
        { class: 'card-titles' },
        h('div', { class: 'card-title' }, 'Home'),
        h('div', { class: 'card-sub' }, 'Tabs that are not bound to a Workspace'),
      ),
    ),
    h(
      'div',
      { class: 'card-meta' },
      h('span', { class: 'pill' }, `${openTabs} tab${openTabs === 1 ? '' : 's'}`),
    ),
  );
  return h('div', { class: 'card-shell' }, card);
}

function countHomeTabs(state) {
  let count = 0;
  for (const entry of state.indexes.tabs.values()) {
    if (!entry.screen.project_id) count += 1;
  }
  return count;
}

function projectCard(ctx, state, project) {
  const open = async () => {
    try {
      await ctx.command('project.open', { project_id: project.id });
    } finally {
      ctx.navigate('workspace', { id: project.id });
    }
  };
  const worktreeCount = project.worktrees.length;
  const card = h(
    'button',
    { class: 'card card-button', type: 'button', onClick: open },
    h(
      'div',
      { class: 'card-head' },
      h('span', { class: 'card-icon' }, icon(project.kind === 'git' ? 'git-branch' : 'folder')),
      h(
        'div',
        { class: 'card-titles' },
        h('div', { class: 'card-title' }, project.name),
        h('div', { class: 'card-sub' }, shortenPath(project.root_path)),
      ),
    ),
    h(
      'div',
      { class: 'card-meta' },
      project.primary_branch ? h('span', { class: 'pill pill-mono' }, project.primary_branch) : null,
      h('span', { class: 'pill' }, `${worktreeCount} worktree${worktreeCount === 1 ? '' : 's'}`),
      project.counts.working
        ? h('span', { class: 'pill pill-accent' }, `${project.counts.working} working`)
        : null,
      project.counts.needs_attention
        ? h('span', { class: 'pill pill-warn' }, `${project.counts.needs_attention} need input`)
        : null,
      project.open_in_window_id ? h('span', { class: 'pill pill-info' }, 'Open on desktop') : null,
      project.last_opened_ts
        ? h('span', { class: 'pill' }, relativeTime(project.last_opened_ts))
        : null,
    ),
  );
  const menuButton = h(
    'button',
    {
      class: 'icon-button card-menu',
      type: 'button',
      'aria-label': `Actions for ${project.name}`,
      onClick: () => openProjectMenu(ctx, project),
    },
    icon('kebab'),
  );
  attachLongPress(card, () => openProjectMenu(ctx, project));
  return h('div', { class: 'card-shell' }, card, menuButton);
}

export function openProjectMenu(ctx, project) {
  ctx.openSheet('menu', {
    title: project.name,
    items: [
      {
        label: 'Open Workspace',
        icon: 'folder',
        onSelect: async () => {
          await ctx.command('project.open', { project_id: project.id });
          ctx.navigate('workspace', { id: project.id });
        },
      },
      {
        label: 'Show on desktop',
        icon: 'desktop',
        onSelect: () => ctx.command('project.reveal', { project_id: project.id }),
      },
      {
        label: 'Rename',
        icon: 'edit',
        onSelect: () => ctx.openSheet('rename-project', { projectId: project.id, name: project.name }),
      },
      { separator: true },
      {
        label: 'Remove from Spirit',
        icon: 'trash',
        danger: true,
        onSelect: async () => {
          const confirmed = await ctx.confirm({
            title: `Remove ${project.name}?`,
            message: 'Spirit forgets this Workspace and closes its screens. The folder stays on disk.',
            confirmLabel: 'Remove',
            danger: true,
          });
          if (!confirmed) return;
          await ctx.command('project.remove', { project_id: project.id, confirm: true });
          ctx.toast(`Removed ${project.name}`, 'ok');
          ctx.navigate('workspaces', {});
        },
      },
    ],
  });
}

function newWorkspaceCard(ctx) {
  return h(
    'div',
    { class: 'card-shell' },
    h(
      'button',
      {
        class: 'card card-button card-dashed',
        type: 'button',
        onClick: () => ctx.openSheet('new-workspace', {}),
      },
      icon('plus'),
      'New Workspace',
    ),
  );
}
