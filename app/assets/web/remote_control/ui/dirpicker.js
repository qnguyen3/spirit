import { h, clear, replace, icon } from '../dom.js';

function separatorFor(path) {
  return path.indexOf('\\') >= 0 && path.indexOf('/') < 0 ? '\\' : '/';
}

export function pathSegments(path) {
  if (!path) return [];
  const separator = separatorFor(path);
  const parts = path.split(separator).filter((part) => part.length > 0);
  const segments = [];
  const windowsRoot = /^[A-Za-z]:$/.test(parts[0] || '');
  let prefix = windowsRoot ? '' : separator;
  if (windowsRoot) {
    segments.push({ label: parts[0], path: `${parts[0]}${separator}` });
    prefix = `${parts[0]}${separator}`;
    parts.shift();
  } else {
    segments.push({ label: separator, path: separator });
  }
  for (const part of parts) {
    prefix = prefix.endsWith(separator) ? `${prefix}${part}` : `${prefix}${separator}${part}`;
    segments.push({ label: part, path: prefix });
  }
  return segments;
}

export function createDirPicker(ctx, options) {
  const settings = options || {};
  const breadcrumb = h('nav', { class: 'breadcrumb', 'aria-label': 'Folder path' });
  const list = h('ul', { class: 'dirpicker-list' });
  const status = h('p', { class: 'field-hint', role: 'status' });
  const useButton = h(
    'button',
    { class: 'button button-primary button-block', type: 'button', onClick: choose },
    icon('check'),
    'Use this folder',
  );
  const element = h(
    'div',
    { class: 'dirpicker' },
    breadcrumb,
    list,
    status,
    useButton,
  );

  let currentPath = null;
  let loading = false;

  function choose() {
    if (!currentPath || typeof settings.onPick !== 'function') return;
    settings.onPick(currentPath);
  }

  function renderBreadcrumb() {
    clear(breadcrumb);
    const segments = pathSegments(currentPath);
    segments.forEach((segment, index) => {
      if (index > 0) breadcrumb.appendChild(h('span', { class: 'breadcrumb-sep', 'aria-hidden': 'true' }, '›'));
      breadcrumb.appendChild(
        h(
          'button',
          {
            class: 'breadcrumb-item',
            type: 'button',
            onClick: () => load(segment.path),
            'aria-current': index === segments.length - 1 ? 'true' : null,
          },
          segment.label,
        ),
      );
    });
  }

  function renderEntries(data) {
    clear(list);
    if (data.parent) {
      list.appendChild(
        h(
          'li',
          {},
          h(
            'button',
            { class: 'row row-button', type: 'button', onClick: () => load(data.parent) },
            h('span', { class: 'row-lead' }, icon('arrow-up')),
            h('span', { class: 'row-texts' }, h('span', { class: 'row-title' }, 'Parent folder')),
          ),
        ),
      );
    }
    if (!data.entries || data.entries.length === 0) {
      list.appendChild(h('li', { class: 'section-empty' }, 'No subfolders here.'));
      return;
    }
    for (const entry of data.entries) {
      list.appendChild(
        h(
          'li',
          {},
          h(
            'button',
            { class: 'row row-button', type: 'button', onClick: () => load(entry.path) },
            h('span', { class: 'row-lead' }, icon(entry.is_git_repo ? 'git-branch' : 'folder')),
            h(
              'span',
              { class: 'row-texts' },
              h('span', { class: 'row-title' }, entry.name),
              entry.is_git_repo ? h('span', { class: 'row-sub' }, 'Git repository') : null,
            ),
            h('span', { class: 'row-trail' }, icon('chevron-right', 'icon-sm')),
          ),
        ),
      );
    }
  }

  async function load(path) {
    if (loading) return;
    loading = true;
    useButton.disabled = true;
    status.textContent = 'Loading folders…';
    try {
      const data = await ctx.command('fs.list_dirs', path ? { path } : {});
      currentPath = data.path;
      renderBreadcrumb();
      renderEntries(data);
      status.textContent = currentPath;
      if (typeof settings.onPathChange === 'function') settings.onPathChange(currentPath);
    } catch (error) {
      replace(status, error && error.message ? error.message : 'Could not read that folder.');
      status.classList.add('field-error');
    } finally {
      loading = false;
      useButton.disabled = !currentPath;
    }
  }

  load(settings.initialPath || null);

  return {
    element,
    load,
    path() {
      return currentPath;
    },
  };
}
