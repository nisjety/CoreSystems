import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempDisposable } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import electron from 'electron';

const finn = 'https://www.finn.no/realestate/homes/search.html?location=0.20061&property_type=3';
const walk = process.argv.includes('--walk');
const filter = process.argv.includes('--filter');
const narrow = process.argv.includes('--narrow');
const fixture = String.raw`<!doctype html><title>Verevon Nett test</title><h1>Verevon Nett test</h1><script>
  const browser = window.verevonDesktopBrowser;
  const events = [];
  const report = (result) => fetch('/result', { method: 'POST', body: JSON.stringify(result) });
  const trace = (stage, page) => fetch('/trace', { method: 'POST', body: JSON.stringify({ stage,
    status: page.status, url: page.url, heading: page.heading, targets: page.targets.filter((target) =>
      /eiendom|bolig|leilighet|søk|oslo|område|godta|avvis|cookie/i.test(target.label)).slice(0, 24) }) });
  async function run() {
    browser.subscribe((event) => { if (event.type === 'cursor') events.push(event); });
    await browser.setViewport(${JSON.stringify(narrow)}
      ? { x: 300, y: 230, width: 990, height: 445 }
      : { x: 0, y: 100, width: innerWidth, height: innerHeight - 100 });
    const id = await browser.startRun();
    let result;
    try {
      if (${JSON.stringify(walk)}) {
        const home = await browser.execute(id, { kind: 'navigate', url: 'https://www.finn.no/' });
        await trace('FINN homepage', home);
        const category = home.targets.find((target) => target.role === 'link' && /^Eiendom\b/i.test(target.label));
        if (!category) throw new Error('The FINN homepage did not expose an Eiendom link.');
        const properties = await browser.execute(id, { kind: 'click', target: category.id });
        await trace('Eiendom click', properties);
        if (!/\/realestate\b/.test(properties.url)) throw new Error('The browser did not open Eiendom after clicking.');
        const forSale = properties.targets.find((target) => target.role === 'link' &&
          /^Bolig\b/i.test(target.label) && /\/realestate\/homes\//.test(target.href));
        if (!forSale) throw new Error('The Eiendom page did not expose a Bolig til salgs link.');
        const listings = await browser.execute(id, { kind: 'click', target: forSale.id });
        await trace('Bolig til salgs click', listings);
        if (!/\/realestate\/homes\//.test(listings.url)) throw new Error('The browser did not open Bolig til salgs after clicking.');
      }
      if (${JSON.stringify(filter)}) {
        result = await browser.execute(id, { kind: 'navigate', url: 'https://www.finn.no/realestate/homes/search.html' });
        await trace('Unfiltered results', result);
        for (const label of ['Oslo', 'Leilighet']) {
          for (let attempt = 0; attempt < 3; attempt++) {
            const target = result.targets.find((target) => target.role === 'checkbox' && target.label.startsWith(label));
            if (!target) throw new Error('The results page did not expose the ' + label + ' checkbox.');
            if (target.checked) break;
            try { result = await browser.execute(id, { kind: 'click', target: target.id }); }
            catch (error) { if (attempt === 2) throw error; }
            await new Promise((resolve) => setTimeout(resolve, 500));
            result = await browser.execute(id, { kind: 'snapshot' });
          }
          if (!result.targets.some((target) => target.checked && target.label.startsWith(label))) {
            throw new Error('The actual checkbox did not select ' + label + '.');
          }
          await trace(label + ' click', result);
        }
      } else {
        result = await browser.execute(id, { kind: 'navigate', url: ${JSON.stringify(finn)} });
      }
    } finally {
      await browser.endRun(id);
    }
    const selected = result.targets.filter((target) => target.checked).map((target) => target.label);
    const retained = await browser.getState();
    await browser.navigate('https://www.iana.org/');
    await browser.command('back');
    const deadline = Date.now() + 15000;
    let returned;
    while (Date.now() < deadline) {
      returned = await browser.getState();
      if (returned.url === result.url && !returned.isLoading) break;
      await new Promise((resolve) => setTimeout(resolve, 150));
    }
    await report({ url: result.url, status: result.status, title: result.title, heading: result.heading,
      selected, cursorEvents: events.length, cursorMoved: new Set(events.map(({x,y}) => x + ',' + y)).size > 1,
      retainedUrl: retained.url, returnedUrl: returned.url, canGoForward: returned.canGoForward });
  }
  run().catch((error) => report({ error: error.message }));
</script>`;

let resolveResult;
let rejectResult;
const resultPromise = new Promise((resolve, reject) => {
  resolveResult = resolve;
  rejectResult = reject;
});
const server = createServer(async (request, response) => {
  if (request.method === 'POST' && ['/result', '/trace'].includes(request.url)) {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    response.writeHead(204).end();
    try {
      const payload = JSON.parse(Buffer.concat(chunks).toString());
      if (request.url === '/trace') console.log(JSON.stringify(payload, null, 2));
      else resolveResult(payload);
    } catch (error) {
      rejectResult(error);
    }
  } else {
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(fixture);
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const directory = fileURLToPath(new URL('.', import.meta.url));
const profile = await mkdtempDisposable(join(tmpdir(), 'verevon-desktop-smoke-'));
const child = spawn(electron, ['.'], {
  cwd: directory,
  stdio: 'inherit',
  env: { ...process.env, VEREVON_APP_URL: `http://127.0.0.1:${server.address().port}`,
    VEREVON_DESKTOP_DATA_PATH: profile.path },
});
const timeout = setTimeout(() => rejectResult(new Error('The Electron browser smoke test timed out.')), 110_000);
child.once('error', rejectResult);
child.once('exit', (code) => rejectResult(new Error(`Electron exited before reporting a result (${code}).`)));

try {
  const result = await resultPromise;
  console.log(JSON.stringify(result, null, 2));
  assert.equal(result.error, undefined);
  assert.equal(result.status, 200);
  assert.match(result.url, /finn\.no\/realestate\/homes\/search\.html/);
  assert.ok(result.selected.some((label) => /\bOslo\b/i.test(label)), 'Oslo must be selected in the page DOM');
  assert.ok(result.selected.some((label) => /\bLeilighet\b/i.test(label)), 'Leilighet must be selected in the page DOM');
  assert.ok(result.cursorEvents >= 2 && result.cursorMoved, 'The agent cursor must move live');
  assert.equal(result.retainedUrl, result.url);
  assert.equal(result.returnedUrl, result.url);
  assert.equal(result.canGoForward, true, 'User navigation must stay in the same tab history');
} finally {
  clearTimeout(timeout);
  server.closeAllConnections();
  server.close();
  if (child.exitCode === null && child.signalCode === null && child.pid) {
    await new Promise((resolve) => {
      child.once('exit', resolve);
      child.kill();
    });
  }
  for (let attempt = 0; attempt < 4; attempt += 1) {
    try { await profile.remove(); break; } catch (error) {
      if (attempt === 3) console.error(`Could not remove temporary browser profile (${error.code}).`);
      else await delay(300);
    }
  }
}
