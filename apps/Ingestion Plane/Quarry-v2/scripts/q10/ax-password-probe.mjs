// WS12.S1 Q10 recipe (D12-6, OQ-10): what the pinned Chromium exposes for a password field in the
// accessibility tree, and whether DOM.describeNode works without DOM.enable.
//
// NOT RUN yet (build-first rule). Run it once against the pinned Chromium before S1 merges, then
// copy the printed JSON into crates/quarry-browser/tests/fixtures/ax-password-probe.json.
//
// Usage (Node 22+, for the global WebSocket and fetch):
//   chromium --headless=new --remote-debugging-port=9222 --no-sandbox about:blank &
//   node scripts/q10/ax-password-probe.mjs 9222
//
// The dummy values below are not secrets. The typed text is printed on purpose: the point is to
// learn whether the AX value of a password field is plain, bullets or absent.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const port = process.argv[2] ?? '9222';
const base = `http://127.0.0.1:${port}`;
const html = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'secrets-lab.html'), 'utf8');
const EDITABLE = ['textbox', 'searchbox', 'combobox', 'spinbutton'];

const version = await (await fetch(`${base}/json/version`)).json();
const target = await (await fetch(`${base}/json/new?about:blank`, { method: 'PUT' })).json();
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
let nextId = 1;
const pending = new Map();
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (!message.id || !pending.has(message.id)) return;
  const { ok, fail } = pending.get(message.id);
  pending.delete(message.id);
  if (message.error) fail(new Error(JSON.stringify(message.error))); else ok(message.result);
};
const send = (method, params = {}) => new Promise((ok, fail) => {
  const id = nextId++;
  pending.set(id, { ok, fail });
  ws.send(JSON.stringify({ id, method, params }));
});
const evaluate = (expression) => send('Runtime.evaluate', { expression });

await send('Accessibility.enable');
await send('Page.enable');
await send('Page.navigate', { url: `data:text/html;charset=utf-8,${encodeURIComponent(html)}` });
await new Promise((ok) => setTimeout(ok, 800));
await evaluate("document.getElementById('pw').value = 'dummy-prefill'; document.getElementById('toggle-pw').value = 'dummy-toggle';");

const dump = async () => {
  const { nodes } = await send('Accessibility.getFullAXTree', { depth: 8 });
  return nodes
    .filter((node) => EDITABLE.includes(node.role?.value))
    .map((node) => ({
      backend: node.backendDOMNodeId ?? null,
      name: node.name?.value ?? null,
      value: node.value?.value ?? null,
      properties: (node.properties ?? []).map((property) => property.name),
    }));
};
const shape = (value) => {
  if (value === null || value === undefined || value === '') return 'absent';
  return /^[•*]+$/u.test(value) ? 'bullets' : 'plain';
};

const prefilled = await dump();
await evaluate("document.getElementById('pw2').focus()");
await send('Input.insertText', { text: 's3cret-typed' });
const typed = await dump();
await evaluate("document.getElementById('toggle').click()");
const shown = await dump();

let describe;
const newPassword = typed.find((node) => node.name === 'New password');
try {
  const reply = await send('DOM.describeNode', { backendNodeId: newPassword.backend });
  describe = { ok: true, nodeName: reply.node.nodeName };
} catch (error) {
  describe = { ok: false, error: String(error.message).slice(0, 200) };
}

const passwordNode = prefilled.find((node) => node.name === 'Password');
const toggleNode = shown.find((node) => node.name === 'Show password target');
console.log(JSON.stringify({
  chromium_version: version.Browser,
  password_ax_value: shape(passwordNode?.value),
  toggle_ax_value: shape(toggleNode?.value),
  describe_node_without_dom_enable: describe,
}, null, 2));

await fetch(`${base}/json/close/${target.id}`);
ws.close();
