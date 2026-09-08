import { h, clear, icon, relativeTime } from '../dom.js';

const THEMES = [
  { id: 'system', label: 'System' },
  { id: 'light', label: 'Light' },
  { id: 'dark', label: 'Dark' },
];

export function render(host, ctx, state) {
  clear(host);
  const pad = h('div', { class: 'screen-pad' });
  host.appendChild(h('div', { class: 'screen' }, pad));

  pad.appendChild(appearanceSection(ctx, state));
  pad.appendChild(alertsSection(ctx, state));
  pad.appendChild(connectionSection(ctx, state));
  pad.appendChild(pairAnotherDeviceSection(state));
  pad.appendChild(devicesSection(ctx, state));
  pad.appendChild(accountSection(ctx));

  if ((state.ui.devicesState || 'idle') === 'idle') loadDevices(ctx);
}

function sectionOf(title, ...children) {
  return h(
    'div',
    { class: 'section' },
    h('div', { class: 'section-head' }, h('h2', { class: 'section-title' }, title)),
    ...children,
  );
}

function toggleRow(label, description, pressed, onToggle) {
  return h(
    'button',
    {
      class: 'switch-row',
      type: 'button',
      'aria-pressed': pressed ? 'true' : 'false',
      onClick: onToggle,
    },
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, label),
      h('div', { class: 'row-sub' }, description),
    ),
    h('span', { class: 'switch-track' }, h('span', { class: 'switch-knob' })),
  );
}

function appearanceSection(ctx, state) {
  const theme = state.prefs.theme || 'system';
  const tabs = h('div', { class: 'tab-row', role: 'tablist', 'aria-label': 'Theme' });
  for (const option of THEMES) {
    tabs.appendChild(
      h(
        'button',
        {
          class: 'tab-row-item',
          type: 'button',
          role: 'tab',
          'aria-selected': theme === option.id ? 'true' : 'false',
          onClick: () => ctx.store.patchPrefs({ theme: option.id }),
        },
        option.label,
      ),
    );
  }
  const scale = state.prefs.fontScale || 1;
  return sectionOf(
    'Appearance',
    tabs,
    h(
      'button',
      {
        class: 'row row-button',
        type: 'button',
        onClick: () => ctx.openSheet('font-size', {}),
      },
      h('span', { class: 'row-lead' }, icon('edit')),
      h(
        'div',
        { class: 'row-texts' },
        h('div', { class: 'row-title' }, 'Terminal zoom'),
        h('div', { class: 'row-sub' }, `${Math.round(scale * 100)}% of the fitted size`),
      ),
      h('span', { class: 'row-trail' }, icon('chevron-right', 'icon-sm')),
    ),
  );
}

function alertsSection(ctx, state) {
  return sectionOf(
    'Alerts',
    h(
      'div',
      { class: 'row-group' },
      toggleRow('Sound', 'Play a tone when an agent needs input', state.prefs.sound, () =>
        ctx.store.patchPrefs({ sound: !state.prefs.sound }),
      ),
      toggleRow('Vibration', 'Buzz this device when an agent needs input', state.prefs.vibrate, () =>
        ctx.store.patchPrefs({ vibrate: !state.prefs.vibrate }),
      ),
    ),
  );
}

function infoRow(label, value) {
  return h(
    'div',
    { class: 'row' },
    h('div', { class: 'row-texts' }, h('div', { class: 'row-title' }, label)),
    h('span', { class: 'row-trail' }, value),
  );
}

function connectionSection(ctx, state) {
  const hello = state.hello || {};
  const server = state.snapshot ? state.snapshot.server : null;
  const rows = h(
    'div',
    { class: 'row-group' },
    infoRow('Status', state.connection.status),
    infoRow('Spirit version', hello.app_version || 'unknown'),
    infoRow('Protocol', String(hello.protocol === undefined ? '-' : hello.protocol)),
    infoRow('Instance', hello.instance_id || '-'),
    server ? infoRow('Connected clients', String(server.connected_clients)) : null,
    server ? infoRow('LAN access', server.lan_access ? 'On' : 'Off') : null,
    h(
      'button',
      { class: 'row row-button', type: 'button', onClick: () => ctx.ws.retry() },
      h('span', { class: 'row-lead' }, icon('refresh')),
      h('div', { class: 'row-texts' }, h('div', { class: 'row-title' }, 'Reconnect now')),
    ),
  );
  return sectionOf('Connection', rows);
}

function pairAnotherDeviceSection(state) {
  const server = state.snapshot ? state.snapshot.server : null;
  const body = h('div', { class: 'pairing-panel' });
  body.appendChild(
    h(
      'p',
      { class: 'section-note' },
      server && server.lan_access
        ? 'Scan this with another device on the same network to pair it.'
        : 'This code points at this machine only. Turn on "Allow access from other devices on this network" in the desktop Settings to pair a phone.',
    ),
  );
  body.appendChild(
    h('img', {
      class: 'pairing-qr',
      src: '/api/v1/pairing-qr.svg',
      width: '220',
      height: '220',
      alt: 'QR code containing this instance\u2019s pairing link',
    }),
  );
  body.appendChild(
    h(
      'p',
      { class: 'section-note section-warning' },
      'The code contains the access token. Treat it like a password.',
    ),
  );
  return sectionOf('Pair another device', body);
}

