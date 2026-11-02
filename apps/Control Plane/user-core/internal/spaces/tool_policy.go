package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strconv"
	"strings"
	"unicode"
)

// WS10-S5a: the per-tool policy of a shared Space. Two layers, both tighten-only:
// the Space ceiling (every agent of the Space) and the agent layer (one agent).
// The level order is allow < ask < block. Across layers the result is the most
// restrictive; within a layer a specific tool key beats "*". An absent row means
// allow, and allow never loosens a default gate (risk, posture, never-auto set);
// only a standing rule or a once decision waives a prompt. Execution Core folds
// the merged view with its own sources by max_restrictive, so nothing computed
// here can loosen anything.

// ToolPolicyLevel is how a tool call is treated.
type ToolPolicyLevel string

const (
	ToolLevelAllow ToolPolicyLevel = "allow"
	ToolLevelAsk   ToolPolicyLevel = "ask"
	ToolLevelBlock ToolPolicyLevel = "block"
)

// Rank orders the levels by restriction. Unknown values rank 0 and are invalid.
func (l ToolPolicyLevel) Rank() int {
	switch l {
	case ToolLevelAllow:
		return 1
	case ToolLevelAsk:
		return 2
	case ToolLevelBlock:
		return 3
	}
	return 0
}

// Valid reports whether l is one of the three closed levels.
func (l ToolPolicyLevel) Valid() bool { return l.Rank() > 0 }

// Policy layers stored by Control. The organisation floor is read from
// capability-core when that plane delivers it; it is not stored here.
const (
	ToolPolicyLayerSpaceCeiling = "space_ceiling"
	ToolPolicyLayerAgent        = "agent"
	// ToolPolicySourceDefault marks a tool no layer speaks for.
	ToolPolicySourceDefault = "default"
	// ToolPolicyWildcard is the tool key that covers every tool of a layer
	// without a specific row.
	ToolPolicyWildcard = "*"
	// MaxToolPolicyEntries bounds one layer so a policy write stays small.
	MaxToolPolicyEntries = 200
)

// ToolPolicyEntry is one row of a layer: a tool key and its level.
type ToolPolicyEntry struct {
	ToolKey string          `json:"tool_key"`
	Level   ToolPolicyLevel `json:"level"`
}

// ToolPolicySet is one stored layer. Revision 0 means "nothing stored yet": a
// first write presents expected_revision 0.
type ToolPolicySet struct {
	Layer          string            `json:"layer"`
	SpaceRef       string            `json:"space_ref"`
	AgentSubjectID string            `json:"agent_subject_id,omitempty"`
	Revision       int64             `json:"revision"`
	UpdatedBy      string            `json:"updated_by,omitempty"`
	Entries        []ToolPolicyEntry `json:"entries"`
}

// ToolPolicyUpdate replaces one whole layer under a revision compare-and-set.
type ToolPolicyUpdate struct {
	Entries          []ToolPolicyEntry `json:"entries"`
	ExpectedRevision int64             `json:"expected_revision"`
}

// validToolPolicyKey accepts "*" or a printable key without whitespace or
// control characters (tool names, `mcp__server__tool`, `execute_provider_action:op`).
func validToolPolicyKey(key string) bool {
	if key != strings.TrimSpace(key) || key == "" || len(key) > maxApprovalKeyLen {
		return false
	}
	for _, r := range key {
		if unicode.IsSpace(r) || unicode.IsControl(r) {
			return false
		}
	}
	return true
}

// Validate checks shape only; authority is resolved separately. It rejects
// duplicate keys, unknown levels and oversized layers.
func (u ToolPolicyUpdate) Validate() error {
	if u.ExpectedRevision < 0 {
		return fmt.Errorf("expected_revision must not be negative")
	}
	if len(u.Entries) > MaxToolPolicyEntries {
		return fmt.Errorf("a tool policy holds at most %d entries", MaxToolPolicyEntries)
	}
	seen := make(map[string]struct{}, len(u.Entries))
	for _, entry := range u.Entries {
		if !validToolPolicyKey(entry.ToolKey) {
			return fmt.Errorf("tool_key must be * or a tool name without whitespace, up to %d characters", maxApprovalKeyLen)
		}
		if !entry.Level.Valid() {
			return fmt.Errorf("level must be allow, ask or block")
		}
		if _, dup := seen[entry.ToolKey]; dup {
			return fmt.Errorf("tool_key %q appears twice", entry.ToolKey)
		}
		seen[entry.ToolKey] = struct{}{}
	}
	return nil
}

