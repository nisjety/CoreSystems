# Browser MCP

Minimal local MCP server that gives Codex a small Playwright-backed browser toolset:

- `open_url`
- `click_element`
- `type_text`
- `read_dom`
- `extract_text`
- `check_page_errors`

## Files

- `scripts/browser-mcp/package.json`
- `scripts/browser-mcp/server.mjs`

## Install

```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/scripts/browser-mcp
npm install
npm run install:browser
```

## Run Standalone

```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/scripts/browser-mcp
npm start
```

This is a stdio MCP server, so it will wait for an MCP client.

## Connect To Codex

Option 1: register it with Codex CLI

```bash
codex mcp add local-browser -- node /Volumes/Lagring/Triodelab/CoreSystem/scripts/browser-mcp/server.mjs
```

Option 2: add it manually to `~/.codex/config.toml`

```toml
[mcp_servers.local-browser]
command = "node"
args = ["/Volumes/Lagring/Triodelab/CoreSystem/scripts/browser-mcp/server.mjs"]
```

Then restart Codex and verify:

```bash
codex mcp list
codex mcp get local-browser
```

## Local Dev Server

Start your app normally:

```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/apps/frontend
npm run dev
```

Then ask Codex to use `http://localhost:3000`.

## Example Prompts

- `Open http://localhost:3000 and tell me what is on the page.`
- `Use the local-browser MCP server to open the app and click the login button.`
- `Open http://localhost:3000, fill the signup form, and submit it.`
- `Open http://localhost:3000 and check for page errors.`
- `Open http://localhost:3000/settings and extract the visible text from the main content area.`

## Notes

- Selectors are CSS selectors.
- Logs must go to stderr, not stdout, because stdout is reserved for MCP JSON-RPC traffic.
- By default the browser runs headless. Set `BROWSER_MCP_HEADLESS=false` to see the browser window.
