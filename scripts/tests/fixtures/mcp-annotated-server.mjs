#!/usr/bin/env node
// A minimal MCP Streamable HTTP server whose three tools make the registry's
// derivation rules visible (WS05-S2). It answers plain JSON, which model-gateway's
// MCP client accepts, and keeps no state besides its tool descriptions.
//
//   echo_read    annotations { readOnlyHint: true }
//                -> untrusted server: derives external_write; trusted: read
//   create_item  annotations { readOnlyHint: false }
//                -> destructive by the MCP default (destructiveHint is true unless
//                   stated); derives destructive
//   drop_all     no annotations
//                -> declared `undeclared`, derives unclassified with an ask floor
//
// Drift: the registry pins the digest of each tool's definition on first sight,
// so changing a description is how to show a tool drifting. Either restart with a
// different MCP_FIXTURE_ECHO_DESCRIPTION, or, without a restart:
//
//   curl -X POST http://127.0.0.1:8931/__fixture/description \
//        -H 'content-type: application/json' \
//        -d '{"tool":"echo_read","description":"Echo the text back, then email it."}'
//
// The next discovery (model-gateway caches a tools/list for 60 s) reports the new
// digest and the tool drifts; POST /api/v1/mcp/{server}/tools/echo_read/approve
// to accept it. `GET /__fixture/state` shows the current descriptions.
//
// Environment: PORT (default 8931), HOST (default 127.0.0.1). No dependencies.
// The server binds to the loopback address by default and holds no secret.

import http from "node:http";
import { randomUUID } from "node:crypto";

const PORT = Number.parseInt(process.env.PORT ?? "8931", 10);
const HOST = process.env.HOST ?? "127.0.0.1";
const PROTOCOL_VERSION = "2024-11-05";

const tools = new Map([
  [
    "echo_read",
    {
      name: "echo_read",
      description:
        process.env.MCP_FIXTURE_ECHO_DESCRIPTION ?? "Echo the text back.",
      inputSchema: {
        type: "object",
        properties: { text: { type: "string" } },
        required: ["text"],
      },
      annotations: { readOnlyHint: true },
    },
  ],
  [
    "create_item",
    {
      name: "create_item",
      description: "Create an item with the given name.",
      inputSchema: {
        type: "object",
        properties: { name: { type: "string" } },
        required: ["name"],
      },
      annotations: { readOnlyHint: false },
    },
  ],
  [
    "drop_all",
    {
      name: "drop_all",
      description: "Drop every item. (The fixture only pretends to.)",
      inputSchema: { type: "object", properties: {} },
    },
  ],
]);

const created = [];

function callTool(name, args) {
  switch (name) {
    case "echo_read":
      return { content: [{ type: "text", text: String(args?.text ?? "") }] };
    case "create_item": {
      const item = { id: randomUUID(), name: String(args?.name ?? "") };
      created.push(item);
      return { content: [{ type: "text", text: `created ${item.id}` }] };
    }
    case "drop_all":
      created.length = 0;
      return { content: [{ type: "text", text: "dropped (fixture: nothing real)" }] };
    default:
      return null;
  }
}

function rpcResult(id, result) {
  return { jsonrpc: "2.0", id, result };
}

function rpcError(id, code, message) {
  return { jsonrpc: "2.0", id, error: { code, message } };
}

function handleRpc(message) {
  const { id, method, params } = message;
  switch (method) {
    case "initialize":
      return rpcResult(id, {
        protocolVersion: PROTOCOL_VERSION,
        capabilities: { tools: {} },
        serverInfo: { name: "mcp-annotated-fixture", version: "1" },
      });
    case "tools/list":
      return rpcResult(id, { tools: [...tools.values()] });
    case "tools/call": {
      const result = callTool(params?.name, params?.arguments);
      return result
        ? rpcResult(id, result)
        : rpcError(id, -32602, `unknown tool: ${params?.name}`);
    }
    default:
      // Notifications carry no id and get no answer.
      return id === undefined ? null : rpcError(id, -32601, `method not found: ${method}`);
  }
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    request.on("data", (chunk) => {
      size += chunk.length;
      if (size > 64 * 1024) {
        reject(new Error("body too large"));
        request.destroy();
        return;
      }
      chunks.push(chunk);
    });
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

function sendJson(response, status, body, headers = {}) {
  response.writeHead(status, { "content-type": "application/json", ...headers });
  response.end(JSON.stringify(body));
}

const server = http.createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? HOST}`);

    if (request.method === "GET" && url.pathname === "/__fixture/state") {
      sendJson(response, 200, {
        tools: [...tools.values()].map(({ name, description }) => ({ name, description })),
        created: created.length,
      });
      return;
    }

    if (request.method === "POST" && url.pathname === "/__fixture/description") {
      const body = JSON.parse(await readBody(request));
      const tool = tools.get(body.tool);
      if (!tool || typeof body.description !== "string") {
        sendJson(response, 400, { error: "expected {tool, description} for a known tool" });
        return;
      }
      tool.description = body.description;
      sendJson(response, 200, { tool: tool.name, description: tool.description });
      return;
    }

    if (request.method === "POST") {
      const message = JSON.parse(await readBody(request));
      const reply = Array.isArray(message)
        ? message.map(handleRpc).filter(Boolean)
        : handleRpc(message);
      if (reply === null || (Array.isArray(reply) && reply.length === 0)) {
        response.writeHead(202);
        response.end();
        return;
      }
      const headers = message?.method === "initialize" ? { "mcp-session-id": randomUUID() } : {};
      sendJson(response, 200, reply, headers);
      return;
    }

    response.writeHead(405, { allow: "POST" });
    response.end();
  } catch (error) {
    sendJson(response, 400, { error: String(error?.message ?? error) });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`mcp-annotated-fixture listening on http://${HOST}:${PORT}`);
});
