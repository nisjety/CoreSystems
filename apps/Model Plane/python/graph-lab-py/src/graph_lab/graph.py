"""Core graph data structures for reasoning experiments."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Literal

import networkx as nx
from pydantic import BaseModel, Field


class GraphNode(BaseModel, frozen=True):
    """A node in a reasoning graph."""

    id: str
    label: str
    content: str
    node_type: Literal["concept", "fact", "claim"]
    metadata: dict[str, object] = Field(default_factory=dict)
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class GraphEdge(BaseModel, frozen=True):
    """A directed edge between two nodes."""

    source_id: str
    target_id: str
    relation: Literal["supports", "contradicts", "elaborates", "causes"]
    weight: float = 1.0


class ReasoningGraph:
    """A directed graph of reasoning nodes and edges.

    Nodes are stored by id in a dict; edges in a list.
    The graph is append-only (immutable-style) — mutations return None but
    the container itself is mutable for ergonomic construction.
    """

    def __init__(self) -> None:
        self._nodes: dict[str, GraphNode] = {}
        self._edges: list[GraphEdge] = []

    # -- accessors (immutable copies) -----------------------------------------

    @property
    def nodes(self) -> dict[str, GraphNode]:
        return dict(self._nodes)

    @property
    def edges(self) -> list[GraphEdge]:
        return list(self._edges)

    # -- mutators --------------------------------------------------------------

    def add_node(self, node: GraphNode) -> None:
        """Add *node* to the graph. Raises ValueError if id already exists."""
        if node.id in self._nodes:
            raise ValueError(f"Node '{node.id}' already exists")
        self._nodes[node.id] = node

    def add_edge(self, edge: GraphEdge) -> None:
        """Add *edge* to the graph.

        Raises ValueError if source or target nodes are missing.
        """
        if edge.source_id not in self._nodes:
            raise ValueError(f"Source node '{edge.source_id}' not found")
        if edge.target_id not in self._nodes:
            raise ValueError(f"Target node '{edge.target_id}' not found")
        self._edges.append(edge)

    # -- queries ---------------------------------------------------------------

    def neighbors(self, node_id: str) -> list[str]:
        """Return ids of nodes reachable via outgoing edges from *node_id*."""
        if node_id not in self._nodes:
            raise ValueError(f"Node '{node_id}' not found")
        return [e.target_id for e in self._edges if e.source_id == node_id]

    def subgraph(self, node_ids: set[str]) -> ReasoningGraph:
        """Return a new ReasoningGraph containing only the given *node_ids*."""
        sub = ReasoningGraph()
        for nid in node_ids:
            if nid in self._nodes:
                sub.add_node(self._nodes[nid])
        for edge in self._edges:
            if edge.source_id in node_ids and edge.target_id in node_ids:
                sub.add_edge(edge)
        return sub

    def to_networkx(self) -> nx.DiGraph:
        """Convert to a NetworkX directed graph."""
        g = nx.DiGraph()
        for nid, node in self._nodes.items():
            g.add_node(nid, **node.model_dump())
        for edge in self._edges:
            g.add_edge(
                edge.source_id,
                edge.target_id,
                relation=edge.relation,
                weight=edge.weight,
            )
        return g
