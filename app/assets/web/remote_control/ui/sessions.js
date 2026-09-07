import { h, clear, icon, relativeTime, basename } from '../dom.js';
import { sessionsRanked } from '../store.js';

const STATUS_INFO = {
  idle: { dot: 'dot-idle', pill: 'pill', label: 'Idle' },
  in_progress: { dot: 'dot-working', pill: 'pill-accent', label: 'Working' },
  success: { dot: 'dot-done', pill: 'pill-ok', label: 'Done' },
  failed: { dot: 'dot-failed', pill: 'pill-danger', label: 'Failed' },
  blocked: { dot: 'dot-needs-attention', pill: 'pill-warn', label: 'Needs input' },
  cancelled: { dot: 'dot-idle', pill: 'pill', label: 'Cancelled' },
};

export function statusInfo(status) {
  return STATUS_INFO[status] || STATUS_INFO.idle;
}

export function render(host, ctx, state) {
  clear(host);
  const pad = h('div', { class: 'screen-pad' });
  host.appendChild(h('div', { class: 'screen' }, pad));

  const live = sessionsRanked(state);
  const liveRows = h('div', { class: 'row-group' });
  if (!live.length) {
    liveRows.appendChild(
      h('p', { class: 'section-empty' }, 'No agent is running right now. Launch one from a Workspace.'),
    );
  }
  for (const entry of live) liveRows.appendChild(liveRow(ctx, state, entry));
  pad.appendChild(
    h(
      'div',
      { class: 'section' },
      h('div', { class: 'section-head' }, h('h2', { class: 'section-title' }, 'Live')),
      liveRows,
    ),
  );

  const features = state.snapshot ? state.snapshot.features : { session_history: false };
  if (!features.session_history) {
    pad.appendChild(
      h(
        'div',
        { class: 'section' },
        h('div', { class: 'section-head' }, h('h2', { class: 'section-title' }, 'History')),
        h('p', { class: 'section-empty' }, 'Session history is turned off in this build of Spirit.'),
      ),
    );
    return;
  }

  const historyRows = h('div', { class: 'row-group' });
  const historyState = state.ui.historyState || 'idle';
  if (historyState === 'loading') {
    historyRows.appendChild(
      h(
        'p',
        { class: 'section-empty' },
        h('span', { class: 'spinner', role: 'img', 'aria-label': 'Loading' }),
      ),
    );
  } else if (historyState === 'error') {
    historyRows.appendChild(h('p', { class: 'section-empty' }, state.ui.historyError || 'Could not load history.'));
  } else if (!state.ui.history || !state.ui.history.length) {
    historyRows.appendChild(h('p', { class: 'section-empty' }, 'No past sessions recorded yet.'));
  } else {
    for (const session of state.ui.history) historyRows.appendChild(historyRow(ctx, session));
  }

  pad.appendChild(
    h(
      'div',
      { class: 'section' },
      h(
        'div',
        { class: 'section-head' },
        h('h2', { class: 'section-title' }, 'History'),
        h('span', { class: 'section-spacer' }),
        h(
          'button',
          {
            class: 'button button-quiet button-small',
            type: 'button',
            'aria-label': 'Refresh history',
            onClick: () => refreshHistory(ctx),
          },
          icon('refresh', 'icon-sm'),
          'Refresh',
        ),
      ),
      historyRows,
    ),
  );

  if (historyState === 'idle') loadHistory(ctx);
}

