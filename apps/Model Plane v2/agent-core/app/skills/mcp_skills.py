"""MCP skill builder — auto-generate SkillConfig from MCP tool listings.

CC pattern: when an MCP server exposes tools, each tool (or group of
related tools) can be wrapped as a skill so the agent can discover and
inject relevant instructions based on the user's goal.

This module provides:
  - ``build_skill_from_mcp_tool()`` — creates a single skill from one MCP tool
  - ``build_skills_from_tools()``   — batch version for a tool list
"""

from __future__ import annotations

from typing import Any

from app.skills.domain import SkillConfig, SkillSource


def build_skill_from_mcp_tool(
    server_name: str,
    tool_name: str,
    description: str,
    input_schema: dict[str, Any] | None = None,
    org_id: str = "__mcp__",
) -> SkillConfig:
    """Create a SkillConfig from an MCP tool definition.

    The generated content includes the tool name, its description,
    and a summary of its input schema so the agent knows when to use it.
    """
    lines = [
        f"# MCP Tool: {tool_name}",
        f"Server: {server_name}",
        "",
        description,
    ]

    if input_schema and "properties" in input_schema:
        lines.append("")
        lines.append("## Parameters")
        for param, spec in input_schema["properties"].items():
            ptype = spec.get("type", "any")
            pdesc = spec.get("description", "")
            required = param in input_schema.get("required", [])
            req_mark = " (required)" if required else ""
            lines.append(f"- `{param}` ({ptype}{req_mark}): {pdesc}")

    content = "\n".join(lines)

    # Derive trigger keywords from tool name parts
    keywords = [w for w in tool_name.replace("-", "_").split("_") if len(w) > 2]

    return SkillConfig(
        org_id=org_id,
        name=f"mcp-{server_name}-{tool_name}",
        description=f"MCP tool '{tool_name}' from {server_name}: {description[:120]}",
        content=content,
        trigger_keywords=keywords,
        source=SkillSource.MCP,
        when_to_use=f"Use when the user needs {tool_name} from {server_name}",
    )


def build_skills_from_tools(
    server_name: str,
    tools: list[dict[str, Any]],
    org_id: str = "__mcp__",
) -> list[SkillConfig]:
    """Batch-convert a list of MCP tool definitions to SkillConfig instances.

    Each tool dict should have at minimum ``name`` and ``description`` keys.
    ``inputSchema`` is optional.
    """
    results: list[SkillConfig] = []
    for tool in tools:
        name = tool.get("name", "")
        desc = tool.get("description", "")
        schema = tool.get("inputSchema") or tool.get("input_schema")
        if not name:
            continue
        results.append(
            build_skill_from_mcp_tool(
                server_name=server_name,
                tool_name=name,
                description=desc,
                input_schema=schema,
                org_id=org_id,
            )
        )
    return results
