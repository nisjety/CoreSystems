//! Subagent lineage records. Owned by `session-core`.
//!
//! Tracks parent/child relationships between runs so the full execution graph
//! can be reconstructed for audit, recovery, and summarization.
//!
//! Invariants enforced:
//!   - A run cannot be its own parent.
//!   - A run has at most one direct parent.
//!   - Cycles are rejected (enforced at graph-building time via `attach`).
//!   - Depth is bounded (configurable per thread).

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{OrchestrationError, OrchestrationResult};

/// Role of a subagent relative to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentRole {
    /// A "coder" subagent — implements a discrete task.
    Coder,
    /// A "reviewer" subagent — reviews output of another agent.
    Reviewer,
    /// A "researcher" subagent — gathers context.
    Researcher,
    /// A "explorer" subagent — surveys a codebase or domain.
    Explorer,
    /// Generic subagent without a specialized role.
    Generic,
}

/// One edge in the lineage graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageEdge {
    /// Parent run id.
    pub parent_run_id: String,
    /// Child run id.
    pub child_run_id: String,
    /// Child's role.
    pub role: SubagentRole,
    /// When the child was spawned.
    pub spawned_at: DateTime<Utc>,
}

/// Subagent lineage graph for one thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubagentLineage {
    /// Thread this lineage belongs to.
    pub thread_id: String,
    /// Maximum allowed depth (0 means no bound).
    #[serde(default)]
    pub max_depth: u32,
    /// All edges in the graph.
    #[serde(default)]
    pub edges: Vec<LineageEdge>,
}

impl SubagentLineage {
    const KIND: &'static str = "subagent_lineage";

