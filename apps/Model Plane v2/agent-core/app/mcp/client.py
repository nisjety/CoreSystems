"""MCP client — connect to external MCP servers via stdio or HTTP.

Implements the JSON-RPC 2.0 protocol used by MCP:
- stdio: spawn subprocess, communicate via stdin/stdout
- http: POST to server URL with JSON-RPC body

Each client instance manages a single MCP server connection.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
from typing import Any
from uuid import uuid4

import httpx

# websockets is optional; only required for WS transport
try:
    import websockets.asyncio.client as _ws_client  # type: ignore[import]
    _WEBSOCKETS_AVAILABLE = True
except ImportError:  # pragma: no cover
    _ws_client = None  # type: ignore[assignment]
    _WEBSOCKETS_AVAILABLE = False

from app.mcp.config import (
    McpServerConfig,
    McpServerStatus,
    McpTool,
    McpToolCallResponse,
    McpTransport,
)
from app.mcp.oauth import McpOAuthTokenManager
from app.resilience import CircuitBreaker, retry

logger = logging.getLogger(__name__)

# Timeout for MCP tool calls
MCP_CALL_TIMEOUT = 30.0  # seconds
MCP_INIT_TIMEOUT = 10.0  # seconds

# Per-server circuit breakers (Phase 2 resilience)
_server_breakers: dict[str, CircuitBreaker] = {}


def _get_breaker(server_name: str) -> CircuitBreaker:
    """Get or create a circuit breaker for an MCP server."""
    if server_name not in _server_breakers:
        _server_breakers[server_name] = CircuitBreaker(
            name=f"mcp-{server_name}",
            failure_threshold=3,
            recovery_timeout=60.0,
        )
    return _server_breakers[server_name]


class MCPClient:
    """Client for a single MCP server connection."""

    def __init__(self, config: McpServerConfig) -> None:
        self._config = config
        self._status = McpServerStatus.DISCONNECTED
        self._tools: list[McpTool] = []

        # stdio transport state
        self._process: asyncio.subprocess.Process | None = None
        self._request_id = 0

        # http transport state
        self._http_client: httpx.AsyncClient | None = None

        # sse transport state
        self._sse_client: httpx.AsyncClient | None = None
        self._sse_post_url: str | None = None
        self._sse_pending: dict[int, asyncio.Future] = {}
        self._sse_reader_task: asyncio.Task | None = None

        # ws transport state
        self._ws: object | None = None  # websockets.ClientConnection
        self._ws_pending: dict[int, asyncio.Future] = {}
        self._ws_reader_task: asyncio.Task | None = None

        # oauth manager (created lazily if config.oauth is set)
        self._oauth_manager: McpOAuthTokenManager | None = None

    @property
    def status(self) -> McpServerStatus:
        return self._status

    @property
    def server_name(self) -> str:
        return self._config.name

    @property
    def tools(self) -> list[McpTool]:
        return list(self._tools)

    async def connect(self) -> None:
        """Connect to the MCP server and discover tools."""
        try:
            if self._config.transport == McpTransport.STDIO:
                await self._connect_stdio()
            elif self._config.transport == McpTransport.HTTP:
                await self._connect_http()
            elif self._config.transport == McpTransport.SSE:
                await self._connect_sse()
            elif self._config.transport == McpTransport.WS:
                await self._connect_ws()

            self._status = McpServerStatus.CONNECTED
            logger.info(
                "mcp_connected",
                extra={
                    "server": self._config.name,
                    "transport": self._config.transport.value,
                    "tools_count": len(self._tools),
                },
            )
        except Exception as exc:
            self._status = McpServerStatus.ERROR
            logger.error(
                "mcp_connect_failed",
                extra={"server": self._config.name, "error": str(exc)},
            )
            raise

    async def disconnect(self) -> None:
        """Disconnect from the MCP server."""
        if self._process is not None:
            try:
                self._process.terminate()
                await asyncio.wait_for(self._process.wait(), timeout=5.0)
            except (ProcessLookupError, asyncio.TimeoutError):
                self._process.kill()
            self._process = None

        if self._http_client is not None:
            await self._http_client.aclose()
            self._http_client = None

        if self._sse_reader_task is not None:
            self._sse_reader_task.cancel()
            try:
                await self._sse_reader_task
            except asyncio.CancelledError:
                pass
            self._sse_reader_task = None
        if self._sse_client is not None:
            await self._sse_client.aclose()
            self._sse_client = None
        # Reject any pending SSE waiters
        for fut in self._sse_pending.values():
            if not fut.done():
                fut.set_exception(RuntimeError("SSE connection closed"))
        self._sse_pending.clear()

        if self._ws_reader_task is not None:
            self._ws_reader_task.cancel()
            try:
                await self._ws_reader_task
            except asyncio.CancelledError:
                pass
            self._ws_reader_task = None
        if self._ws is not None:
            try:
                await self._ws.close()  # type: ignore[attr-defined]
            except Exception:
                pass
            self._ws = None
        # Reject any pending WS waiters
        for fut in self._ws_pending.values():
            if not fut.done():
                fut.set_exception(RuntimeError("WebSocket connection closed"))
        self._ws_pending.clear()

        self._status = McpServerStatus.DISCONNECTED
        self._tools = []

    async def list_tools(self) -> list[McpTool]:
        """Return cached tool list (populated on connect)."""
        return list(self._tools)

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> McpToolCallResponse:
        """Invoke a tool on the MCP server (with circuit breaker)."""
        if self._status != McpServerStatus.CONNECTED:
            return McpToolCallResponse(
                is_error=True, error_message="MCP server not connected"
            )

        breaker = _get_breaker(self._config.name)
        if breaker.should_reject():
            return McpToolCallResponse(
                is_error=True,
                error_message=f"MCP server {self._config.name} circuit open (too many failures)",
            )

        try:
            if self._config.transport == McpTransport.STDIO:
                result = await self._call_stdio(name, arguments)
            elif self._config.transport == McpTransport.HTTP:
                result = await self._call_http(name, arguments)
            elif self._config.transport == McpTransport.SSE:
                result = await self._call_sse(name, arguments)
            elif self._config.transport == McpTransport.WS:
                result = await self._call_ws(name, arguments)
            else:
                return McpToolCallResponse(
                    is_error=True,
                    error_message=f"Unsupported transport: {self._config.transport}",
                )

            if result.is_error:
                breaker.record_failure()
            else:
                breaker.record_success()
            return result
        except Exception as exc:
            breaker.record_failure()
            logger.error(
                "mcp_call_failed",
                extra={"server": self._config.name, "tool": name, "error": str(exc)},
            )
            return McpToolCallResponse(is_error=True, error_message=str(exc))

    # ----------------------------------------------------------------
    # stdio transport
    # ----------------------------------------------------------------

    async def _connect_stdio(self) -> None:
        """Spawn the MCP server process and initialize."""
        if not self._config.command:
            raise ValueError("stdio transport requires 'command'")

        env = {**os.environ, **self._config.env}
        cmd = [self._config.command, *self._config.args]

        self._process = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
        )

        # Initialize MCP session
        init_resp = await self._jsonrpc_stdio("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "agent-core", "version": "2.0"},
        })

        # Send initialized notification
        await self._notify_stdio("notifications/initialized", {})

        # Discover tools
        tools_resp = await self._jsonrpc_stdio("tools/list", {})
        self._tools = [
            McpTool(
                name=t["name"],
                description=t.get("description", ""),
                input_schema=t.get("inputSchema", {}),
                server_name=self._config.name,
            )
            for t in tools_resp.get("tools", [])
        ]

    async def _call_stdio(self, name: str, arguments: dict[str, Any]) -> McpToolCallResponse:
        """Call a tool via stdio JSON-RPC."""
        result = await self._jsonrpc_stdio(
            "tools/call",
            {"name": name, "arguments": arguments},
            timeout=MCP_CALL_TIMEOUT,
        )

        if "error" in result:
            return McpToolCallResponse(
                is_error=True,
                error_message=result["error"].get("message", str(result["error"])),
            )

        content = result.get("content", [])
        # Flatten text content into a single string
        text_parts = [
            c.get("text", "") for c in content if c.get("type") == "text"
        ]
        return McpToolCallResponse(
            content="\n".join(text_parts) if text_parts else content,
            is_error=result.get("isError", False),
        )

    async def _jsonrpc_stdio(
        self, method: str, params: dict[str, Any], timeout: float = MCP_INIT_TIMEOUT
    ) -> dict[str, Any]:
        """Send a JSON-RPC request via stdio and return the result."""
        if self._process is None or self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("MCP process not running")

        self._request_id += 1
        request = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": method,
            "params": params,
        }
        line = json.dumps(request) + "\n"
        self._process.stdin.write(line.encode())
        await self._process.stdin.drain()

        # Read response line
        raw = await asyncio.wait_for(
            self._process.stdout.readline(), timeout=timeout
        )
        if not raw:
            raise RuntimeError("MCP process closed stdout")

        response = json.loads(raw.decode())
        if "error" in response:
            return {"error": response["error"]}
        return response.get("result", {})

    async def _notify_stdio(self, method: str, params: dict[str, Any]) -> None:
        """Send a JSON-RPC notification (no response expected)."""
        if self._process is None or self._process.stdin is None:
            raise RuntimeError("MCP process not running")

        notification = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }
        line = json.dumps(notification) + "\n"
        self._process.stdin.write(line.encode())
        await self._process.stdin.drain()

    # ----------------------------------------------------------------
    # HTTP transport
    # ----------------------------------------------------------------

    async def _connect_http(self) -> None:
        """Initialize HTTP connection and discover tools."""
        if not self._config.url:
            raise ValueError("http transport requires 'url'")

        self._http_client = httpx.AsyncClient(
            base_url=self._config.url,
            headers=self._config.headers,
            timeout=MCP_CALL_TIMEOUT,
        )

        # Initialize
        init_resp = await self._jsonrpc_http("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "agent-core", "version": "2.0"},
        })

        # Discover tools
        tools_resp = await self._jsonrpc_http("tools/list", {})
        self._tools = [
            McpTool(
                name=t["name"],
                description=t.get("description", ""),
                input_schema=t.get("inputSchema", {}),
                server_name=self._config.name,
            )
            for t in tools_resp.get("tools", [])
        ]

    async def _call_http(self, name: str, arguments: dict[str, Any]) -> McpToolCallResponse:
        """Call a tool via HTTP JSON-RPC."""
        result = await self._jsonrpc_http(
            "tools/call", {"name": name, "arguments": arguments}
        )

        if "error" in result:
            return McpToolCallResponse(
                is_error=True,
                error_message=result["error"].get("message", str(result["error"])),
            )

        content = result.get("content", [])
        text_parts = [
            c.get("text", "") for c in content if c.get("type") == "text"
        ]
        return McpToolCallResponse(
            content="\n".join(text_parts) if text_parts else content,
            is_error=result.get("isError", False),
        )

    async def _jsonrpc_http(
        self, method: str, params: dict[str, Any]
    ) -> dict[str, Any]:
        """Send a JSON-RPC request via HTTP POST."""
        if self._http_client is None:
            raise RuntimeError("HTTP client not initialized")

        self._request_id += 1
        body = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": method,
            "params": params,
        }

        resp = await self._http_client.post("/", json=body)
        resp.raise_for_status()
        data = resp.json()

        if "error" in data:
            return {"error": data["error"]}
        return data.get("result", {})

    # ----------------------------------------------------------------
    # SSE transport
    # ----------------------------------------------------------------

    async def _connect_sse(self) -> None:
        """Open SSE stream, get POST endpoint, initialize, discover tools."""
        if not self._config.url:
            raise ValueError("sse transport requires 'url'")

        oauth_headers = await self._oauth_headers()
        base_headers = {**self._config.headers, **oauth_headers}

        self._sse_client = httpx.AsyncClient(
            timeout=httpx.Timeout(connect=10.0, read=None, write=10.0, pool=10.0),
            headers=base_headers,
        )

        # Open SSE stream; first event must be `event: endpoint`
        sse_url = self._config.url.rstrip("/") + "/sse"
        response = await self._sse_client.send(
            self._sse_client.build_request("GET", sse_url),
            stream=True,
        )
        response.raise_for_status()

        # Read first SSE event to get the POST endpoint URL
        post_url: str | None = None
        async for raw_line in response.aiter_lines():
            if raw_line.startswith("data:"):
                post_url = raw_line[5:].strip()
                break

        if not post_url:
            raise RuntimeError("SSE server never sent endpoint URL")

        self._sse_post_url = post_url

        # Start background reader task
        self._sse_reader_task = asyncio.create_task(
            self._sse_reader_loop(response),
            name=f"sse-reader-{self._config.name}",
        )

        # Initialize MCP session over SSE
        await self._jsonrpc_sse("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "agent-core", "version": "2.0"},
        })

        # Discover tools
        tools_resp = await self._jsonrpc_sse("tools/list", {})
        self._tools = [
            McpTool(
                name=t["name"],
                description=t.get("description", ""),
                input_schema=t.get("inputSchema", {}),
                server_name=self._config.name,
            )
            for t in tools_resp.get("tools", [])
        ]

    async def _sse_reader_loop(self, response: httpx.Response) -> None:
        """Background task: read SSE events and resolve waiting futures."""
        event_type = ""
        data_lines: list[str] = []
        try:
            async for raw_line in response.aiter_lines():
                if raw_line.startswith("event:"):
                    event_type = raw_line[6:].strip()
                elif raw_line.startswith("data:"):
                    data_lines.append(raw_line[5:].strip())
                elif raw_line == "":
                    # End of SSE event block
                    if data_lines:
                        raw_data = "\n".join(data_lines)
                        try:
                            msg = json.loads(raw_data)
                            msg_id = msg.get("id")
                            if msg_id is not None and msg_id in self._sse_pending:
                                fut = self._sse_pending.pop(msg_id)
                                if not fut.done():
                                    fut.set_result(msg)
                        except json.JSONDecodeError:
                            pass
                    event_type = ""
                    data_lines = []
        except asyncio.CancelledError:
            pass
        except Exception as exc:
            logger.error("sse_reader_error", extra={"server": self._config.name, "error": str(exc)})
            # Reject all pending futures
            for fut in self._sse_pending.values():
                if not fut.done():
                    fut.set_exception(exc)
            self._sse_pending.clear()

    async def _call_sse(self, name: str, arguments: dict[str, Any]) -> McpToolCallResponse:
        """Call a tool via SSE transport."""
        result = await self._jsonrpc_sse(
            "tools/call", {"name": name, "arguments": arguments},
            timeout=MCP_CALL_TIMEOUT,
        )
        if "error" in result:
            return McpToolCallResponse(
                is_error=True,
                error_message=result["error"].get("message", str(result["error"])),
            )
        content = result.get("content", [])
        text_parts = [c.get("text", "") for c in content if c.get("type") == "text"]
        return McpToolCallResponse(
            content="\n".join(text_parts) if text_parts else content,
            is_error=result.get("isError", False),
        )

    async def _jsonrpc_sse(
        self,
        method: str,
        params: dict[str, Any],
        timeout: float = MCP_INIT_TIMEOUT,
    ) -> dict[str, Any]:
        """POST a JSON-RPC request and await response via SSE."""
        if self._sse_client is None or self._sse_post_url is None:
            raise RuntimeError("SSE client not initialized")

        self._request_id += 1
        req_id = self._request_id
        body = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}

        loop = asyncio.get_running_loop()
        fut: asyncio.Future = loop.create_future()
        self._sse_pending[req_id] = fut

        oauth_headers = await self._oauth_headers()
        await self._sse_client.post(
            self._sse_post_url, json=body, headers=oauth_headers
        )

        try:
            response = await asyncio.wait_for(fut, timeout=timeout)
        except asyncio.TimeoutError:
            self._sse_pending.pop(req_id, None)
            raise TimeoutError(f"SSE response timeout for {method}")

        if "error" in response:
            return {"error": response["error"]}
        return response.get("result", {})

    # ----------------------------------------------------------------
    # WebSocket transport
    # ----------------------------------------------------------------

    async def _connect_ws(self) -> None:
        """Open WebSocket connection, initialize, discover tools."""
        if not self._config.url:
            raise ValueError("ws transport requires 'url'")
        if not _WEBSOCKETS_AVAILABLE:
            raise RuntimeError(
                "websockets package is required for WS transport: pip install websockets"
            )

        ws_url = self._config.url
        if ws_url.startswith("http://"):
            ws_url = "ws://" + ws_url[7:]
        elif ws_url.startswith("https://"):
            ws_url = "wss://" + ws_url[8:]

        extra_headers = {**self._config.headers, **(await self._oauth_headers())}
        self._ws = await _ws_client.connect(ws_url, additional_headers=extra_headers)

        # Start background reader
        self._ws_reader_task = asyncio.create_task(
            self._ws_reader_loop(),
            name=f"ws-reader-{self._config.name}",
        )

        # Initialize MCP session
        await self._jsonrpc_ws("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "agent-core", "version": "2.0"},
        })

        # Discover tools
        tools_resp = await self._jsonrpc_ws("tools/list", {})
        self._tools = [
            McpTool(
                name=t["name"],
                description=t.get("description", ""),
                input_schema=t.get("inputSchema", {}),
                server_name=self._config.name,
            )
            for t in tools_resp.get("tools", [])
        ]

    async def _ws_reader_loop(self) -> None:
        """Background task: read WS messages and resolve waiting futures."""
        try:
            async for raw_msg in self._ws:  # type: ignore[union-attr]
                try:
                    msg = json.loads(raw_msg)
                    msg_id = msg.get("id")
                    if msg_id is not None and msg_id in self._ws_pending:
                        fut = self._ws_pending.pop(msg_id)
                        if not fut.done():
                            fut.set_result(msg)
                except json.JSONDecodeError:
                    pass
        except asyncio.CancelledError:
            pass
        except Exception as exc:
            logger.error("ws_reader_error", extra={"server": self._config.name, "error": str(exc)})
            for fut in self._ws_pending.values():
                if not fut.done():
                    fut.set_exception(exc)
            self._ws_pending.clear()

    async def _call_ws(self, name: str, arguments: dict[str, Any]) -> McpToolCallResponse:
        """Call a tool via WebSocket transport."""
        result = await self._jsonrpc_ws(
            "tools/call", {"name": name, "arguments": arguments},
            timeout=MCP_CALL_TIMEOUT,
        )
        if "error" in result:
            return McpToolCallResponse(
                is_error=True,
                error_message=result["error"].get("message", str(result["error"])),
            )
        content = result.get("content", [])
        text_parts = [c.get("text", "") for c in content if c.get("type") == "text"]
        return McpToolCallResponse(
            content="\n".join(text_parts) if text_parts else content,
            is_error=result.get("isError", False),
        )

    async def _jsonrpc_ws(
        self,
        method: str,
        params: dict[str, Any],
        timeout: float = MCP_INIT_TIMEOUT,
    ) -> dict[str, Any]:
        """Send a JSON-RPC request over WebSocket and await response."""
        if self._ws is None:
            raise RuntimeError("WebSocket not initialized")

        self._request_id += 1
        req_id = self._request_id
        body = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}

        loop = asyncio.get_running_loop()
        fut: asyncio.Future = loop.create_future()
        self._ws_pending[req_id] = fut

        await self._ws.send(json.dumps(body))  # type: ignore[union-attr]

        try:
            response = await asyncio.wait_for(fut, timeout=timeout)
        except asyncio.TimeoutError:
            self._ws_pending.pop(req_id, None)
            raise TimeoutError(f"WS response timeout for {method}")

        if "error" in response:
            return {"error": response["error"]}
        return response.get("result", {})

    # ----------------------------------------------------------------
    # Reconnect + OAuth helpers
    # ----------------------------------------------------------------

    async def reconnect_with_backoff(self, max_retries: int = 5) -> None:
        """Disconnect, then retry connect with exponential back-off.

        Delays: 1, 2, 4, 8, 16 seconds.
        """
        delays = [1, 2, 4, 8, 16]
        await self.disconnect()
        for attempt, delay in enumerate(delays[:max_retries], start=1):
            logger.info(
                "mcp_reconnect_attempt",
                extra={"server": self._config.name, "attempt": attempt, "delay": delay},
            )
            await asyncio.sleep(delay)
            try:
                await self.connect()
                logger.info("mcp_reconnect_success", extra={"server": self._config.name})
                return
            except Exception as exc:
                logger.warning(
                    "mcp_reconnect_failed",
                    extra={"server": self._config.name, "attempt": attempt, "error": str(exc)},
                )
        raise RuntimeError(
            f"MCP server '{self._config.name}' could not reconnect after {max_retries} attempts"
        )

    async def _oauth_headers(self) -> dict[str, str]:
        """Return OAuth Bearer headers if this server has OAuth configured."""
        if self._config.oauth is None:
            return {}
        if self._oauth_manager is None:
            # Requires a Redis client; skip if none provided on config
            redis_client = getattr(self._config, "_redis", None)
            if redis_client is None:
                return {}
            self._oauth_manager = McpOAuthTokenManager(
                redis=redis_client,
                server_id=self._config.name,
                config=self._config.oauth,
            )
        return await self._oauth_manager.auth_headers()
