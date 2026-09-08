#!/usr/bin/env node
// Run with Playwright on NODE_PATH; uses the locally installed Chrome browser.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { join, extname } from 'node:path';

const { chromium } = createRequire(import.meta.url)('playwright');
const assetRoot = fileURLToPath(new URL('../app/assets/web/remote_control/', import.meta.url));
const server = createServer(async (request, response) => {
  const path = request.url === '/' ? 'index.html' : request.url.replace(/^\/assets\/test\//, '');
  try {
    const bytes = await readFile(join(assetRoot, path));
    response.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' }[extname(path)] || 'application/octet-stream');
    response.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'");
    response.end(path === 'index.html' ? bytes.toString().replaceAll('{{BUILD_ID}}', 'test') : bytes);
  } catch {
    response.statusCode = 404;
    response.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ channel: 'chrome', headless: true });
browser.on("disconnected", () => server.close());
const artifacts = process.env.REMOTE_CONTROL_TEST_ARTIFACTS;
if (artifacts) await mkdir(artifacts, { recursive: true });

async function fixture(page) {
  await page.addInitScript(() => {
    const terminalTab = (id) => ({
      id: `tab-${id}`, title: `Terminal ${id}`, kind: 'terminal', worktree_id: 'wt',
      pinned: false, agent_summary: 'none', focused_pane_id: `pane-${id}`,
      panes: [{ id: `pane-${id}`, kind: 'terminal', title: 'Terminal', terminal: {
        terminal_id: id, title: `Terminal ${id}`, cwd: '/repo', mode: 'prompt', cols: 80, rows: 24, read_only: false,
      } }],
    });
    const screen = { id: 'screen', window_id: 'window', project_id: 'project', name: 'Spirit', active_tab_id: 'tab-1', sections: [{ title: 'main', worktree_id: 'wt', tabs: [terminalTab('1')] }] };
    const snapshot = {
      version: 1, instance_id: 'test', active_window_id: 'window',
      windows: [{ id: 'window', active_screen_id: 'screen', screens: [screen] }],
      projects: [{ id: 'project', name: 'Spirit', kind: 'git', root_path: '/repo', primary_branch: 'main', screen_id: 'screen', open_in_window_id: 'window', counts: { working: 0, needs_attention: 0 }, worktrees: [{ id: 'wt', project_id: 'project', name: 'main', branch: 'main', kind: 'primary', path: '/repo', open_tab_count: 1, agent_summary: 'none' }] }],
      sessions: [], server: { connected_clients: 1, lan_access: false },
      features: { ade_workspaces: true, session_history: false },
      agents: [{ index: 0, display_name: 'Codex', installed: true, supports_yolo: true, brand_color: '#ffffff' }, { index: 1, display_name: 'Unavailable agent', installed: false, supports_yolo: false }],
    };
    let nextTerminal = 2;
    window.commands = [];
    window.failCommand = null;
    class Socket extends EventTarget {
      static OPEN = 1;
      readyState = 1;
      constructor() {
        super();
        window.testSocket = this;
        setTimeout(() => {
          this.dispatchEvent(new Event('open'));
          this.sendMessage({ type: 'hello', protocol: 1, instance_id: 'test', client_id: 'c1', capabilities: [] });
          this.pushState();
        });
      }
      sendMessage(message) { this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(message) })); }
      pushState() { this.sendMessage({ type: 'state', version: ++snapshot.version, snapshot }); }
      closeTerminal() { screen.sections[0].tabs = []; this.pushState(); }
      send(raw) {
        const command = JSON.parse(raw);
        if (command.type === 'ping') { this.sendMessage({ type: 'pong' }); return; }
        window.commands.push(command);
        setTimeout(() => {
          if (window.failCommand === command.name) {
            this.sendMessage({ type: 'result', id: command.id, ok: false, error: { code: 'conflict', message: 'Test failure: retry this action.' } });
            return;
          }
          let data = {};
          if (command.name === 'terminal.create' || command.name === 'agent.launch') {
            const id = String(nextTerminal++);
            screen.sections[0].tabs.push(terminalTab(id));
            data = { terminal_id: id, tab_id: `tab-${id}` };
          }
          if (command.name === 'worktree.create') data = { branch: command.params.name || 'generated-branch' };
          if (command.name === 'terminal.selection') data = { text: 'Selected desktop output' };
          if (command.name === 'terminal.frame') {
            const canvas = document.createElement('canvas');
            canvas.width = 800; canvas.height = 450;
            const graphics = canvas.getContext('2d');
            graphics.fillStyle = '#10151b'; graphics.fillRect(0, 0, 800, 450);
            graphics.fillStyle = '#78ddaa'; graphics.font = '20px monospace'; graphics.fillText('Desktop frame · exact pixels', 24, 40);
            graphics.strokeStyle = '#78ddaa'; graphics.strokeRect(24, 70, 750, 300);
            data = { image: canvas.toDataURL().split(',')[1], width: 800, height: 450 };
          }
          this.sendMessage({ type: 'result', id: command.id, ok: true, data });
          if (['terminal.create', 'agent.launch', 'worktree.create'].includes(command.name)) this.pushState();
        }, 10);
      }
      close() { this.readyState = 3; this.dispatchEvent(new CloseEvent('close')); }
    }
    window.WebSocket = Socket;
  });
  await page.goto(`${origin}/#/w/project`);
  await page.getByRole('button', { name: 'New terminal', exact: true }).waitFor();
}

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 950 } });
  page.setDefaultTimeout(10000);
  page.setDefaultNavigationTimeout(10000);
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await fixture(page);
  await page.getByRole('button', { name: 'Launch agent', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('radio', { name: /Codex/ }).waitFor();
  assert.equal(await dialog.getByRole('button', { name: 'Auto approve' }).isEnabled(), true);
  assert.equal(await dialog.getByRole('radio', { name: /Unavailable agent/ }).isDisabled(), true);
  await dialog.getByRole('button', { name: 'Launch agent', exact: true }).click();
  await page.waitForURL('**/#/t/2');
  await page.waitForFunction(() => document.querySelector('.terminal-mirror')?.width === 800 && document.querySelector('.terminal-mirror-status')?.hidden);
  const canvas = page.locator('.terminal-mirror');
  await canvas.click({ position: { x: 100, y: 100 } });
  await page.keyboard.type('echo test');
  await page.keyboard.press('Enter');
  await page.mouse.wheel(0, 120);
  await page.waitForFunction(() => window.commands.some((command) => command.name === 'terminal.interact' && command.params.kind === 'scroll'));
  assert.equal(await page.evaluate(() => window.commands.some((command) => command.name === 'terminal.interact' && command.params.key === 'enter')), true);
  if (artifacts) await page.screenshot({ path: join(artifacts, 'desktop-mirror.png') });
  console.log('PASS agent picker, navigation, native frame display, keyboard and scroll');
  await page.getByRole('button', { name: 'More actions' }).click();
  await dialog.getByRole('button', { name: 'Terminal zoom' }).click();
  await dialog.getByRole('heading', { name: 'Terminal zoom' }).waitFor();
  const originalWidth = await canvas.evaluate((element) => element.clientWidth);
  await dialog.getByRole('button', { name: 'Larger' }).click();
  await page.waitForFunction((width) => document.querySelector('.terminal-mirror').clientWidth === Math.round(width * 1.25), originalWidth);
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await dialog.waitFor({ state: 'detached' });
  console.log('PASS replacing an action menu with a zoom sheet');
  await page.getByRole('button', { name: 'More actions' }).click();
  await dialog.getByRole('button', { name: 'Copy selection' }).click();
  await dialog.getByRole('heading', { name: 'Copy selection' }).waitFor();
  assert.equal(await dialog.locator('textarea').inputValue(), 'Selected desktop output');
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await dialog.waitFor({ state: 'detached' });
  console.log('PASS native selection is available in the copy sheet');

  await page.goto(`${origin}/#/w/project`);
  await page.getByRole('button', { name: 'New terminal', exact: true }).click();
  await dialog.getByRole('button', { name: 'Create terminal' }).click();
  await page.waitForURL('**/#/t/3');
  console.log('PASS new terminal stays on the created terminal after sheet closes');

  await page.goto(`${origin}/#/w/project`);
  await page.getByRole('button', { name: 'New worktree', exact: true }).click();
  await dialog.getByLabel('Branch name').fill('remote-smoke');
  await page.evaluate(() => { window.failCommand = 'worktree.create'; });
  await dialog.getByRole('button', { name: 'Create worktree', exact: true }).click();
  await dialog.getByRole('alert').filter({ hasText: 'Test failure' }).waitFor();
  assert.equal(await dialog.getByLabel('Branch name').inputValue(), 'remote-smoke');
  await page.evaluate(() => { window.failCommand = null; });
  await dialog.getByRole('button', { name: 'Create worktree', exact: true }).click();
  await dialog.waitFor({ state: 'detached' });
  await page.waitForURL('**/#/w/project');
  console.log('PASS worktree form, failure feedback, retained draft and retry');

  await page.getByRole('button', { name: 'Launch agent', exact: true }).click();
  await page.goBack();
  await dialog.waitFor({ state: 'detached' });
  assert.match(page.url(), /#\/w\/project$/);
  console.log('PASS browser Back dismisses a sheet without leaving the workspace');

  await page.goto(`${origin}/#/t/1`);
  await page.waitForFunction(() => document.querySelector('.terminal-mirror-status')?.hidden);
  await page.evaluate(() => { window.failCommand = 'terminal.frame'; });
  await page.getByRole('button', { name: 'Show terminal again' }).waitFor();
  const before = await page.evaluate(() => window.commands.filter((command) => command.name === 'terminal.interact').length);
  await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(() => window.commands.filter((command) => command.name === 'terminal.interact').length), before);
  await page.evaluate(() => { window.failCommand = null; });
  await page.getByRole('button', { name: 'Show terminal again' }).click();
  await page.waitForFunction(() => document.querySelector('.terminal-mirror-status')?.hidden);
  await page.evaluate(() => window.testSocket.closeTerminal());
  await page.locator('.terminal-foot').getByRole('button', { name: 'Back to Sessions' }).waitFor();
  console.log('PASS stalled display blocks stale input, recovers, and handles terminal closure');

  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
  mobile.on('pageerror', (error) => errors.push(error.message));
  await fixture(mobile);
  await mobile.getByRole('button', { name: 'Launch agent', exact: true }).click();
  await mobile.getByRole('dialog').getByRole('button', { name: 'Launch agent', exact: true }).click();
  await mobile.waitForURL('**/#/t/2');
  await mobile.waitForFunction(() => document.querySelector('.terminal-mirror-status')?.hidden);
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await mobile.locator('.terminal-mirror').evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    const pointer = { pointerType: 'touch', pointerId: 1, clientX: bounds.x + 100, clientY: bounds.y + 120 };
    element.dispatchEvent(new PointerEvent('pointerdown', pointer));
    element.dispatchEvent(new PointerEvent('pointermove', { ...pointer, clientY: pointer.clientY - 40 }));
    element.dispatchEvent(new PointerEvent('pointerup', pointer));
  });
  await mobile.waitForFunction(() => window.commands.some((command) => command.name === 'terminal.interact' && command.params.kind === 'scroll' && command.params.delta_y === 40));
  if (artifacts) await mobile.screenshot({ path: join(artifacts, 'mobile-mirror.png') });
  console.log('PASS mobile sheet, launch, frame sizing and no page overflow');
  assert.deepEqual(errors, []);
  console.log('PASS no uncaught browser errors');
} finally {
  await browser.close();
  server.close();
}
