import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { publicWebUrl, startPublicWebProxy } from './egress.mjs';
import { evaluatePage } from './page.mjs';

const targetId = /^t[a-f0-9]{16}$/;
const safeMethods = new Set(['GET', 'HEAD', 'OPTIONS']);

export async function prepareDesktopBrowserSession(session) {
  const proxy = await startPublicWebProxy();
  try {
    await session.setProxy({
      mode: 'fixed_servers',
      proxyRules: `http://127.0.0.1:${proxy.port}`,
      proxyBypassRules: '<-loopback>',
    });
  } catch (error) {
    proxy.close();
    throw error;
  }
  session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  session.setPermissionCheckHandler(() => false);
  session.on('will-download', (event) => event.preventDefault());
  return proxy;
}

export function createDesktopBrowser(contents, publish) {
  let run;
  let inFlight;
  let manualNavigation;
  let releasing;
  let dispatchedInput;
  let agentFocused = false;
  let loadError;
  let status;
  let navigationRevision = 0;
  let isolatedWorld;
  let cursor;
  let expectedPopup;
  let disposed = false;

  function getState() {
    return {
      type: 'state',
      url: contents.getURL(),
      title: contents.getTitle(),
      isLoading: contents.isLoadingMainFrame(),
      canGoBack: contents.navigationHistory.canGoBack(),
      canGoForward: contents.navigationHistory.canGoForward(),
      loadError,
      status,
    };
  }

  function emitState() {
    if (!disposed && !contents.isDestroyed()) publish(getState());
  }

  contents.session.webRequest.onBeforeRequest((details, callback) => {
    try {
      const url = new URL(details.url);
      if (url.protocol === 'ws:' || url.protocol === 'wss:') {
        if (run || inFlight) throw new Error('Agenten kan bare lese nettsider.');
        publicWebUrl(url.href.replace(/^ws/, 'http'));
      } else if (['http:', 'https:'].includes(url.protocol)) {
        publicWebUrl(url.href, Boolean(run || inFlight));
        if ((run || inFlight) && !safeMethods.has(details.method)) {
          throw new Error('Agenten kan bare lese nettsider.');
        }
      } else if (details.resourceType === 'mainFrame' && url.href !== 'about:blank') {
        throw new Error('Nettleseren kan bare åpne offentlige nettadresser.');
      }
      callback({ cancel: false });
    } catch {
      callback({ cancel: true });
    }
  });

  const allowNavigation = (event) => {
    if (!event.isMainFrame) return;
    try {
      publicWebUrl(event.url, Boolean(run || inFlight));
    } catch {
      event.preventDefault();
      loadError = 'Nettleseren kan bare åpne offentlige nettadresser.';
      emitState();
    }
  };
  contents.on('will-frame-navigate', allowNavigation);
  contents.on('will-redirect', allowNavigation);
  contents.setWindowOpenHandler(({ url, disposition, postBody }) => {
    // User-opened links still navigate the retained tab. Suppress script popups
    // and let an agent open only the exact link it just targeted.
    if (!postBody && (run ? url === expectedPopup :
      !inFlight && ['foreground-tab', 'background-tab'].includes(disposition))) {
      try {
        const destination = publicWebUrl(url, Boolean(run || inFlight));
        void contents.loadURL(destination.href).catch(() => {});
      } catch {
        // Window creation and unsafe destinations both remain blocked.
      }
    }
    return { action: 'deny' };
  });
  contents.setWebRTCIPHandlingPolicy('disable_non_proxied_udp');
  contents.on('did-start-navigation', (event) => {
    if (!event.isMainFrame || event.isSameDocument) return;
    loadError = undefined;
    status = undefined;
  });
  contents.on('did-navigate', (_event, _url, code) => {
    navigationRevision += 1;
    status = code > 0 ? code : undefined;
    emitState();
  });
  contents.on('did-fail-load', (_event, code, _message, _url, isMainFrame) => {
    if (isMainFrame && code !== -3) {
      loadError = `Nettleseren kunne ikke åpne siden (${code}).`;
      emitState();
    }
  });
  contents.on('render-process-gone', () => {
    if (run) void endRun(run).catch(() => {});
    loadError = 'Nettleseren ble lukket. Åpne siden på nytt.';
    emitState();
  });
  for (const event of ['did-start-loading', 'did-stop-loading', 'page-title-updated', 'did-navigate-in-page']) {
    contents.on(event, emitState);
  }
  contents.on('before-mouse-event', (_event, mouse) => {
    if (mouse.type !== 'mouseDown' && mouse.type !== 'mouseWheel') return;
    if (dispatchedInput?.type === mouse.type && dispatchedInput.x === mouse.x && dispatchedInput.y === mouse.y) return;
    handoff();
  });
  contents.on('before-input-event', (_event, input) => {
    if (input.type === 'keyDown' &&
        (dispatchedInput?.type !== 'keyDown' || dispatchedInput.key !== input.key)) handoff();
  });

  function handoff() {
    if (!run) return;
    void endRun(run).catch(() => {});
    publish({ type: 'handoff' });
  }

  function debug(method, parameters = {}) {
    if (!contents.debugger.isAttached()) contents.debugger.attach('1.3');
    return contents.debugger.sendCommand(method, parameters);
  }

  async function evaluate(operation, value) {
    if (!isolatedWorld || isolatedWorld.revision !== navigationRevision) {
      const revision = navigationRevision;
      const { frameTree } = await debug('Page.getFrameTree');
      const { executionContextId } = await debug('Page.createIsolatedWorld', {
        frameId: frameTree.frame.id,
        worldName: 'quarry-desktop-browser',
      });
      isolatedWorld = { revision, contextId: executionContextId };
    }
    try {
      return await evaluateInWorld(isolatedWorld.contextId, operation, value);
    } catch (error) {
      isolatedWorld = undefined;
      throw error;
    }
  }

  async function evaluateInWorld(contextId, operation, value = null) {
    const { result, exceptionDetails } = await debug('Runtime.evaluate', {
      contextId,
      expression: `(${evaluatePage.toString()})(${JSON.stringify(operation)},${JSON.stringify(value)})`,
      returnByValue: true,
    });
    if (exceptionDetails) throw new Error('Kunne ikke lese nettleserens side.');
    return result.value;
  }

  async function page(operation, value = null) {
    let timeout;
    try {
      return await Promise.race([
        evaluate(operation, value),
        new Promise((_, reject) => {
          timeout = setTimeout(() => reject(new Error('Nettstedet brukte for lang tid på å svare.')), 12_000);
        }),
      ]);
    } finally {
      clearTimeout(timeout);
    }
  }

  async function snapshot() {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const { url, title, canGoBack, canGoForward } = getState();
      if (!url || url === 'about:blank' || !/^https?:/.test(url)) {
        return { url, title, heading: null, text: '', targets: [], canGoBack, canGoForward, status, loadError };
      }
      const revision = navigationRevision;
      try {
        const result = await page('snapshot');
        if (contents.getURL() === url && navigationRevision === revision) {
          return { url, title, ...result, canGoBack, canGoForward, status, loadError };
        }
      } catch (error) {
        if (contents.getURL() === url && navigationRevision === revision) throw error;
      }
      await delay(100);
    }
    throw new Error('Siden har endret seg. Les den på nytt.');
  }

  async function sendInput(method, parameters) {
    if (!agentFocused) {
      await debug('Emulation.setFocusEmulationEnabled', { enabled: true });
      agentFocused = true;
    }
    const types = { mousePressed: 'mouseDown', mouseWheel: 'mouseWheel', keyDown: 'keyDown' };
    dispatchedInput = { ...parameters, type: types[parameters.type] };
    try {
      return await debug(`Input.${method}`, parameters);
    } finally {
      dispatchedInput = undefined;
    }
  }

  function assertRun(id) {
    if (disposed || !run || run !== id) throw new Error('Nettleseroppgaven er avsluttet.');
  }

  async function moveCursor(id, point, action) {
    const origin = cursor ?? { x: 10, y: 12 };
    for (let step = 1; step <= 6; step += 1) {
      assertRun(id);
      cursor = {
        x: Math.round(origin.x + ((point.x - origin.x) * step) / 6),
        y: Math.round(origin.y + ((point.y - origin.y) * step) / 6),
      };
      await page('cursor', cursor);
      assertRun(id);
      publish({ type: 'cursor', ...cursor, action });
      if (step < 6) await delay(23);
    }
  }

  async function tryCookieOptOut(id, waitForFrame, signal) {
    for (let attempt = 0; attempt < 4; attempt += 1) {
      assertRun(id);
      signal.throwIfAborted();
      const frames = await page('consentFrames');
      if (!Array.isArray(frames) || frames.length === 0) {
        if (!waitForFrame || attempt === 3) return;
        await delay(180);
        continue;
      }
      const { frameTree } = await debug('Page.getFrameTree');
      let clicked = false;
      for (const frame of frames) {
        if (frame.width < 20 || frame.height < 20) continue;
        const node = frameTree.childFrames?.find(({ frame: child }) => child.url === frame.url);
        if (!node) continue;
        const { executionContextId } = await debug('Page.createIsolatedWorld', {
          frameId: node.frame.id,
          worldName: 'quarry-desktop-consent',
        });
        const choice = await evaluateInWorld(executionContextId, 'consentChoice');
        if (!choice || choice.x < 0 || choice.y < 0 || choice.x >= frame.width || choice.y >= frame.height) continue;
        signal.throwIfAborted();
        const point = { x: Math.round(frame.x + choice.x), y: Math.round(frame.y + choice.y) };
        await moveCursor(id, point, 'click');
        const current = await evaluateInWorld(executionContextId, 'consentChoice');
        if (!current || current.x !== choice.x || current.y !== choice.y || current.reject !== choice.reject) break;
        const currentFrames = await page('consentFrames');
        if (!currentFrames.some((item) => item.url === frame.url && item.x === frame.x && item.y === frame.y)) break;
        assertRun(id);
        signal.throwIfAborted();
        await sendInput('dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', buttons: 1, clickCount: 1 });
        const completed = run === id && !signal.aborted;
        await sendInput('dispatchMouseEvent', { type: 'mouseReleased', ...(completed ? point : { x: 0, y: 0 }),
          button: 'left', buttons: 0, clickCount: completed ? 1 : 0 });
        assertRun(id);
        clicked = true;
        await delay(320);
        if (choice.reject) return;
        break;
      }
      if (!clicked) await delay(150);
    }
  }

  async function dismissOptionalCookies(id, waitForFrame = false) {
    const controller = new AbortController();
    let timeout;
    try {
      await Promise.race([
        tryCookieOptOut(id, waitForFrame, controller.signal),
        new Promise((resolve) => {
          timeout = setTimeout(() => { controller.abort(); resolve(); }, 5_000);
        }),
      ]);
    } catch {
      assertRun(id);
    } finally {
      clearTimeout(timeout);
    }
  }

  async function navigate(url, id) {
    const destination = publicWebUrl(url, Boolean(id));
    const previousNavigation = navigationRevision;
    let timeout;
    try {
      await Promise.race([
        contents.loadURL(destination.href),
        new Promise((_, reject) => {
          timeout = setTimeout(() => {
            if (navigationRevision === previousNavigation) contents.stop();
            reject(new Error('Nettstedet brukte for lang tid på å svare.'));
          }, 22_000);
        }),
      ]);
    } catch (error) {
      if (id) assertRun(id);
      if (!loadError && !contents.getURL().startsWith('https://')) {
        loadError = 'Nettleseren kunne ikke åpne siden.';
        emitState();
      }
      if (navigationRevision === previousNavigation || !/^https?:/.test(contents.getURL()) || loadError) throw error;
    } finally {
      clearTimeout(timeout);
    }
    if (id) {
      assertRun(id);
      await moveCursor(id, { x: 28, y: 28 }, 'navigate');
      await dismissOptionalCookies(id, true);
    }
  }

  async function perform(id, action) {
    assertRun(id);
    switch (action?.kind) {
      case 'navigate':
        await navigate(action.url, id);
        break;
      case 'snapshot':
        if (/^https?:/.test(contents.getURL())) await dismissOptionalCookies(id);
        break;
      case 'click':
      case 'fill': {
        const { target, text, kind } = action;
        if (typeof target !== 'string' || !targetId.test(target) ||
            (kind === 'fill' && (typeof text !== 'string' || text.length > 500 || /[\r\n\u0000]/.test(text)))) {
          throw new Error('Ugyldig nettleserhandling.');
        }
        await dismissOptionalCookies(id);
        let hit = await page('target', { id: target, kind });
        if (hit?.unavailable === 'covered') {
          await dismissOptionalCookies(id, true);
          hit = await page('target', { id: target, kind });
        }
        if (!hit || hit.unavailable) {
          throw new Error('Denne kontrollen kan ikke brukes. Les siden på nytt.');
        }
        if (hit.href) publicWebUrl(hit.href, true);
        if (hit.covered) {
          assertRun(id);
          await navigate(hit.href, id);
          break;
        }
        await moveCursor(id, hit, kind);
        let current = await page('target', { id: target, kind });
        if (!current || current.unavailable) {
          await delay(160);
          assertRun(id);
          await dismissOptionalCookies(id, true);
          current = await page('target', { id: target, kind });
        }
        if (!current || current.unavailable || current.href !== hit.href) {
          throw new Error('Kontrollen flyttet seg. Les siden på nytt.');
        }
        assertRun(id);
        if (current.covered) {
          await navigate(current.href, id);
          break;
        }
        const point = { x: current.x, y: current.y };
        if (cursor.x !== point.x || cursor.y !== point.y) {
          cursor = point;
          await page('cursor', cursor);
          assertRun(id);
          publish({ type: 'cursor', ...point, action: kind });
        }
        await sendInput('dispatchMouseEvent', { type: 'mouseMoved', ...point });
        assertRun(id);
        expectedPopup = hit.href;
        const beforeClick = contents.getURL();
        await sendInput('dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', buttons: 1, clickCount: 1 });
        const completedClick = run === id;
        await sendInput('dispatchMouseEvent', { type: 'mouseReleased',
          ...(completedClick ? point : { x: 0, y: 0 }), button: 'left', buttons: 0, clickCount: completedClick ? 1 : 0 });
        assertRun(id);
        if (kind === 'fill') {
          if (!(await page('focus'))) throw new Error('Søkefeltet er ikke i fokus.');
          assertRun(id);
          await sendInput('dispatchKeyEvent', { type: 'keyDown', key: 'a', code: 'KeyA', modifiers: process.platform === 'darwin' ? 4 : 2, windowsVirtualKeyCode: 65 });
          await sendInput('dispatchKeyEvent', { type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65 });
          assertRun(id);
          await sendInput('insertText', { text });
        }
        await delay(220);
        assertRun(id);
        // Some anchors open a new browsing context or leave navigation to a
        // delayed page handler. Follow the verified link in the retained tab.
        if (hit.href && hit.href !== beforeClick && contents.getURL() === beforeClick && !contents.isLoadingMainFrame()) {
          await navigate(hit.href, id);
        }
        break;
      }
      case 'press':
        if (action.key !== 'Enter' || !(await page('focus'))) {
          throw new Error('Enter er bare tilgjengelig i søkefelt.');
        }
        assertRun(id);
        await sendInput('dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
        await sendInput('dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
        await delay(220);
        break;
      case 'scroll': {
        if (!Number.isInteger(action.deltaY) || action.deltaY === 0 || Math.abs(action.deltaY) > 1800) {
          throw new Error('Ugyldig rulleavstand.');
        }
        const point = cursor ?? { x: 40, y: 40 };
        await moveCursor(id, point, 'scroll');
        await sendInput('dispatchMouseEvent', { type: 'mouseWheel', ...point, deltaX: 0, deltaY: action.deltaY });
        await delay(160);
        break;
      }
      default:
        throw new Error('Nettleserhandlingen støttes ikke.');
    }
    assertRun(id);
    if (cursor && action.kind !== 'snapshot') await page('cursor', cursor);
    assertRun(id);
    return snapshot();
  }

  function endRun(id) {
    if (releasing?.id === id) return releasing.promise;
    if (run !== id) return Promise.resolve();
    run = undefined;
    expectedPopup = undefined;
    const promise = (async () => {
      if (inFlight) {
        if (!contents.isDestroyed()) contents.stop();
        await inFlight.catch(() => {});
      }
      if (agentFocused && !contents.isDestroyed() && contents.debugger.isAttached()) {
        await contents.debugger.sendCommand('Emulation.setFocusEmulationEnabled', { enabled: false }).catch(() => {});
      }
      agentFocused = false;
      if (cursor && !disposed) await page('cursor', { ...cursor, release: true }).catch(() => {});
    })().finally(() => { if (releasing?.id === id) releasing = undefined; });
    releasing = { id, promise };
    return promise;
  }

  return {
    getState,
    startRun() {
      if (run || inFlight || releasing || manualNavigation || contents.isLoadingMainFrame()) {
        throw new Error('Vent til nettleseren er klar.');
      }
      run = randomUUID();
      return run;
    },
    execute(id, action) {
      assertRun(id);
      if (inFlight) throw new Error('Vent til forrige nettleserhandling er ferdig.');
      inFlight = perform(id, action).finally(() => {
        expectedPopup = undefined;
        inFlight = undefined;
      });
      return inFlight;
    },
    endRun,
    async navigate(url) {
      if (run || inFlight) throw new Error('Stopp agenten før du åpner en ny side.');
      if (manualNavigation) throw new Error('Vent til nettleseren er klar.');
      cursor = undefined;
      manualNavigation = navigate(url).then(snapshot).finally(() => { manualNavigation = undefined; });
      return manualNavigation;
    },
    async command(command) {
      if (command === 'stop') {
        if (run) await endRun(run);
        else contents.stop();
      } else {
        if (run || inFlight) throw new Error('Stopp agenten før du navigerer.');
        if (manualNavigation) throw new Error('Vent til nettleseren er klar.');
        cursor = undefined;
        switch (command) {
          case 'back':
            if (contents.navigationHistory.canGoBack()) contents.navigationHistory.goBack();
            break;
          case 'forward':
            if (contents.navigationHistory.canGoForward()) contents.navigationHistory.goForward();
            break;
          case 'reload':
            contents.reload();
            break;
          default:
            throw new Error('Nettleserhandlingen støttes ikke.');
        }
      }
      return getState();
    },
    dispose() {
      disposed = true;
      run = undefined;
      contents.close({ waitForBeforeUnload: false });
    },
  };
}
