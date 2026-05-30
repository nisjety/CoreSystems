"""Tests for graph_lab graph structures and traversal."""

from __future__ import annotations

import pytest

from graph_lab.graph import GraphEdge, GraphNode, ReasoningGraph
from graph_lab.traversal import traverse_graph


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_node(id: str, *, label: str = "", node_type: str = "concept") -> GraphNode:
    return GraphNode(
        id=id,
        label=label or id,
        content=f"Content for {id}",
        node_type=node_type,
    )


def _make_edge(
    src: str, tgt: str, relation: str = "supports", weight: float = 1.0
) -> GraphEdge:
    return GraphEdge(source_id=src, target_id=tgt, relation=relation, weight=weight)


def _sample_graph() -> ReasoningGraph:
    """Build a small diamond graph: A -> B, A -> C, B -> D, C -> D."""
    g = ReasoningGraph()
    for nid in ("A", "B", "C", "D"):
        g.add_node(_make_node(nid))
    g.add_edge(_make_edge("A", "B"))
    g.add_edge(_make_edge("A", "C"))
    g.add_edge(_make_edge("B", "D"))
    g.add_edge(_make_edge("C", "D"))
    return g


# ---------------------------------------------------------------------------
# Node tests
# ---------------------------------------------------------------------------

class TestGraphNode:
    def test_create_node(self) -> None:
        node = _make_node("n1", label="First", node_type="fact")
        assert node.id == "n1"
        assert node.label == "First"
        assert node.node_type == "fact"
        assert node.created_at is not None

    def test_node_is_frozen(self) -> None:
        node = _make_node("n1")
        with pytest.raises(Exception):
            node.id = "n2"  # type: ignore[misc]


# ---------------------------------------------------------------------------
# Edge tests
# ---------------------------------------------------------------------------

class TestGraphEdge:
    def test_create_edge(self) -> None:
        edge = _make_edge("a", "b", "contradicts", 0.5)
        assert edge.source_id == "a"
        assert edge.target_id == "b"
        assert edge.relation == "contradicts"
        assert edge.weight == 0.5


# ---------------------------------------------------------------------------
# ReasoningGraph tests
# ---------------------------------------------------------------------------

class TestReasoningGraph:
    def test_add_node(self) -> None:
        g = ReasoningGraph()
        g.add_node(_make_node("x"))
        assert "x" in g.nodes

    def test_add_duplicate_node_raises(self) -> None:
        g = ReasoningGraph()
        g.add_node(_make_node("x"))
        with pytest.raises(ValueError, match="already exists"):
            g.add_node(_make_node("x"))

    def test_add_edge(self) -> None:
        g = ReasoningGraph()
        g.add_node(_make_node("a"))
        g.add_node(_make_node("b"))
        g.add_edge(_make_edge("a", "b"))
        assert len(g.edges) == 1

    def test_add_edge_missing_source(self) -> None:
        g = ReasoningGraph()
        g.add_node(_make_node("b"))
        with pytest.raises(ValueError, match="Source node"):
            g.add_edge(_make_edge("a", "b"))

    def test_add_edge_missing_target(self) -> None:
        g = ReasoningGraph()
        g.add_node(_make_node("a"))
        with pytest.raises(ValueError, match="Target node"):
            g.add_edge(_make_edge("a", "b"))

    def test_neighbors(self) -> None:
        g = _sample_graph()
        assert set(g.neighbors("A")) == {"B", "C"}
        assert g.neighbors("D") == []

    def test_neighbors_unknown_node(self) -> None:
        g = ReasoningGraph()
        with pytest.raises(ValueError, match="not found"):
            g.neighbors("nope")

    def test_subgraph(self) -> None:
        g = _sample_graph()
        sub = g.subgraph({"A", "B"})
        assert set(sub.nodes.keys()) == {"A", "B"}
        assert len(sub.edges) == 1
        assert sub.edges[0].source_id == "A"
        assert sub.edges[0].target_id == "B"

    def test_to_networkx(self) -> None:
        g = _sample_graph()
        nx_g = g.to_networkx()
        assert len(nx_g.nodes) == 4
        assert len(nx_g.edges) == 4


# ---------------------------------------------------------------------------
# Traversal tests
# ---------------------------------------------------------------------------

class TestTraversal:
    def test_bfs_full(self) -> None:
        g = _sample_graph()
        result = traverse_graph(g, "A", max_depth=10, strategy="bfs")
        assert result[0] == "A"
        assert set(result) == {"A", "B", "C", "D"}

    def test_dfs_full(self) -> None:
        g = _sample_graph()
        result = traverse_graph(g, "A", max_depth=10, strategy="dfs")
        assert result[0] == "A"
        assert set(result) == {"A", "B", "C", "D"}

    def test_bfs_depth_limit(self) -> None:
        g = _sample_graph()
        result = traverse_graph(g, "A", max_depth=1, strategy="bfs")
        assert result[0] == "A"
        assert "B" in result
        assert "C" in result
        # D is 2 hops away, should not be reached at depth 1
        assert "D" not in result

    def test_dfs_depth_limit(self) -> None:
        g = _sample_graph()
        result = traverse_graph(g, "A", max_depth=1, strategy="dfs")
        assert result[0] == "A"
        assert "D" not in result

    def test_unknown_start(self) -> None:
        g = _sample_graph()
        with pytest.raises(ValueError, match="not found"):
            traverse_graph(g, "Z")

    def test_unknown_strategy(self) -> None:
        g = _sample_graph()
        with pytest.raises(ValueError, match="Unknown traversal"):
            traverse_graph(g, "A", strategy="random")  # type: ignore[arg-type]
