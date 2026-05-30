// Reference MCP bridge — exposes registered MCP servers behind a
// single HTTP endpoint the gateway can proxy to.
//
// Why a bridge: the model-gateway's `ProxyMcpTool` RPC sends
// `{name, arguments}` over HTTP `POST /tools/call` to the registered
// server's URL. Real MCP servers speak the MCP JSON-RPC framing over
// stdio or SSE; this bridge translates from the gateway's flat HTTP
// shape into whichever transport the underlying server expects.
//
// Two modes:
//   1. STDIO  — bridge spawns the configured executable, talks MCP
//               JSON-RPC over stdio.
//   2. HTTP   — bridge forwards verbatim (the underlying server
//               already speaks our flat HTTP shape).
//
// Config file (JSON):
//   [
//     {
//       "server_id": "fs",
//       "transport": "stdio",
//       "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/data"],
//       "tool_allowlist": ["read_file", "write_file", "list_dir"]
//     },
//     {
//       "server_id": "ddg",
//       "transport": "http",
//       "url": "https://duckduckgo.example/mcp",
//       "token": "Bearer ..."
//     }
//   ]
//
// Set MCP_BRIDGE_CONFIG to the config file path. Default: ./bridge.json.
// Listens on PORT (default 9201).

const fs = require("fs");
const http = require("http");
const { spawn } = require("child_process");
const { URL } = require("url");

const CONFIG_PATH = process.env.MCP_BRIDGE_CONFIG || "./bridge.json";
const PORT = parseInt(process.env.PORT || "9201", 10);

function log(level, msg, fields = {}) {
  console.log(JSON.stringify({ ts: new Date().toISOString(), level, msg, ...fields }));
}

function loadConfig() {
  if (!fs.existsSync(CONFIG_PATH)) {
    log("warn", "bridge config missing; bridge will reject every call", { path: CONFIG_PATH });
    return [];
  }
  try {
    return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
  } catch (err) {
    log("error", "bridge config parse failed", { error: err.message });
    return [];
  }
}

const SERVERS = loadConfig().reduce((acc, s) => {
  if (s && s.server_id) acc[s.server_id] = s;
  return acc;
}, {});

// stdio MCP servers are long-lived processes. Lazily start one per
// server_id and reuse across calls.
const STDIO_PROCS = {};

function ensureStdioProc(server) {
  if (STDIO_PROCS[server.server_id]) return STDIO_PROCS[server.server_id];
  const [cmd, ...args] = server.command || [];
  if (!cmd) throw new Error("stdio server missing command");
  const child = spawn(cmd, args, { stdio: ["pipe", "pipe", "inherit"] });
  child.on("exit", () => {
    log("warn", "stdio mcp server exited", { server_id: server.server_id });
    delete STDIO_PROCS[server.server_id];
  });
  const state = { child, pending: new Map(), nextId: 1, buf: "" };
  child.stdout.on("data", (chunk) => {
    state.buf += chunk.toString("utf-8");
    while (true) {
      const nl = state.buf.indexOf("\n");
      if (nl < 0) break;
      const line = state.buf.slice(0, nl).trim();
      state.buf = state.buf.slice(nl + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line);
        if (msg.id && state.pending.has(msg.id)) {
          const resolve = state.pending.get(msg.id);
          state.pending.delete(msg.id);
          resolve(msg);
        }
      } catch (_) {
        // Ignore malformed lines — MCP servers may also log to stdout.
      }
    }
  });
  STDIO_PROCS[server.server_id] = state;
  return state;
}

function callStdio(server, name, args) {
  return new Promise((resolve, reject) => {
    let state;
    try { state = ensureStdioProc(server); }
    catch (e) { return reject(e); }
    const id = String(state.nextId++);
    const msg = {
      jsonrpc: "2.0",
      id,
      method: "tools/call",
      params: { name, arguments: args || {} },
    };
    state.pending.set(id, resolve);
    state.child.stdin.write(JSON.stringify(msg) + "\n");
    setTimeout(() => {
      if (state.pending.has(id)) {
        state.pending.delete(id);
        reject(new Error("stdio mcp call timed out"));
      }
    }, 30000);
  });
}

async function callHttp(server, name, args) {
  const r = await fetch(`${server.url}/tools/call`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(server.token ? { Authorization: server.token } : {}),
    },
    body: JSON.stringify({ name, arguments: args || {} }),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`http ${r.status}: ${text.slice(0, 200)}`);
  try { return JSON.parse(text); }
  catch (_) { return { raw: text }; }
}

function isAllowed(server, name) {
  if (!server.tool_allowlist || server.tool_allowlist.length === 0) return true;
  return server.tool_allowlist.some((p) => name.startsWith(p));
}

// Single endpoint: POST /tools/call?server_id=ID with {name, arguments}.
// The model-gateway constructs this URL from the McpServer.url field by
// appending /tools/call (see runtime_registries.rs::handle_proxy_mcp_tool).
// For multi-server bridges, operators register one McpServer per server_id
// with a URL like `http://mcp-bridge:9201?server_id=fs`.
const server = http.createServer(async (req, res) => {
  const u = new URL(req.url, `http://localhost:${PORT}`);
  if (req.method === "GET" && (u.pathname === "/health" || u.pathname === "/")) {
    res.writeHead(200, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ ok: true, servers: Object.keys(SERVERS) }));
  }
  if (!(req.method === "POST" && u.pathname === "/tools/call")) {
    res.writeHead(404).end();
    return;
  }
  const server_id = u.searchParams.get("server_id");
  const cfg = SERVERS[server_id];
  if (!cfg) {
    res.writeHead(404, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ error: `unknown server_id: ${server_id}` }));
  }

  let body = "";
  for await (const chunk of req) body += chunk;
  let payload;
  try { payload = JSON.parse(body || "{}"); }
  catch (e) {
    res.writeHead(400, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ error: `bad json: ${e.message}` }));
  }
  const { name, arguments: args } = payload;
  if (!name) {
    res.writeHead(400, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ error: "name is required" }));
  }
  if (!isAllowed(cfg, name)) {
    res.writeHead(403, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ error: `tool not in allowlist: ${name}` }));
  }

  try {
    const result = cfg.transport === "stdio"
      ? await callStdio(cfg, name, args)
      : await callHttp(cfg, name, args);
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(result));
  } catch (e) {
    log("error", "mcp call failed", { server_id, name, error: e.message });
    res.writeHead(502, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: e.message }));
  }
});

server.listen(PORT, () => {
  log("info", "mcp bridge listening", { port: PORT, servers: Object.keys(SERVERS) });
});

process.on("SIGTERM", () => {
  log("info", "SIGTERM — shutting down");
  for (const id of Object.keys(STDIO_PROCS)) STDIO_PROCS[id].child.kill();
  server.close(() => process.exit(0));
});