    /// Construct an empty lineage graph for a thread.
    ///
    /// # Errors
    /// - `MissingField("thread_id")` when the thread id is empty.
    pub fn new(thread_id: impl Into<String>, max_depth: u32) -> OrchestrationResult<Self> {
        let thread_id = thread_id.into();
        if thread_id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "thread_id",
            });
        }
        Ok(Self {
            thread_id,
            max_depth,
            edges: Vec::new(),
        })
    }

    /// Attach a child run to a parent. Enforces: no self-parent, no cycle, no
    /// duplicate-edge, no depth overrun, no second-parent for an existing child.
    ///
    /// # Errors
    /// - `InvariantViolation` on any rule breach.
    /// - `MissingField` when any id is empty.
    pub fn attach(
        &mut self,
        parent_run_id: impl Into<String>,
        child_run_id: impl Into<String>,
        role: SubagentRole,
        spawned_at: DateTime<Utc>,
    ) -> OrchestrationResult<()> {
        let parent = parent_run_id.into();
        let child = child_run_id.into();
        if parent.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "parent_run_id",
            });
        }
        if child.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "child_run_id",
            });
        }
        if parent == child {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: format!("run {parent} cannot be its own parent"),
            });
        }
        if self.edges.iter().any(|e| e.child_run_id == child) {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: format!("child {child} already has a parent"),
            });
        }

        // Cycle check: attaching parent→child loops iff child is already an
        // ancestor of parent. Walk parent's chain; if child appears, closing
        // the edge would reach itself.
        if self.is_ancestor(&parent, &child) {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: format!("attaching {parent} → {child} would create a cycle"),
            });
        }

        // Depth bound: depth(parent) + 1 <= max_depth (when max_depth > 0).
        if self.max_depth > 0 {
            let d = self.depth_of(&parent).saturating_add(1);
            if d > self.max_depth {
                return Err(OrchestrationError::InvariantViolation {
                    kind: Self::KIND,
                    detail: format!(
                        "attaching {parent} → {child} would exceed max_depth {}",
                        self.max_depth
                    ),
                });
            }
        }

        self.edges.push(LineageEdge {
            parent_run_id: parent,
            child_run_id: child,
            role,
            spawned_at,
        });
        Ok(())
    }

    /// Return the parent run id for `run_id`, if any.
    #[must_use]
    pub fn parent_of(&self, run_id: &str) -> Option<&str> {
        self.edges
            .iter()
            .find(|e| e.child_run_id == run_id)
            .map(|e| e.parent_run_id.as_str())
    }

    /// Return the direct children of `run_id`.
    #[must_use]
    pub fn children_of(&self, run_id: &str) -> Vec<&str> {
        self.edges
            .iter()
            .filter(|e| e.parent_run_id == run_id)
            .map(|e| e.child_run_id.as_str())
            .collect()
    }

    /// Walk ancestors of `run_id` (parent, grandparent, …). Terminates at root.
    #[must_use]
    pub fn ancestors_of(&self, run_id: &str) -> Vec<String> {
        let parents: HashMap<&str, &str> = self
            .edges
            .iter()
            .map(|e| (e.child_run_id.as_str(), e.parent_run_id.as_str()))
            .collect();
        let mut out = Vec::new();
        let mut cur = run_id;
        while let Some(p) = parents.get(cur) {
            out.push((*p).to_owned());
            cur = *p;
        }
        out
    }

    /// BFS-collected descendants of `run_id`.
    #[must_use]
    pub fn descendants_of(&self, run_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut queue: VecDeque<&str> = VecDeque::from([run_id]);
        let mut seen: HashSet<String> = HashSet::from([run_id.to_owned()]);
        while let Some(node) = queue.pop_front() {
            for child in self.children_of(node) {
                if seen.insert(child.to_owned()) {
                    out.push(child.to_owned());
                    queue.push_back(child);
                }
            }
        }
        out
    }

    /// Depth of `run_id` (0 for a root).
    #[must_use]
    pub fn depth_of(&self, run_id: &str) -> u32 {
        u32::try_from(self.ancestors_of(run_id).len()).unwrap_or(u32::MAX)
    }

    /// True if `candidate_ancestor` is on the ancestor chain of `run_id`.
    fn is_ancestor(&self, run_id: &str, candidate_ancestor: &str) -> bool {
        self.ancestors_of(run_id)
            .iter()
            .any(|a| a == candidate_ancestor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    fn fresh(max: u32) -> SubagentLineage {
        SubagentLineage::new("thread_01", max).expect("ctor")
    }

    #[test]
    fn rejects_self_parent() {
        let mut g = fresh(0);
        let err = g
            .attach("run_A", "run_A", SubagentRole::Generic, now())
            .unwrap_err();
        assert_eq!(err.code(), "INVARIANT_VIOLATION");
    }

    #[test]
    fn attaches_simple_parent_child() {
        let mut g = fresh(0);
        g.attach("run_A", "run_B", SubagentRole::Coder, now())
            .unwrap();
        assert_eq!(g.parent_of("run_B"), Some("run_A"));
        assert_eq!(g.children_of("run_A"), vec!["run_B"]);
    }

    #[test]
    fn rejects_duplicate_parent() {
        let mut g = fresh(0);
        g.attach("run_A", "run_B", SubagentRole::Generic, now())
            .unwrap();
        assert!(g
            .attach("run_C", "run_B", SubagentRole::Generic, now())
            .is_err());
    }

    #[test]
    fn rejects_cycle() {
        let mut g = fresh(0);
        g.attach("run_A", "run_B", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_B", "run_C", SubagentRole::Generic, now())
            .unwrap();
        // run_C → run_A would form A→B→C→A.
        let err = g
            .attach("run_C", "run_A", SubagentRole::Generic, now())
            .unwrap_err();
        assert_eq!(err.code(), "INVARIANT_VIOLATION");
    }

    #[test]
    fn depth_and_ancestors_walk_chain() {
        let mut g = fresh(0);
        g.attach("run_A", "run_B", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_B", "run_C", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_C", "run_D", SubagentRole::Generic, now())
            .unwrap();

        assert_eq!(g.depth_of("run_A"), 0);
        assert_eq!(g.depth_of("run_B"), 1);
        assert_eq!(g.depth_of("run_C"), 2);
        assert_eq!(g.depth_of("run_D"), 3);
        assert_eq!(g.ancestors_of("run_D"), vec!["run_C", "run_B", "run_A"]);
    }

    #[test]
    fn max_depth_enforced() {
        let mut g = fresh(2);
        g.attach("run_A", "run_B", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_B", "run_C", SubagentRole::Generic, now())
            .unwrap();
        // Next attach would be at depth 3; max is 2.
        let err = g
            .attach("run_C", "run_D", SubagentRole::Generic, now())
            .unwrap_err();
        assert_eq!(err.code(), "INVARIANT_VIOLATION");
    }

    #[test]
    fn descendants_are_bfs() {
        let mut g = fresh(0);
        g.attach("run_A", "run_B", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_A", "run_C", SubagentRole::Generic, now())
            .unwrap();
        g.attach("run_B", "run_D", SubagentRole::Generic, now())
            .unwrap();

        let mut d = g.descendants_of("run_A");
        d.sort();
        assert_eq!(d, vec!["run_B", "run_C", "run_D"]);
    }

    #[test]
    fn lineage_roundtrips_json() {
        let mut g = fresh(4);
        g.attach("run_A", "run_B", SubagentRole::Reviewer, now())
            .unwrap();
        let s = serde_json::to_string(&g).unwrap();
        let back: SubagentLineage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.edges.len(), 1);
        assert_eq!(back.edges[0].role, SubagentRole::Reviewer);
    }
}
