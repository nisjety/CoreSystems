# Model Plane bridges

Reference implementations for the optional external bridges the
`model-gateway` proxies to. Both expose a tiny HTTP contract the
gateway already speaks (see `proto/model_plane/v1/gateway.proto` for
the RPC shapes). Without either bridge configured, the corresponding
gateway RPCs return `Unimplemented` cleanly.

## mcp-bridge (Node)

Hosts external [MCP](https://modelcontextprotocol.io) servers behind a
single HTTP endpoint. The gateway calls `POST /tools/call?server_id=…`
with `{name, arguments}`; the bridge dispatches to the configured
upstream over stdio or HTTP and returns the result verbatim.

```
docker build -t mp-mcp-bridge ./mcp-bridge
docker run --rm -p 9201:9201 \
    -v $(pwd)/mcp-bridge/bridge.example.json:/app/bridge.json \
    mp-mcp-bridge
```

Then register each upstream MCP server against the gateway via
`ModelGateway.RegisterMcpServer` with:

| Field      | Example                                                  |
|------------|----------------------------------------------------------|
| `name`     | `fs`                                                     |
| `url`      | `http://mcp-bridge:9201?server_id=fs`                    |
| `transport`| `http` (the bridge translates to stdio internally)       |
| `enabled`  | `true`                                                   |
| `tool_allowlist` | optional list of allowed tool-name prefixes       |

## lsp-bridge (Python)

Spawns language servers (TypeScript, Python, Go, Rust) on demand and
exposes a JSON wrapper around the LSP JSON-RPC protocol. Speaks the
gateway's `LspQuery` shape: `POST /lsp/query` with `{operation,
file_path, line, column}`.

```
docker build -t mp-lsp-bridge ./lsp-bridge
docker run --rm -p 9202:9202 \
    -v /path/to/workspace:/workspace -e LSP_WORKSPACE_ROOT=/workspace \
    mp-lsp-bridge
```

Then set the gateway's `LSP_BRIDGE_URL=http://lsp-bridge:9202`. The
gateway will start returning real LSP responses on `LspQuery`.

## Why bridges live here, not inside the gateway

- LSP servers are workspace-rooted, stateful, and language-specific —
  much easier to manage outside the Rust hot path.
- MCP servers are typically third-party stdio processes — the bridge
  isolates their lifecycle from the gateway.
- Each bridge is < 300 LOC and trivially replaceable. Operators with a
  richer MCP host (e.g. Claude Desktop, FastMCP) can point the gateway
  straight at that instead.
