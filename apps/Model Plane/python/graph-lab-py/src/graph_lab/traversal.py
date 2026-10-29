"""Graph traversal strategies for reasoning graphs."""

from __future__ import annotations

from collections import deque
from typing import Literal

from graph_lab.graph import ReasoningGraph


def traverse_graph(
    graph: ReasoningGraph,
    start_id: str,
    *,
    max_depth: int = 3,
    strategy: Literal["bfs", "dfs"] = "bfs",
) -> list[str]:
    """Traverse *graph* from *start_id* up to *max_depth* hops.

    Args:
        graph: The reasoning graph to traverse.
        start_id: Node id to begin traversal from.
        max_depth: Maximum traversal depth (default 3).
        strategy: ``"bfs"`` for breadth-first (default) or ``"dfs"`` for
            depth-first traversal.

    Returns:
        Ordered list of visited node ids.

    Raises:
        ValueError: If *start_id* is not in the graph or *strategy* is
            unrecognised.
    """
    if start_id not in graph.nodes:
        raise ValueError(f"Start node '{start_id}' not found in graph")

    if strategy == "bfs":
        return _bfs(graph, start_id, max_depth)
    if strategy == "dfs":
        return _dfs(graph, start_id, max_depth)

    raise ValueError(f"Unknown traversal strategy: '{strategy}'")


def _bfs(graph: ReasoningGraph, start_id: str, max_depth: int) -> list[str]:
    visited: list[str] = []
    seen: set[str] = {start_id}
    queue: deque[tuple[str, int]] = deque([(start_id, 0)])

    while queue:
        current, depth = queue.popleft()
        visited.append(current)
        if depth >= max_depth:
            continue
        for neighbor_id in graph.neighbors(current):
            if neighbor_id not in seen:
                seen.add(neighbor_id)
                queue.append((neighbor_id, depth + 1))

    return visited


def _dfs(graph: ReasoningGraph, start_id: str, max_depth: int) -> list[str]:
    visited: list[str] = []
    seen: set[str] = set()

    def _visit(node_id: str, depth: int) -> None:
        if node_id in seen:
            return
        seen.add(node_id)
        visited.append(node_id)
        if depth >= max_depth:
            return
        for neighbor_id in graph.neighbors(node_id):
            _visit(neighbor_id, depth + 1)

    _visit(start_id, 0)
    return visited
