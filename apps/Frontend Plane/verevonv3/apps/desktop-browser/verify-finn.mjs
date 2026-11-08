import assert from 'node:assert/strict';
import { mkdtemp, readFile, realpath, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { _electron, expect } from '@playwright/test';

const authPath = process.env.VEREVON_DESKTOP_TEST_AUTH;
if (!authPath) throw new Error('Set VEREVON_DESKTOP_TEST_AUTH to a valid local Playwright storage state.');
const auth = JSON.parse(await readFile(authPath, 'utf8'));
const origin = process.env.VEREVON_APP_URL || 'http://localhost:5173/dashboard';
const login = new URL('/login', origin).href;
const require = createRequire(import.meta.url);
const temporary = await realpath(tmpdir());
const profile = await mkdtemp(join(temporary, 'verevon-desktop-test-'));
let desktop;

try {
  desktop = await _electron.launch({
    executablePath: require('electron'),
    args: [`--user-data-dir=${profile}`, dirname(fileURLToPath(import.meta.url))],
    env: { ...process.env, VEREVON_APP_URL: login },
    timeout: 45_000,
  });
  assert.equal(await desktop.evaluate(({ app }) => app.getPath('userData')), profile);
  const app = await desktop.firstWindow();
  await app.waitForURL(login, { waitUntil: 'domcontentloaded' });
  await expect(app.getByRole('button', { name: /^(?:Logg inn|Sign in)$/i }).and(app.locator('button[type="submit"]'))).toBeVisible({ timeout: 25_000 });
  await app.context().addCookies(auth.cookies);
  await app.goto(origin, { waitUntil: 'domcontentloaded' }).catch(error => {
    if (!String(error).includes('net::ERR_ABORTED')) throw error;
  });
  const assistant = app.getByRole('region', { name: /Spør Verevon|Ask Verevon/ });
  await expect(assistant).toBeVisible({ timeout: 25_000 });
  const field = assistant.getByRole('textbox', { name: /Oppgave til Verevon|Task for Verevon/ });
  if (!(await field.isVisible())) {
    const expand = assistant.getByRole('button', { name: /Åpne skrivefelt|Expand chat bar/ });
    if (await expand.isVisible()) await expand.click();
  }
  await expect(field).toBeVisible({ timeout: 25_000 });
  await app.evaluate(() => {
    window.desktopTestMoves = [];
    window.desktopTestApproval = false;
    window.verevonDesktopBrowser.subscribe(event => {
      if (event.type === 'cursor') window.desktopTestMoves.push(event);
    });
    new MutationObserver(() => {
      if (document.querySelector('.global-assistant__approval')) window.desktopTestApproval = true;
    }).observe(document.body, { childList: true, subtree: true });
  });
  app.on('response', async response => {
    if (!response.url().endsWith('/api/v1/browser/assistant/desktop/suggestions')) return;
    const body = await response.json().catch(() => null);
    const suggestion = body?.data?.suggestion;
    const input = response.request().postDataJSON();
    const label = input?.observation?.targets?.find(target => target.id === suggestion?.action?.target)?.label;
    const filters = input?.observation?.targets?.filter(target => target.checked && /^(Oslo|Leilighet)\b/.test(target.label)).map(target => target.label);
    console.log('Planner step', response.status(), suggestion?.action?.kind || suggestion?.outcome || body?.error?.code, label || '', filters || [], input?.history?.at(-1) || '');
  });
  const prompt = process.env.VEREVON_DESKTOP_FINN_PROMPT || 'Vis meg leiligheter til salgs i Oslo på FINN. Åpne resultatlisten og bekreft at Oslo og Leilighet er valgt.';
  const send = assistant.getByRole('button', { name: /Start oppgave|Start task/ });
  await field.fill(prompt);
  if (!(await send.isEnabled())) {
    await field.fill('');
    await field.pressSequentially(prompt, { delay: 8 });
  }
  await expect(field).toHaveValue(prompt);
  await expect(send).toBeEnabled();
  await send.click();
  const nett = app.getByRole('region', { name: /Lokal nettleser|Local browser/ });
  await expect(nett).toBeVisible({ timeout: 25_000 });

  const guestId = async () => desktop.evaluate(({ webContents }) => webContents.getAllWebContents()
    .find(contents => /https:\/\/(?:www\.)?finn\.no\//.test(contents.getURL()))?.id ?? null);
  await expect.poll(guestId, { timeout: 35_000 }).not.toBeNull();
  const originalGuest = await guestId();
  await expect.poll(() => app.evaluate(() => new Set(window.desktopTestMoves.map(({ x, y }) => `${x},${y}`)).size), { timeout: 30_000 }).toBeGreaterThan(1);
  const task = assistant.locator('.global-assistant__agent');
  await expect(task).toContainText(/Kjøringen er avsluttet|The run has ended|Agentoppgaven kunne ikke fullføres|The agent task could not be completed/, { timeout: 195_000 });
  console.log('Assistant', (await task.innerText()).slice(0, 700));
  if (!(await task.locator('.global-assistant__result').count())) {
    console.log('FINN viewport', await desktop.evaluate(async ({ webContents }, id) => {
      return webContents.fromId(id).executeJavaScript(`(() => ({
        width: innerWidth, height: innerHeight, scroll: scrollY,
        dialogs: [...document.querySelectorAll('[aria-modal="true"] iframe[title]')].map(frame => frame.title),
        filters: [...document.querySelectorAll('input[type="checkbox"], [role="checkbox"]')].flatMap(input => {
          const label = input.labels?.[0] || input.parentElement;
          if (!/^(Oslo|Leilighet)\\b/.test(label?.innerText?.trim())) return [];
          const rect = label.getBoundingClientRect();
          const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
          return [{ label: label.innerText.trim(), y: rect.top, hit: hit?.tagName, covered: Boolean(hit && !label.contains(hit)) }];
        }).slice(0, 6)
      }))()`);
    }, originalGuest));
  }
  await expect(task).toContainText(/Kjøringen er avsluttet|The run has ended/);
  await expect(task.locator('.global-assistant__result')).not.toBeEmpty();
  assert.equal(await app.evaluate(() => window.desktopTestApproval), false, 'no Verevon approval must appear during the task');
  assert.equal(await guestId(), originalGuest, 'the finished result must be in the same native guest');
  await expect(nett.getByRole('status')).toHaveText(/Du styrer nettleseren|You control the browser/);
  const filters = await desktop.evaluate(async ({ webContents }, id) => {
    const guest = webContents.fromId(id);
    return guest.executeJavaScript(`(() => {
      const checked = [...document.querySelectorAll('input[type="checkbox"]:checked, [role="checkbox"][aria-checked="true"]')].map(input =>
        [input.getAttribute('aria-label'), ...(input.labels ? [...input.labels].map(label => label.innerText) : []), input.parentElement?.innerText].filter(Boolean).join(' '));
      const selected = [...document.querySelectorAll('button[aria-label], [aria-pressed="true"]')].map(button => button.getAttribute('aria-label') || button.innerText);
      return { href: location.href, labels: [...checked, ...selected.filter(label => /fjern|remove/i.test(label))] };
    })()`);
  }, originalGuest);
  console.log('FINN', filters.href, filters.labels);
  assert.match(new URL(filters.href).pathname, /\/search(?:\.html)?$/);
  assert.match(filters.labels.join('\n'), /\bOslo\b/i);
  assert.match(filters.labels.join('\n'), /\bLeilighet\b/i);

  const reload = nett.getByRole('button', { name: /Last på nytt|Reload/ });
  await expect(reload).toBeEnabled();
  const loaded = desktop.evaluate(({ webContents }, id) => new Promise(resolve => {
    const timeout = setTimeout(() => resolve(false), 25_000);
    webContents.fromId(id).once('did-finish-load', () => { clearTimeout(timeout); resolve(true); });
  }), originalGuest);
  await reload.click();
  assert.equal(await loaded, true);
  assert.equal(await guestId(), originalGuest, 'manual input must operate on the retained guest');
  console.log('Verified selected Oslo and Leilighet, live cursor, automatic handoff, and same-tab manual reload.');
} finally {
  await desktop?.close();
  const resolvedProfile = await realpath(profile);
  if (dirname(resolvedProfile) === temporary && basename(resolvedProfile).startsWith('verevon-desktop-test-')) {
    await rm(resolvedProfile, { recursive: true, force: true, maxRetries: 4 });
  }
}
