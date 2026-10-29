import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { chromium } from 'playwright'
import * as z from 'zod/v4'

const DEFAULT_TIMEOUT_MS = Number(process.env.BROWSER_MCP_TIMEOUT_MS ?? 15_000)
const DEFAULT_HEADLESS = process.env.BROWSER_MCP_HEADLESS !== 'false'

const server = new McpServer(
  {
    name: 'browser-mcp',
    version: '0.1.0',
  },
  {
    capabilities: {
      logging: {},
    },
  },
)

let browserPromise = null
let contextPromise = null
let pagePromise = null
const pageErrors = []
const consoleErrors = []

async function getBrowser() {
  if (!browserPromise) {
    browserPromise = chromium.launch({
      headless: DEFAULT_HEADLESS,
    })
  }

  return browserPromise
}

async function getContext() {
  if (!contextPromise) {
    const browser = await getBrowser()
    contextPromise = browser.newContext({
      viewport: { width: 1440, height: 900 },
    })
  }

  return contextPromise
}

async function getPage() {
  if (!pagePromise) {
    const context = await getContext()
    const page = await context.newPage()

    page.setDefaultTimeout(DEFAULT_TIMEOUT_MS)

    page.on('pageerror', (error) => {
      pageErrors.push({
        message: error.message,
        timestamp: new Date().toISOString(),
      })
    })

    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push({
          message: msg.text(),
          location: msg.location(),
          timestamp: new Date().toISOString(),
        })
      }
    })

    pagePromise = Promise.resolve(page)
  }

  return pagePromise
}

function trimText(value, maxLength = 5000) {
  if (!value) {
    return ''
  }

  return value.length > maxLength ? `${value.slice(0, maxLength - 3)}...` : value
}

function asTextResult(data) {
  return {
    content: [
      {
        type: 'text',
        text: JSON.stringify(data, null, 2),
      },
    ],
    structuredContent: data,
  }
}

server.registerTool(
  'open_url',
  {
    title: 'Open URL',
    description: 'Open a page in the Playwright browser.',
    inputSchema: z.object({
      url: z.string().url().describe('The URL to open, for example http://localhost:3000'),
      waitUntil: z.enum(['load', 'domcontentloaded', 'networkidle']).optional(),
    }),
  },
  async ({ url, waitUntil = 'domcontentloaded' }) => {
    const page = await getPage()
    await page.goto(url, { waitUntil })

    return asTextResult({
      ok: true,
      url: page.url(),
      title: await page.title(),
    })
  },
)

server.registerTool(
  'click_element',
  {
    title: 'Click Element',
    description: 'Click an element using a CSS selector.',
    inputSchema: z.object({
      selector: z.string().min(1).describe('A CSS selector, for example button[type="submit"]'),
    }),
  },
  async ({ selector }) => {
    const page = await getPage()
    await page.locator(selector).first().click()

    return asTextResult({
      ok: true,
      selector,
      url: page.url(),
    })
  },
)

server.registerTool(
  'type_text',
  {
    title: 'Type Text',
    description: 'Fill a text field using a CSS selector.',
    inputSchema: z.object({
      selector: z.string().min(1).describe('A CSS selector for an input, textarea, or contenteditable element'),
      text: z.string().describe('The text to type'),
      submit: z.boolean().optional(),
    }),
  },
  async ({ selector, text, submit = false }) => {
    const page = await getPage()
    const locator = page.locator(selector).first()

    await locator.fill(text)

    if (submit) {
      await locator.press('Enter')
    }

    return asTextResult({
      ok: true,
      selector,
      textLength: text.length,
      submitted: submit,
    })
  },
)

server.registerTool(
  'read_dom',
  {
    title: 'Read DOM',
    description: 'Read a DOM fragment as HTML.',
    inputSchema: z.object({
      selector: z.string().optional().describe('A CSS selector. Defaults to body.'),
      maxLength: z.number().int().positive().max(20_000).optional(),
    }),
  },
  async ({ selector = 'body', maxLength = 5000 }) => {
    const page = await getPage()
    const html = await page.locator(selector).first().evaluate((element) => element.outerHTML)

    return asTextResult({
      ok: true,
      selector,
      html: trimText(html, maxLength),
    })
  },
)

server.registerTool(
  'extract_text',
  {
    title: 'Extract Text',
    description: 'Extract visible text from an element or the full page.',
    inputSchema: z.object({
      selector: z.string().optional().describe('A CSS selector. Defaults to body.'),
      maxLength: z.number().int().positive().max(20_000).optional(),
    }),
  },
  async ({ selector = 'body', maxLength = 5000 }) => {
    const page = await getPage()
    const text = await page.locator(selector).first().innerText()

    return asTextResult({
      ok: true,
      selector,
      text: trimText(text, maxLength),
    })
  },
)

server.registerTool(
  'check_page_errors',
  {
    title: 'Check Page Errors',
    description: 'Return collected page errors and console.error messages for the current page.',
    inputSchema: z.object({}),
  },
  async () => {
    const page = await getPage()

    return asTextResult({
      ok: true,
      url: page.url(),
      pageErrors,
      consoleErrors,
      hasErrors: pageErrors.length > 0 || consoleErrors.length > 0,
    })
  },
)

async function shutdown() {
  try {
    const context = await contextPromise
    await context?.close()
  } catch {}

  try {
    const browser = await browserPromise
    await browser?.close()
  } catch {}
}

async function main() {
  const transport = new StdioServerTransport()
  await server.connect(transport)
  console.error('browser-mcp running on stdio')
}

process.on('SIGINT', async () => {
  await shutdown()
  process.exit(0)
})

process.on('SIGTERM', async () => {
  await shutdown()
  process.exit(0)
})

main().catch(async (error) => {
  console.error('Fatal error in browser-mcp:', error)
  await shutdown()
  process.exit(1)
})
