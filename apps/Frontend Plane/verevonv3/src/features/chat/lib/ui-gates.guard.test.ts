// Proves the UI copy and raw-colour gate fires on the chat and agents surfaces,
// and stays quiet where it must.
//
// Runs both real configs against the same fixtures: the product config (the
// one `pnpm lint` uses) and the standalone gate the spa-ui-gates job runs
// (eslint.ui-gates.config.js). lintText does not apply the baseline in
// eslint-suppressions.json (the CLI reads it, not the ESLint class), so every
// finding a fixture produces is a raw finding.
import { ESLint } from 'eslint'
import { beforeAll, describe, expect, it } from 'vitest'

const RULE = 'no-restricted-syntax'
const SCOPED = 'src/features/chat/components/UiGateFixture.tsx'

let product: ESLint
let gate: ESLint

beforeAll(async () => {
  product = new ESLint()
  gate = new ESLint({ overrideConfigFile: 'eslint.ui-gates.config.js' })
  // Warm both flat configs and the TS parser once (see fabrication-guard.test.ts).
  for (const engine of [product, gate]) {
    await engine.lintText('export const warmup = 1\n', { filePath: 'src/features/chat/__ui_gate_warmup__.tsx' })
  }
}, 180_000)

async function gateMessages(engine: ESLint, code: string, filePath: string): Promise<string[]> {
  const [result] = await engine.lintText(code, { filePath })
  return (result?.messages ?? []).filter((m) => m.ruleId === RULE).map((m) => m.message)
}

const FIRES: Array<[label: string, code: string, filePath: string, message: RegExp]> = [
  ['bare JSX text', 'export const View = () => <button>Lagre</button>\n', SCOPED, /Bare copy/],
  [
    'a naming attribute literal',
    'export const View = () => <button aria-label="Forrige versjon" />\n',
    SCOPED,
    /Accessible name/,
  ],
  [
    'a conditional inside a naming attribute',
    "export const View = (p: { open: boolean }) => <button title={p.open ? 'Lukk' : 'Vis'} />\n",
    SCOPED,
    /Accessible name/,
  ],
  [
    'a conditional in JSX children',
    "export const View = (p: { open: boolean }) => <span>{p.open ? 'Lukk' : 'Vis'}</span>\n",
    SCOPED,
    /conditional or logical/,
  ],
  ['a data label', "export const tabs = [{ label: 'Oversikt' }]\n", SCOPED, /data table/],
  ['Norwegian letters outside i18n.tr', "export const hint = 'Kjøp nå'\n", SCOPED, /outside i18n\.tr/],
  [
    'Norwegian letters in a template',
    'export const hint = (n: number) => `Kjøp ${n}`\n',
    SCOPED,
    /outside i18n\.tr/,
  ],
  [
    'a raw hex colour',
    "export const View = () => <div style={{ color: '#1a2b3c' }} />\n",
    SCOPED,
    /design token/,
  ],
  ['a raw rgb colour in a template', 'export const border = `1px solid rgb(10, 20, 30)`\n', SCOPED, /design token/],
  [
    'bare copy in the agents surface',
    'export const View = () => <h2>Agentens arbeidsflate</h2>\n',
    'src/features/agents/components/UiGateFixture.tsx',
    /Bare copy/,
  ],
]

const QUIET: Array<[label: string, code: string, filePath: string]> = [
  ['an i18n.tr call with Norwegian arguments', "export const label = i18n.tr('Kjøp nå', 'Buy now')\n", SCOPED],
  [
    'an i18n.tr template with Norwegian text',
    'export const label = (n: number) => i18n.tr(`Kjøp ${n}`, `Buy ${n}`)\n',
    SCOPED,
  ],
  [
    'translated naming and copy',
    "export const View = () => <button aria-label={i18n.tr('Forrige versjon', 'Previous version')}>{i18n.tr('Lagre', 'Save')}</button>\n",
    SCOPED,
  ],
  [
    'attributes that carry no copy',
    'export const View = () => <div class="verevon-chat-panel" data-state="open" type="button" role="region" />\n',
    SCOPED,
  ],
  [
    'a locale-neutral product name with its reason',
    'export const Brand = () => (\n  <>\n    {/* eslint-disable-next-line no-restricted-syntax -- locale-neutral: product name */}\n    <span>Verevon</span>\n  </>\n)\n',
    SCOPED,
  ],
  [
    'copy outside the gate scope',
    'export const View = () => <button>Lagre</button>\n',
    'src/features/voice/UiGateFixture.tsx',
  ],
  [
    'copy in a test file',
    'export const View = () => <button>Lagre</button>\n',
    'src/features/chat/components/UiGateFixture.test.tsx',
  ],
]

describe.each([
  ['the product config', () => product],
  ['the standalone gate', () => gate],
])('UI copy and raw-colour gate through %s', (_name, engine) => {
  it.each(FIRES)('fires on %s', async (_label, code, filePath, message) => {
    const messages = await gateMessages(engine(), code, filePath)
    expect(messages.length).toBeGreaterThan(0)
    expect(messages.some((m) => message.test(m))).toBe(true)
  }, 30_000)

  it.each(QUIET)('stays quiet on %s', async (_label, code, filePath) => {
    expect(await gateMessages(engine(), code, filePath)).toEqual([])
  }, 30_000)
})
