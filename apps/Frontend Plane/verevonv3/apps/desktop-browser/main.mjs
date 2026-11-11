import { app, BrowserWindow, ipcMain, session, WebContentsView } from 'electron';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import {
  createDesktopBrowser,
  prepareDesktopBrowserSession,
} from '../../../../Ingestion Plane/Quarry-v2/adapters/desktop-browser/index.mjs';

const application = new URL(process.env.VEREVON_APP_URL || 'http://localhost:5173/dashboard');
const developmentHost = ['localhost', '127.0.0.1', '[::1]'].includes(application.hostname);
if (application.username || application.password ||
    (application.protocol !== 'https:' && !(developmentHost && application.protocol === 'http:'))) {
  throw new Error('VEREVON_APP_URL must be HTTPS or a loopback development URL.');
}
if (process.env.VEREVON_DESKTOP_DATA_PATH) app.setPath('userData', process.env.VEREVON_DESKTOP_DATA_PATH);

const channel = 'verevon:desktop-browser:';
const webPreferences = { sandbox: true, contextIsolation: true, nodeIntegration: false };
let window;
let guest;
let guestCreation;
let viewport;
let closed = false;
let reset = Promise.resolve();

function layoutGuest() {
  if (!guest || !window || closed) return;
  const { view } = guest;
  if (!viewport) {
    view.setVisible(false);
    return;
  }
  const [windowWidth, windowHeight] = window.getContentSize();
  const zoom = window.webContents.getZoomFactor();
  const { x, y, width, height } = viewport;
  const left = Math.max(0, Math.round(x * zoom));
  const top = Math.max(0, Math.round(y * zoom));
  const right = Math.min(windowWidth, Math.round((x + width) * zoom));
  const bottom = Math.min(windowHeight, Math.round((y + height) * zoom));
  const visible = right > left && bottom > top;
  if (visible) view.setBounds({ x: left, y: top, width: right - left, height: bottom - top });
  view.setVisible(visible);
}

function publish(state) {
  if (!closed && window && !window.webContents.isDestroyed()) {
    window.webContents.send(`${channel}event`, state);
  }
}

async function createGuest() {
  const browserSession = session.fromPartition(`quarry-desktop-${randomUUID()}`, { cache: false });
  const proxy = await prepareDesktopBrowserSession(browserSession);
  if (closed) {
    proxy.close();
    throw new Error('Nettleseren er lukket.');
  }
  try {
    const view = new WebContentsView({ webPreferences: { ...webPreferences, session: browserSession } });
    view.setVisible(false);
    const browser = createDesktopBrowser(view.webContents, publish);
    window.contentView.addChildView(view);
    guest = { view, browser, browserSession, proxy };
    layoutGuest();
    return guest;
  } catch (error) {
    proxy.close();
    throw error;
  }
}

async function getGuest() {
  await reset;
  if (closed) throw new Error('Nettleseren er lukket.');
  if (guest) return guest;
  guestCreation ??= createGuest().finally(() => { guestCreation = undefined; });
  return guestCreation;
}

async function disposeGuest() {
  if (guestCreation) await guestCreation.catch(() => {});
  const current = guest;
  guest = undefined;
  if (!current) return;
  if (!closed) window.contentView.removeChildView(current.view);
  current.browser.dispose();
  current.proxy.close();
  await current.browserSession.closeAllConnections();
  await current.browserSession.clearStorageData();
}

function setViewport(value) {
  if (value !== null && (!value || !['x', 'y', 'width', 'height'].every((key) => Number.isFinite(value[key])))) {
    throw new Error('Ugyldig nettleserplassering.');
  }
  viewport = value;
  layoutGuest();
}

const emptyState = { type: 'state', url: '', title: '', isLoading: false, canGoBack: false, canGoForward: false };

function queueReset(clearViewport = false) {
  if (clearViewport) {
    viewport = undefined;
    layoutGuest();
  }
  guest?.view.setVisible(false);
  reset = reset.catch(() => {}).then(disposeGuest).finally(() => publish(emptyState));
  return reset;
}

function register(method, handler) {
  ipcMain.handle(`${channel}${method}`, (event, ...args) => {
    if (closed || !window || event.sender !== window.webContents ||
        event.senderFrame !== window.webContents.mainFrame ||
        new URL(event.senderFrame.url).origin !== application.origin) {
      throw new Error('Denne siden har ikke tilgang til Verevons nettleser.');
    }
    return handler(...args);
  });
}

app.whenReady().then(async () => {
  window = new BrowserWindow({
    width: 1360,
    height: 920,
    minWidth: 740,
    minHeight: 500,
    autoHideMenuBar: true,
    webPreferences: {
      ...webPreferences,
      preload: fileURLToPath(new URL('./preload.cjs', import.meta.url)),
    },
  });
  window.on('resize', layoutGuest);
  window.webContents.on('zoom-changed', layoutGuest);
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-frame-navigate', (event) => {
    if (event.isMainFrame && new URL(event.url).origin !== application.origin) event.preventDefault();
  });
  window.webContents.on('will-redirect', (event) => {
    if (event.isMainFrame && new URL(event.url).origin !== application.origin) event.preventDefault();
  });
  window.webContents.on('did-start-navigation', (event) => {
    if (event.isMainFrame && !event.isSameDocument) void queueReset(true).catch(() => {});
  });
  window.webContents.on('render-process-gone', () => { void queueReset(true).catch(() => {}); });

  register('getState', async () => {
    await reset;
    return guest?.browser.getState() ?? emptyState;
  });
  register('setViewport', setViewport);
  register('navigate', async (url) => (await getGuest()).browser.navigate(url));
  register('command', async (command) => (await getGuest()).browser.command(command));
  register('startRun', async (options) => (await getGuest()).browser.startRun(options));
  register('execute', async (runId, action) => (await getGuest()).browser.execute(runId, action));
  register('endRun', async (runId) => {
    await reset;
    await guest?.browser.endRun(runId);
  });
  register('reset', () => queueReset());

  window.on('close', () => {
    closed = true;
    void disposeGuest().catch(() => {});
  });
  await window.loadURL(application.href);
}).catch((error) => {
  console.error(error.message);
  app.exit(1);
});

app.on('window-all-closed', () => app.quit());
