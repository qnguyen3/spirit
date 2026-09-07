import { h, clear, icon, basename } from '../dom.js';
import { createDirPicker } from './dirpicker.js';

const FOCUSABLE =
  'button:not([disabled]), a[href], input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

const DEFERRED_TIMEOUT_MS = 120000;

let mounted = null;

export function renderSheetHost(host, ctx, state) {
  const sheet = state.ui.sheet;
  if (!sheet) {
    if (mounted) {
      mounted.dispose();
      mounted = null;
    }
    clear(host);
    return;
  }
  if (mounted && mounted.sheet === sheet) {
    if (mounted.update) mounted.update(state);
    return;
  }
  if (mounted) mounted.dispose();
  clear(host);
  mounted = buildSheet(ctx, sheet, state);
  host.appendChild(mounted.node);
  mounted.focusFirst();
}

function buildSheet(ctx, sheet, state) {
  const builder = SHEET_KINDS[sheet.kind] || SHEET_KINDS.menu;
  const props = sheet.props || {};
  const content = builder(ctx, props, state);
  const titleId = `sheet-title-${sheet.kind}`;
  const closeButton = h(
    'button',
    { class: 'icon-button', type: 'button', 'aria-label': 'Close', onClick: () => ctx.closeSheet() },
    icon('close'),
  );
  const bodyClass = content.flush ? 'sheet-body sheet-body-flush' : 'sheet-body';
  const dialog = h(
    'div',
    {
      class: 'sheet',
      role: 'dialog',
      'aria-modal': 'true',
      'aria-labelledby': titleId,
      onClick: (event) => event.stopPropagation(),
    },
    h(
      'header',
      { class: 'sheet-head' },
      h('h2', { class: 'sheet-title', id: titleId }, content.title),
      closeButton,
    ),
    h('div', { class: bodyClass }, content.body),
    content.actions && content.actions.length ? h('footer', { class: 'sheet-foot' }, content.actions) : null,
  );
  const backdrop = h(
    'div',
    { class: 'sheet-backdrop', onClick: () => ctx.closeSheet() },
    dialog,
  );

  const previousFocus = document.activeElement;

  function onKeyDown(event) {
    if (event.key === 'Escape') {
      event.preventDefault();
      ctx.closeSheet();
      return;
    }
    if (event.key !== 'Tab') return;
    const targets = Array.from(dialog.querySelectorAll(FOCUSABLE)).filter(
      (node) => node.offsetParent !== null || node === document.activeElement,
    );
    if (targets.length === 0) return;
    const first = targets[0];
    const last = targets[targets.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  window.addEventListener('keydown', onKeyDown, true);

  return {
    sheet,
    node: backdrop,
    update: content.update,
    focusFirst() {
      const target = dialog.querySelector('input, textarea, select') || dialog.querySelector(FOCUSABLE);
      if (target) target.focus();
    },
    dispose() {
      window.removeEventListener('keydown', onKeyDown, true);
      if (content.dispose) content.dispose();
      if (previousFocus && typeof previousFocus.focus === 'function' && document.contains(previousFocus)) {
        previousFocus.focus();
      }
    },
  };
}

function field(labelText, control, hintText) {
  const id = control.id || `field-${Math.random().toString(36).slice(2, 9)}`;
  control.id = id;
  return h(
    'label',
    { class: 'field', for: id },
    h('span', { class: 'field-label' }, labelText),
    control,
    hintText ? h('span', { class: 'field-hint' }, hintText) : null,
  );
}

function errorLine() {
  return h('p', { class: 'field-error', role: 'alert' });
}

function showError(node, error) {
  node.textContent = error && error.message ? error.message : 'Something went wrong.';
}

function busyButton(label, iconName, handler, className) {
  const button = h(
    'button',
    { class: className || 'button button-primary', type: 'button' },
    iconName ? icon(iconName) : null,
    h('span', { text: label }),
  );
  button.addEventListener('click', async () => {
    if (button.disabled) return;
    button.disabled = true;
    try {
      await handler();
    } finally {
      button.disabled = false;
    }
  });
  return button;
}

function agentOptions(state) {
  const agents = (state.snapshot && state.snapshot.agents) || [];
  return agents.slice().sort((left, right) => {
    if (left.installed !== right.installed) return left.installed ? -1 : 1;
    return left.display_name.localeCompare(right.display_name);
  });
}

function agentChooser(state, onChange, includeNone) {
  const options = agentOptions(state);
  const list = h('div', { class: 'row-group', role: 'radiogroup', 'aria-label': 'Agent' });
  let selected = includeNone ? null : (options[0] ? options[0].index : null);

  function paint() {
    clear(list);
    if (includeNone) list.appendChild(entryRow(null, 'No agent', 'Start a plain shell', true));
    for (const agent of options) {
      list.appendChild(
        entryRow(
          agent.index,
          agent.display_name,
          agent.installed ? 'Installed' : 'Not installed on this machine',
          agent.installed,
          agent.brand_color,
        ),
      );
    }
  }

  function entryRow(index, label, hint, enabled, brandColor) {
    const active = selected === index;
    return h(
      'button',
      {
        class: 'row row-button',
        type: 'button',
        role: 'radio',
        'aria-checked': active ? 'true' : 'false',
        'data-active': active ? 'true' : null,
        disabled: !enabled,
        onClick: () => {
          selected = index;
          paint();
          if (onChange) onChange(selected);
        },
      },
      h(
        'span',
        { class: 'row-lead', style: brandColor ? { color: brandColor } : null },
        icon(index === null ? 'terminal' : 'robot'),
      ),
      h(
        'span',
        { class: 'row-texts' },
        h('span', { class: 'row-title' }, label),
        h('span', { class: 'row-sub' }, hint),
      ),
      active ? h('span', { class: 'row-trail' }, icon('check', 'icon-sm')) : null,
    );
  }

  paint();
  if (onChange) onChange(selected);
  return {
    element: list,
    selection() {
      return selected;
    },
    supportsYolo() {
      const found = options.find((agent) => agent.index === selected);
      return Boolean(found && found.supports_yolo);
    },
  };
}

function approvalChooser() {
  let mode = 'normal';
  const normal = h('button', { class: 'button', type: 'button', 'aria-pressed': 'true' }, 'Ask first');
  const yolo = h('button', { class: 'button', type: 'button', 'aria-pressed': 'false' }, 'Auto approve');
  function paint() {
    normal.setAttribute('aria-pressed', mode === 'normal' ? 'true' : 'false');
    yolo.setAttribute('aria-pressed', mode === 'yolo' ? 'true' : 'false');
    normal.classList.toggle('button-primary', mode === 'normal');
    yolo.classList.toggle('button-primary', mode === 'yolo');
  }
  normal.addEventListener('click', () => {
    mode = 'normal';
    paint();
  });
  yolo.addEventListener('click', () => {
    mode = 'yolo';
    paint();
  });
  paint();
  return {
    element: h('div', { class: 'button-row' }, normal, yolo),
    mode() {
      return mode;
    },
    setEnabled(enabled) {
      yolo.disabled = !enabled;
      if (!enabled && mode === 'yolo') {
        mode = 'normal';
        paint();
      }
    },
  };
}

function parentPicker(ctx, label, onChange) {
  const display = h('input', {
    class: 'input input-mono',
    type: 'text',
    readonly: true,
    'aria-live': 'polite',
    value: '',
  });
  const pickerHost = h('div', {});
  let open = false;
  let picker = null;
  const toggle = h(
    'button',
    { class: 'button button-small', type: 'button', onClick: () => setOpen(!open) },
    icon('folder', 'icon-sm'),
    'Browse',
  );

  function setOpen(next) {
    open = next;
    clear(pickerHost);
    if (!open) {
      picker = null;
      return;
    }
    picker = createDirPicker(ctx, {
      initialPath: display.value || null,
      onPathChange: (path) => {
        display.value = path;
        if (onChange) onChange(path);
      },
      onPick: (path) => {
        display.value = path;
        if (onChange) onChange(path);
        setOpen(false);
      },
    });
    pickerHost.appendChild(picker.element);
  }

  return {
    element: h(
      'div',
      { class: 'field' },
      h('span', { class: 'field-label' }, label),
      h('div', { class: 'button-row' }, display, toggle),
      pickerHost,
    ),
    value() {
      return display.value;
    },
  };
}

function buildMenuSheet(ctx, props) {
  const list = h('div', { class: 'menu-list' });
  for (const item of props.items || []) {
    if (item.separator) {
      list.appendChild(h('div', { class: 'menu-separator', role: 'separator' }));
      continue;
    }
    list.appendChild(
      h(
        'button',
        {
          class: item.danger ? 'menu-item menu-item-danger' : 'menu-item',
          type: 'button',
          disabled: item.disabled === true,
          onClick: () => {
            ctx.closeSheet();
            if (item.onSelect) item.onSelect();
          },
        },
        icon(item.icon || 'chevron-right'),
        h('span', { class: 'menu-item-label' }, item.label),
        item.hint ? h('span', { class: 'menu-item-hint' }, item.hint) : null,
      ),
    );
  }
  return { title: props.title || 'Actions', body: list, flush: true };
}

function buildConfirmSheet(ctx, props) {
  const cancel = h(
    'button',
    {
      class: 'button',
      type: 'button',
      onClick: () => {
        if (props.onCancel) props.onCancel();
        ctx.closeSheet();
      },
    },
    props.cancelLabel || 'Cancel',
  );
  const confirm = h(
    'button',
    {
      class: props.danger ? 'button button-danger' : 'button button-primary',
      type: 'button',
      onClick: () => {
        if (props.onConfirm) props.onConfirm();
        ctx.closeSheet();
      },
    },
    props.confirmLabel || 'Confirm',
  );
  return {
    title: props.title || 'Are you sure?',
    body: h('p', { class: 'sheet-note' }, props.message || ''),
    actions: [cancel, confirm],
  };
}

function buildNewTerminalSheet(ctx, props) {
  const error = errorLine();
  const target = props.worktreeName
    ? `A new terminal tab opens in ${props.worktreeName}.`
    : 'A new terminal tab opens in this Workspace.';
  const create = busyButton('Create terminal', 'terminal', async () => {
    error.textContent = '';
    try {
      const params = { screen_id: props.screenId };
      if (props.worktreeId) params.worktree_id = props.worktreeId;
      const data = await ctx.command('terminal.create', params);
      ctx.closeSheet();
      if (data && data.terminal_id) ctx.navigate('terminal', { id: data.terminal_id });
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'New terminal',
    body: [h('p', { class: 'sheet-note' }, target), error],
    actions: [create],
  };
}

function buildLaunchAgentSheet(ctx, props, state) {
  const error = errorLine();
  const approval = approvalChooser();
  const chooser = agentChooser(state, () => approval.setEnabled(chooser.supportsYolo()), false);
  const launch = busyButton('Launch agent', 'robot', async () => {
    error.textContent = '';
    const index = chooser.selection();
    if (index === null || index === undefined) {
      error.textContent = 'Pick an agent first.';
      return;
    }
    try {
      const params = {
        screen_id: props.screenId,
        catalog_index: index,
        approval_mode: approval.mode(),
      };
      if (props.worktreeId) params.worktree_id = props.worktreeId;
      const data = await ctx.command('agent.launch', params);
      ctx.closeSheet();
      if (data && data.terminal_id) ctx.navigate('terminal', { id: data.terminal_id });
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'Launch agent',
    body: [
      props.worktreeName ? h('p', { class: 'sheet-note' }, `Runs in ${props.worktreeName}.`) : null,
      chooser.element,
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'Permissions'), approval.element),
      error,
    ],
    actions: [launch],
  };
}

function buildNewWorktreeSheet(ctx, props, state) {
  const error = errorLine();
  const nameInput = h('input', {
    class: 'input',
    type: 'text',
    autocapitalize: 'none',
    autocorrect: 'off',
    spellcheck: false,
    placeholder: 'Leave blank to generate a name',
  });
  const approval = approvalChooser();
  const chooser = agentChooser(state, () => approval.setEnabled(chooser.supportsYolo()), true);
  const create = busyButton('Create worktree', 'git-branch', async () => {
    error.textContent = '';
    try {
      const params = { project_id: props.projectId };
      const name = nameInput.value.trim();
      if (name) params.name = name;
      const index = chooser.selection();
      if (index !== null && index !== undefined) {
        params.agent = { catalog_index: index, approval_mode: approval.mode() };
      }
      const data = await ctx.command('worktree.create', params, { timeoutMs: DEFERRED_TIMEOUT_MS });
      ctx.closeSheet();
      ctx.toast(`Created ${data && data.branch ? data.branch : 'worktree'}`, 'ok');
      ctx.navigate('workspace', { id: props.projectId });
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'New worktree',
    body: [
      field('Branch name', nameInput, 'Spirit creates the branch and checks it out in a linked worktree.'),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'Start an agent'), chooser.element),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'Permissions'), approval.element),
      error,
    ],
    actions: [create],
  };
}

function buildRenameWorktreeSheet(ctx, props) {
  const error = errorLine();
  const input = h('input', {
    class: 'input',
    type: 'text',
    autocapitalize: 'none',
    autocorrect: 'off',
    spellcheck: false,
    value: props.name || '',
  });
  const save = busyButton('Save name', 'check', async () => {
    error.textContent = '';
    const name = input.value.trim();
    if (!name) {
      error.textContent = 'Enter a name.';
      return;
    }
    try {
      await ctx.command('worktree.rename', { worktree_id: props.worktreeId, name });
      ctx.closeSheet();
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'Rename worktree',
    body: [field('Name', input), error],
    actions: [save],
  };
}

function buildRenameProjectSheet(ctx, props) {
  const error = errorLine();
  const input = h('input', { class: 'input', type: 'text', value: props.name || '' });
  const save = busyButton('Save name', 'check', async () => {
    error.textContent = '';
    const name = input.value.trim();
    if (!name) {
      error.textContent = 'Enter a name.';
      return;
    }
    try {
      await ctx.command('project.rename', { project_id: props.projectId, name });
      ctx.closeSheet();
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'Rename Workspace',
    body: [field('Name', input), error],
    actions: [save],
  };
}

function buildRenameDeviceSheet(ctx, props) {
  const error = errorLine();
  const input = h('input', { class: 'input', type: 'text', value: props.label || '' });
  const save = busyButton('Save name', 'check', async () => {
    error.textContent = '';
    const label = input.value.trim();
    if (!label) {
      error.textContent = 'Enter a name.';
      return;
    }
    try {
      await ctx.command('devices.rename', { device_id: props.deviceId, label });
      ctx.closeSheet();
      if (props.onDone) props.onDone();
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'Rename device',
    body: [field('Device name', input), error],
    actions: [save],
  };
}

function buildNewWorkspaceSheet(ctx, props, state) {
  const error = errorLine();
  const panel = h('div', { class: 'section' });
  const progressText = h('p', { class: 'field-hint', role: 'status' });
  const progressFill = h('div', { class: 'progress-fill' });
  const progressTrack = h('div', {
    class: 'progress-track',
    role: 'progressbar',
    'aria-label': 'Clone progress',
  });
  progressTrack.appendChild(progressFill);
  const progress = h('div', { class: 'field', hidden: true }, progressTrack, progressText);
  let tab = props.tab || 'folder';
  let cloneJobId = null;

  const tabs = ['folder', 'clone', 'create'];
  const tabLabels = { folder: 'Add folder', clone: 'Clone', create: 'Create new' };
  const tabRow = h('div', { class: 'tab-row', role: 'tablist', 'aria-label': 'Workspace source' });

  function paintTabs() {
    clear(tabRow);
    for (const name of tabs) {
      tabRow.appendChild(
        h(
          'button',
          {
            class: 'tab-row-item',
            type: 'button',
            role: 'tab',
            'aria-selected': tab === name ? 'true' : 'false',
            onClick: () => {
              tab = name;
              paintTabs();
              paintPanel();
            },
          },
          tabLabels[name],
        ),
      );
    }
  }

  function finish(projectId, message) {
    ctx.closeSheet();
    ctx.toast(message, 'ok');
    if (projectId) ctx.navigate('workspace', { id: projectId });
  }

  function folderPanel() {
    const picker = createDirPicker(ctx, {
      initialPath: null,
      onPick: async (path) => {
        error.textContent = '';
        try {
          const data = await ctx.command('project.register', { path }, { timeoutMs: DEFERRED_TIMEOUT_MS });
          finish(data && data.project_id, `Added ${basename(path)}`);
        } catch (failure) {
          showError(error, failure);
        }
      },
    });
    return [
      h('p', { class: 'sheet-note' }, 'Pick a folder on the computer running Spirit.'),
      picker.element,
    ];
  }

  function clonePanel() {
    const urlInput = h('input', {
      class: 'input input-mono',
      type: 'text',
      inputmode: 'url',
      autocapitalize: 'none',
      autocorrect: 'off',
      spellcheck: false,
      placeholder: 'git@github.com:owner/repo.git',
    });
    const nameInput = h('input', {
      class: 'input',
      type: 'text',
      autocapitalize: 'none',
      autocorrect: 'off',
      spellcheck: false,
      placeholder: 'Defaults to the repository name',
    });
    const parent = parentPicker(ctx, 'Parent folder');
    const cancel = h(
      'button',
      {
        class: 'button button-small',
        type: 'button',
        hidden: true,
        onClick: () => {
          if (cloneJobId) ctx.command('project.clone_cancel', { job_id: cloneJobId });
        },
      },
      'Cancel clone',
    );
    const start = busyButton('Clone repository', 'git-branch', async () => {
      error.textContent = '';
      const url = urlInput.value.trim();
      const parentPath = parent.value().trim();
      if (!url) {
        error.textContent = 'Enter a repository URL.';
        return;
      }
      if (!parentPath) {
        error.textContent = 'Choose a parent folder.';
        return;
      }
      const params = { url, parent: parentPath };
      const directory = nameInput.value.trim();
      if (directory) params.directory_name = directory;
      progress.hidden = false;
      cancel.hidden = false;
      progressText.textContent = 'Starting…';
      try {
        const data = await ctx.command('project.clone', params, { timeoutMs: DEFERRED_TIMEOUT_MS });
        finish(data && data.project_id, 'Repository cloned');
      } catch (failure) {
        showError(error, failure);
        progress.hidden = true;
        cancel.hidden = true;
      }
    });
    return [
      field('Repository URL', urlInput),
      parent.element,
      field('Folder name', nameInput),
      progress,
      h('div', { class: 'button-row' }, start, cancel),
    ];
  }

  function createPanel() {
    const nameInput = h('input', {
      class: 'input',
      type: 'text',
      autocapitalize: 'none',
      autocorrect: 'off',
      spellcheck: false,
      placeholder: 'my-project',
    });
    const parent = parentPicker(ctx, 'Parent folder');
    const start = busyButton('Create Workspace', 'plus', async () => {
      error.textContent = '';
      const name = nameInput.value.trim();
      const parentPath = parent.value().trim();
      if (!name) {
        error.textContent = 'Enter a name.';
        return;
      }
      if (!parentPath) {
        error.textContent = 'Choose a parent folder.';
        return;
      }
      try {
        const data = await ctx.command(
          'project.create',
          { name, parent: parentPath },
          { timeoutMs: DEFERRED_TIMEOUT_MS },
        );
        finish(data && data.project_id, `Created ${name}`);
      } catch (failure) {
        showError(error, failure);
      }
    });
    return [
      field('Workspace name', nameInput, 'Spirit runs git init in the new folder.'),
      parent.element,
      h('div', { class: 'button-row' }, start),
    ];
  }

  function paintPanel() {
    clear(panel);
    const nodes = tab === 'folder' ? folderPanel() : tab === 'clone' ? clonePanel() : createPanel();
    for (const node of nodes) if (node) panel.appendChild(node);
  }

  paintTabs();
  paintPanel();

  return {
    title: 'New Workspace',
    body: [tabRow, panel, error],
    update(nextState) {
      const clone = nextState.ui.clone;
      if (!clone) return;
      cloneJobId = clone.job_id;
      progress.hidden = false;
      progressFill.style.setProperty('width', `${clone.percent === undefined || clone.percent === null ? 20 : clone.percent}%`);
      progressFill.classList.toggle('progress-indeterminate', clone.percent === undefined || clone.percent === null);
      progressText.textContent = clone.message || clone.phase || '';
      progressTrack.setAttribute('aria-valuenow', String(clone.percent === undefined || clone.percent === null ? 0 : clone.percent));
    },
  };
}

function buildDirPickerSheet(ctx, props) {
  const picker = createDirPicker(ctx, {
    initialPath: props.initialPath || null,
    onPick: (path) => {
      ctx.closeSheet();
      if (props.onPick) props.onPick(path);
    },
  });
  return { title: props.title || 'Choose a folder', body: picker.element };
}

function buildPasteSheet(ctx, props) {
  const error = errorLine();
  const textarea = h('textarea', {
    class: 'textarea input-mono',
    rows: 5,
    placeholder: 'Paste here, then send it to the terminal',
    autocapitalize: 'none',
    autocorrect: 'off',
    spellcheck: false,
  });
  const send = busyButton('Send to terminal', 'clipboard', async () => {
    error.textContent = '';
    const value = textarea.value;
    if (!value) {
      error.textContent = 'Nothing to paste yet.';
      return;
    }
    try {
      await ctx.command('terminal.paste', { terminal_id: props.terminalId, text: value });
      ctx.closeSheet();
    } catch (failure) {
      showError(error, failure);
    }
  });
  return {
    title: 'Paste',
    body: [
      h('p', { class: 'sheet-note' }, 'Your browser blocks clipboard reads on plain HTTP, so paste into this box instead.'),
      textarea,
      error,
    ],
    actions: [send],
  };
}

function buildCopySheet(ctx, props) {
  const status = h('p', { class: 'field-hint', role: 'status' });
  const textarea = h('textarea', {
    class: 'textarea input-mono',
    rows: 6,
    readonly: true,
    value: props.text || '',
  });
  const copy = h(
    'button',
    {
      class: 'button button-primary',
      type: 'button',
      onClick: async () => {
        textarea.focus();
        textarea.select();
        if (navigator.clipboard && window.isSecureContext) {
          try {
            await navigator.clipboard.writeText(props.text || '');
            status.textContent = 'Copied.';
            return;
          } catch (failure) {
            status.textContent = 'Copy the selected text manually.';
            return;
          }
        }
        status.textContent = 'Text selected. Use your keyboard or long-press to copy.';
      },
    },
    icon('clipboard'),
    'Copy',
  );
  return {
    title: props.title || 'Copy',
    body: [textarea, status],
    actions: [copy],
  };
}

function buildFontSizeSheet(ctx) {
  const label = h('p', { class: 'sheet-note', role: 'status' });
  const steps = [0.75, 0.85, 1, 1.15, 1.3, 1.5];

  function current() {
    return ctx.store.get().prefs.fontScale || 1;
  }

  function move(direction) {
    const index = steps.indexOf(current());
    const base = index < 0 ? steps.indexOf(1) : index;
    const next = Math.min(steps.length - 1, Math.max(0, base + direction));
    ctx.store.patchPrefs({ fontScale: steps[next] });
  }

  function paint() {
    label.textContent = `Terminal text at ${Math.round(current() * 100)}% of the fitted size.`;
  }

  paint();
  return {
    title: 'Font size',
    body: [
      label,
      h(
        'div',
        { class: 'button-row' },
        h('button', { class: 'button', type: 'button', onClick: () => move(-1) }, 'Smaller'),
        h('button', { class: 'button', type: 'button', onClick: () => move(1) }, 'Larger'),
        h(
          'button',
          { class: 'button button-quiet', type: 'button', onClick: () => ctx.store.patchPrefs({ fontScale: 1 }) },
          'Reset',
        ),
      ),
      h('p', { class: 'field-hint' }, 'Spirit picks the largest size that fits the desktop column count; this scales that result.'),
    ],
    update: paint,
  };
}

const SHEET_KINDS = {
  menu: buildMenuSheet,
  confirm: buildConfirmSheet,
  'new-terminal': buildNewTerminalSheet,
  'launch-agent': buildLaunchAgentSheet,
  'new-worktree': buildNewWorktreeSheet,
  'rename-worktree': buildRenameWorktreeSheet,
  'rename-project': buildRenameProjectSheet,
  'rename-device': buildRenameDeviceSheet,
  'new-workspace': buildNewWorkspaceSheet,
  'dir-picker': buildDirPickerSheet,
  paste: buildPasteSheet,
  copy: buildCopySheet,
  'font-size': buildFontSizeSheet,
};

const LONG_PRESS_MS = 500;

export function attachLongPress(node, handler) {
  let timer = null;
  let fired = false;

  function stop() {
    if (timer !== null) window.clearTimeout(timer);
    timer = null;
  }

  node.addEventListener('pointerdown', (event) => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    fired = false;
    stop();
    timer = window.setTimeout(() => {
      fired = true;
      if (navigator.vibrate) navigator.vibrate(10);
      handler();
    }, LONG_PRESS_MS);
  });
  node.addEventListener('pointerup', stop);
  node.addEventListener('pointercancel', stop);
  node.addEventListener('pointerleave', stop);
  node.addEventListener('contextmenu', (event) => {
    event.preventDefault();
    stop();
    if (!fired) handler();
  });
  node.addEventListener(
    'click',
    (event) => {
      if (!fired) return;
      fired = false;
      event.preventDefault();
      event.stopPropagation();
    },
    true,
  );
  return stop;
}