// layerLevel is the level one layer gives a tool key: the specific row, else
// the wildcard row, else nothing.
func layerLevel(entries []ToolPolicyEntry, toolKey string) (ToolPolicyLevel, bool) {
	var wildcard ToolPolicyLevel
	for _, entry := range entries {
		if entry.ToolKey == toolKey {
			return entry.Level, true
		}
		if entry.ToolKey == ToolPolicyWildcard {
			wildcard = entry.Level
		}
	}
	if wildcard != "" {
		return wildcard, true
	}
	return "", false
}

// MergedToolLevel is the result for one tool key and the layer that decided it.
type MergedToolLevel struct {
	Level  ToolPolicyLevel `json:"level"`
	Source string          `json:"source"`
}

// MergedToolPolicy is the view Execution Core and the editor read. Tools holds
// every key some layer names (including "*"); a tool not in Tools takes the
// "*" entry when present, else allow from ToolPolicySourceDefault.
type MergedToolPolicy struct {
	Tools           map[string]MergedToolLevel `json:"tools"`
	CeilingRevision int64                      `json:"ceiling_revision"`
	AgentRevision   int64                      `json:"agent_revision"`
	Digest          string                     `json:"digest"`
}

// MergeToolPolicy folds the Space ceiling and the agent layer (either may be
// nil or empty). The result for each named key is the most restrictive of the
// layers; on a tie the Space ceiling is named as the source because an agent
// layer cannot override it. The merge is tighten-only by construction: the
// result is never less restrictive than either input for any key.
func MergeToolPolicy(ceiling, agent *ToolPolicySet) MergedToolPolicy {
	var ceilingEntries, agentEntries []ToolPolicyEntry
	merged := MergedToolPolicy{Tools: map[string]MergedToolLevel{}}
	if ceiling != nil {
		ceilingEntries = ceiling.Entries
		merged.CeilingRevision = ceiling.Revision
	}
	if agent != nil {
		agentEntries = agent.Entries
		merged.AgentRevision = agent.Revision
	}
	keys := map[string]struct{}{}
	for _, entry := range ceilingEntries {
		keys[entry.ToolKey] = struct{}{}
	}
	for _, entry := range agentEntries {
		keys[entry.ToolKey] = struct{}{}
	}
	for key := range keys {
		result := MergedToolLevel{Level: ToolLevelAllow, Source: ToolPolicySourceDefault}
		if level, ok := layerLevel(ceilingEntries, key); ok {
			result = MergedToolLevel{Level: level, Source: ToolPolicyLayerSpaceCeiling}
		}
		if level, ok := layerLevel(agentEntries, key); ok {
			if result.Source == ToolPolicySourceDefault || level.Rank() > result.Level.Rank() {
				result = MergedToolLevel{Level: level, Source: ToolPolicyLayerAgent}
			}
		}
		merged.Tools[key] = result
	}
	merged.Digest = ToolPolicyDigest(merged)
	return merged
}

// LevelFor answers the merged level of one tool: its own entry, else "*", else
// allow from the default source. Allow here still leaves every other gate in
// force.
func (m MergedToolPolicy) LevelFor(toolKey string) MergedToolLevel {
	if level, ok := m.Tools[toolKey]; ok {
		return level
	}
	if level, ok := m.Tools[ToolPolicyWildcard]; ok {
		return level
	}
	return MergedToolLevel{Level: ToolLevelAllow, Source: ToolPolicySourceDefault}
}

// ToolPolicyDigest commits to the layer revisions and every merged level, so a
// gate that cached a view can tell it from the next one without comparing
// bodies. It carries keys and levels only.
func ToolPolicyDigest(m MergedToolPolicy) string {
	keys := make([]string, 0, len(m.Tools))
	for key := range m.Tools {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	var b strings.Builder
	b.WriteString("tool-policy-view-v1\x00")
	b.WriteString(strconv.FormatInt(m.CeilingRevision, 10))
	b.WriteString("\x00")
	b.WriteString(strconv.FormatInt(m.AgentRevision, 10))
	for _, key := range keys {
		level := m.Tools[key]
		b.WriteString("\x00")
		b.WriteString(key)
		b.WriteString("=")
		b.WriteString(string(level.Level))
		b.WriteString("@")
		b.WriteString(level.Source)
	}
	sum := sha256.Sum256([]byte(b.String()))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// GateViewDigestWithTools extends ApprovalGateViewDigest with the merged tool
// policy, so the digest changes when the ceiling, the quarantine mode or any
// tool level changes.
func GateViewDigestWithTools(policy ApprovalPolicy, tools MergedToolPolicy) string {
	sum := sha256.Sum256([]byte("approval-gate-view-v2\x00" + ApprovalGateViewDigest(policy) + "\x00" + tools.Digest))
	return "sha256:" + hex.EncodeToString(sum[:])
}