function liveRow(ctx, state, entry) {
  const session = entry.session;
  const info = statusInfo(session.status);
  const worktree = entry.worktree_id ? state.indexes.worktrees.get(entry.worktree_id) : null;
  const place = [entry.workspace_name, worktree ? worktree.worktree.name : null].filter(Boolean).join(' · ');
  const preview = session.tool_name
    ? [session.tool_name, session.tool_input_preview].filter(Boolean).join(': ')
    : session.summary || session.status_message || '';

  const row = h(
    'button',
    {
      class: 'row row-button',
      type: 'button',
      onClick: () => ctx.navigate('terminal', { id: entry.terminal_id }),
    },
    h(
      'span',
      { class: 'row-lead', style: { color: session.brand_color } },
      icon('robot'),
      h('span', { class: `dot ${info.dot}`, role: 'img', 'aria-label': info.label }),
    ),
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, session.title || session.display_name),
      h('div', { class: 'row-sub' }, place),
      preview ? h('div', { class: 'row-sub row-sub-mono' }, preview) : null,
    ),
    h('span', { class: 'row-trail' }, h('span', { class: `pill ${info.pill}` }, info.label)),
  );

  const menuButton = h(
    'button',
    {
      class: 'icon-button',
      type: 'button',
      'aria-label': `Actions for ${session.title || session.display_name}`,
      onClick: () =>
        ctx.openSheet('menu', {
          title: session.title || session.display_name,
          items: [
            {
              label: 'Open terminal',
              icon: 'terminal',
              onSelect: () => ctx.navigate('terminal', { id: entry.terminal_id }),
            },
            {
              label: 'Show on desktop',
              icon: 'desktop',
              onSelect: () => ctx.command('desktop.reveal', { terminal_id: entry.terminal_id }),
            },
            {
              label: 'Open Workspace',
              icon: 'folder',
              onSelect: () => ctx.navigate('workspace', { id: entry.project_id || 'home' }),
            },
          ],
        }),
    },
    icon('kebab'),
  );
  return h('div', { class: 'row-with-menu' }, row, menuButton);
}

function historyRow(ctx, session) {
  const row = h(
    'button',
    {
      class: 'row row-button',
      type: 'button',
      onClick: () => resumeSession(ctx, session),
    },
    h('span', { class: 'row-lead', style: { color: session.brand_color } }, icon('robot')),
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, session.title || session.display_name),
      h(
        'div',
        { class: 'row-sub' },
        [
          session.display_name,
          basename(session.cwd),
          relativeTime(session.modified_ts),
          `${session.message_count} message${session.message_count === 1 ? '' : 's'}`,
        ]
          .filter(Boolean)
          .join(' · '),
      ),
    ),
    h('span', { class: 'row-trail' }, icon('play', 'icon-sm')),
  );
  const menuButton = h(
    'button',
    {
      class: 'icon-button',
      type: 'button',
      'aria-label': `Actions for ${session.title || session.display_name}`,
      onClick: () =>
        ctx.openSheet('menu', {
          title: session.title || session.display_name,
          items: [
            { label: 'Resume session', icon: 'play', onSelect: () => resumeSession(ctx, session) },
            {
              label: 'Copy resume command',
              icon: 'clipboard',
              disabled: !session.resume_command,
              onSelect: () =>
                ctx.openSheet('copy', {
                  title: 'Resume command',
                  text: session.resume_command || '',
                }),
            },
          ],
        }),
    },
    icon('kebab'),
  );
  return h('div', { class: 'row-with-menu' }, row, menuButton);
}

async function resumeSession(ctx, session) {
  try {
    const data = await ctx.command('history.resume', { history_id: session.id }, { timeoutMs: 120000 });
    if (data && data.terminal_id) ctx.navigate('terminal', { id: data.terminal_id });
  } catch (error) {
    return;
  }
}

export async function loadHistory(ctx) {
  const state = ctx.store.get();
  if (state.ui.historyState === 'loading') return;
  ctx.store.patchUi({ historyState: 'loading', historyError: null });
  try {
    const data = await ctx.ws.command('history.list', {});
    ctx.store.patchUi({ history: (data && data.sessions) || [], historyState: 'ready' });
  } catch (error) {
    ctx.store.patchUi({
      historyState: 'error',
      historyError: error && error.message ? error.message : 'Could not load history.',
    });
  }
}

export async function refreshHistory(ctx) {
  try {
    await ctx.command('history.refresh', {});
  } catch (error) {
    return;
  }
  ctx.store.patchUi({ historyState: 'idle' });
  loadHistory(ctx);
}

export function sessionSummaryNodes(ctx, state, limit) {
  const live = sessionsRanked(state).slice(0, limit || 5);
  return live.map((entry) => {
    const info = statusInfo(entry.session.status);
    return h(
      'button',
      {
        class: 'row row-button',
        type: 'button',
        onClick: () => ctx.navigate('terminal', { id: entry.terminal_id }),
      },
      h(
        'span',
        { class: 'row-lead', style: { color: entry.session.brand_color } },
        h('span', { class: `dot ${info.dot}`, role: 'img', 'aria-label': info.label }),
      ),
      h(
        'div',
        { class: 'row-texts' },
        h('div', { class: 'row-title' }, entry.session.title || entry.session.display_name),
        h('div', { class: 'row-sub' }, `${entry.workspace_name} · ${info.label}`),
      ),
    );
  });
}