function devicesSection(ctx, state) {
  const devicesState = state.ui.devicesState || 'idle';
  const rows = h('div', { class: 'row-group' });
  if (devicesState === 'loading' || devicesState === 'idle') {
    rows.appendChild(
      h(
        'p',
        { class: 'section-empty' },
        h('span', { class: 'spinner', role: 'img', 'aria-label': 'Loading' }),
      ),
    );
  } else if (devicesState === 'error') {
    rows.appendChild(h('p', { class: 'section-empty' }, state.ui.devicesError || 'Could not load devices.'));
  } else if (!state.ui.devices || !state.ui.devices.length) {
    rows.appendChild(h('p', { class: 'section-empty' }, 'No paired devices recorded.'));
  } else {
    for (const device of state.ui.devices) rows.appendChild(deviceRow(ctx, device));
  }
  return sectionOf(
    'Paired devices',
    rows,
    h(
      'button',
      { class: 'button button-small', type: 'button', onClick: () => reloadDevices(ctx) },
      icon('refresh', 'icon-sm'),
      'Refresh',
    ),
  );
}

function deviceRow(ctx, device) {
  const row = h(
    'div',
    { class: 'row' },
    h(
      'span',
      { class: 'row-lead' },
      icon('phone'),
      h('span', {
        class: `dot ${device.connected ? 'dot-done' : 'dot-offline'}`,
        role: 'img',
        'aria-label': device.connected ? 'Connected' : 'Not connected',
      }),
    ),
    h(
      'div',
      { class: 'row-texts' },
      h('div', { class: 'row-title' }, device.label),
      h(
        'div',
        { class: 'row-sub' },
        [
          device.current ? 'This device' : null,
          device.connected ? 'Connected' : `Last seen ${relativeTime(device.last_seen_ts)}`,
          `Paired ${relativeTime(device.created_ts)}`,
        ]
          .filter(Boolean)
          .join(' · '),
      ),
    ),
  );
  const menuButton = h(
    'button',
    {
      class: 'icon-button',
      type: 'button',
      'aria-label': `Actions for ${device.label}`,
      onClick: () =>
        ctx.openSheet('menu', {
          title: device.label,
          items: [
            {
              label: 'Rename',
              icon: 'edit',
              onSelect: () =>
                ctx.openSheet('rename-device', {
                  deviceId: device.id,
                  label: device.label,
                  onDone: () => reloadDevices(ctx),
                }),
            },
            {
              label: device.current ? 'Revoke and sign out' : 'Revoke access',
              icon: 'trash',
              danger: true,
              onSelect: async () => {
                const confirmed = await ctx.confirm({
                  title: `Revoke ${device.label}?`,
                  message: device.current
                    ? 'This browser signs out immediately and needs the pairing token again.'
                    : 'That browser signs out immediately and needs the pairing token again.',
                  confirmLabel: 'Revoke',
                  danger: true,
                });
                if (!confirmed) return;
                await ctx.command('devices.revoke', { device_id: device.id });
                if (device.current) {
                  window.location.assign('/');
                  return;
                }
                reloadDevices(ctx);
              },
            },
          ],
        }),
    },
    icon('kebab'),
  );
  return h('div', { class: 'row-with-menu' }, row, menuButton);
}

function accountSection(ctx) {
  return sectionOf(
    'This browser',
    h(
      'button',
      { class: 'button button-danger button-block', type: 'button', onClick: () => signOut(ctx) },
      icon('close'),
      'Sign out',
    ),
    h(
      'p',
      { class: 'field-hint' },
      'Signing out clears the pairing cookie on this device. You need the token from Settings on the desktop to pair again.',
    ),
  );
}

async function signOut(ctx) {
  const confirmed = await ctx.confirm({
    title: 'Sign out of Spirit?',
    message: 'This browser loses access until you pair it again with the token.',
    confirmLabel: 'Sign out',
    danger: true,
  });
  if (!confirmed) return;
  try {
    await window.fetch('/api/v1/logout', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'X-Spirit-Remote': '1' },
    });
  } catch (error) {
    ctx.toast('Sign out failed. Check the connection and try again.', 'error');
    return;
  }
  window.location.assign('/');
}

export async function loadDevices(ctx) {
  if ((ctx.store.get().ui.devicesState || 'idle') === 'loading') return;
  ctx.store.patchUi({ devicesState: 'loading', devicesError: null });
  try {
    const data = await ctx.ws.command('devices.list', {});
    ctx.store.patchUi({ devices: (data && data.devices) || [], devicesState: 'ready' });
  } catch (error) {
    ctx.store.patchUi({
      devicesState: 'error',
      devicesError: error && error.message ? error.message : 'Could not load devices.',
    });
  }
}

export function reloadDevices(ctx) {
  ctx.store.patchUi({ devicesState: 'idle' });
  loadDevices(ctx);
}
