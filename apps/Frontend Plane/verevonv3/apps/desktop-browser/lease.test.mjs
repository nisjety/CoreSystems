import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { test } from 'node:test';
import { createDesktopBrowser } from '../../../../Ingestion Plane/Quarry-v2/adapters/desktop-browser/index.mjs';

class Contents extends EventEmitter {
  url = '';
  session = { webRequest: { onBeforeRequest() {} } };
  navigationHistory = { canGoBack: () => false, canGoForward: () => false };
  debugger = {
    isAttached: () => true,
    sendCommand: async (method) => ({
      'Page.getFrameTree': { frameTree: { frame: { id: 'frame' } } },
      'Page.createIsolatedWorld': { executionContextId: 1 },
      'Runtime.evaluate': { result: { value: { heading: null, text: '', targets: [] } } },
    })[method],
  };
  getURL() { return this.url; }
  getTitle() { return 'Example'; }
  isDestroyed() { return false; }
  isLoadingMainFrame() { return false; }
  setWindowOpenHandler() {}
  setWebRTCIPHandlingPolicy() {}
  stop() {}
  loadURL(url) {
    return new Promise((resolve) => {
      this.finish = () => { this.url = url; resolve(); };
    });
  }
}

test('a real mouse click releases the agent lease while pointer movement leaves it active', async () => {
  const contents = new Contents();
  const events = [];
  const browser = createDesktopBrowser(contents, (event) => events.push(event));
  const runId = browser.startRun();
  contents.emit('before-mouse-event', {}, { type: 'mouseMove', x: 10, y: 20 });
  await browser.execute(runId, { kind: 'snapshot' });
  contents.emit('before-mouse-event', {}, { type: 'mouseDown', x: 10, y: 20 });
  assert.deepEqual(events, [{ type: 'handoff' }]);
  assert.throws(() => browser.execute(runId, { kind: 'snapshot' }), /avsluttet/);
  await browser.endRun(runId);
  assert.notEqual(browser.startRun(), runId);
});

test('an address-bar navigation cannot race an agent lease', async () => {
  const contents = new Contents();
  const browser = createDesktopBrowser(contents, () => {});
  const pending = browser.navigate('https://example.com/');
  assert.throws(() => browser.startRun(), /klar/);
  await assert.rejects(browser.navigate('https://www.iana.org/'), /klar/);
  contents.finish();
  assert.equal((await pending).url, 'https://example.com/');
  const runId = browser.startRun();
  await assert.rejects(browser.navigate('https://www.iana.org/'), /Stopp agenten/);
  await browser.endRun(runId);
});
