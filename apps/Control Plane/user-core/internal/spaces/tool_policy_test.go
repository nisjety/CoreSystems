package spaces

import (
	"strings"
	"testing"
)

// WS10-S5a pure legs: the layer merge is tighten-only, LevelFor falls back to
// "*" then allow, Validate bounds a layer, and the digests move with levels.

func toolSet(layer string, revision int64, entries ...ToolPolicyEntry) *ToolPolicySet {
	return &ToolPolicySet{Layer: layer, SpaceRef: "room-1", Revision: revision, Entries: entries}
}

func entry(key string, level ToolPolicyLevel) ToolPolicyEntry {
	return ToolPolicyEntry{ToolKey: key, Level: level}
}

func TestToolPolicyLevelRankIsOrderedAndClosed(t *testing.T) {
	if !(ToolLevelAllow.Rank() < ToolLevelAsk.Rank() && ToolLevelAsk.Rank() < ToolLevelBlock.Rank()) {
		t.Fatal("level order must be allow < ask < block")
	}
	for _, bad := range []ToolPolicyLevel{"", "deny", "Allow", "BLOCK", "never"} {
		if bad.Valid() || bad.Rank() != 0 {
			t.Fatalf("%q must be invalid with rank 0", bad)
		}
	}
}

// The merge must be at least as restrictive as each layer for every tool key,
// including keys only covered by a wildcard. Exhaustive over a small lattice.
func TestMergeToolPolicyIsTightenOnlyForEveryCombination(t *testing.T) {
	levels := []ToolPolicyLevel{"", ToolLevelAllow, ToolLevelAsk, ToolLevelBlock}
	keys := []string{ToolPolicyWildcard, "a", "b"}
	probe := []string{"a", "b", "c", ToolPolicyWildcard}

	build := func(code int) []ToolPolicyEntry {
		var entries []ToolPolicyEntry
		for _, key := range keys {
			level := levels[code%len(levels)]
			code /= len(levels)
			if level != "" {
				entries = append(entries, entry(key, level))
			}
		}
		return entries
	}
	total := 1
	for range keys {
		total *= len(levels)
	}
	for c := 0; c < total; c++ {
		for a := 0; a < total; a++ {
			ceiling := toolSet(ToolPolicyLayerSpaceCeiling, 1, build(c)...)
			agent := toolSet(ToolPolicyLayerAgent, 1, build(a)...)
			merged := MergeToolPolicy(ceiling, agent)
			for _, key := range probe {
				got := merged.LevelFor(key).Level
				for name, layer := range map[string]*ToolPolicySet{"ceiling": ceiling, "agent": agent} {
					if own, ok := layerLevel(layer.Entries, key); ok && got.Rank() < own.Rank() {
						t.Fatalf("key %q: merged %s is looser than the %s layer's %s (ceiling=%v agent=%v)",
							key, got, name, own, ceiling.Entries, agent.Entries)
					}
				}
			}
		}
	}
}

func TestMergeToolPolicySpecificBeatsWildcardWithinALayer(t *testing.T) {
	ceiling := toolSet(ToolPolicyLayerSpaceCeiling, 3, entry("*", ToolLevelBlock), entry("search", ToolLevelAllow))
	merged := MergeToolPolicy(ceiling, nil)
	if got := merged.LevelFor("search"); got.Level != ToolLevelAllow || got.Source != ToolPolicyLayerSpaceCeiling {
		t.Fatalf("specific allow must beat * block inside one layer: %+v", got)
	}
	if got := merged.LevelFor("anything-else"); got.Level != ToolLevelBlock {
		t.Fatalf("an unnamed tool takes *: %+v", got)
	}
}

func TestMergeToolPolicyAgentCannotLoosenTheCeilingAndTiesNameTheCeiling(t *testing.T) {
	ceiling := toolSet(ToolPolicyLayerSpaceCeiling, 2, entry("send_email", ToolLevelAsk), entry("shell", ToolLevelBlock))
	agent := toolSet(ToolPolicyLayerAgent, 5, entry("send_email", ToolLevelAllow), entry("shell", ToolLevelBlock), entry("read", ToolLevelAsk))
	merged := MergeToolPolicy(ceiling, agent)

	if got := merged.Tools["send_email"]; got.Level != ToolLevelAsk || got.Source != ToolPolicyLayerSpaceCeiling {
		t.Fatalf("an agent allow cannot loosen a ceiling ask: %+v", got)
	}
	if got := merged.Tools["shell"]; got.Level != ToolLevelBlock || got.Source != ToolPolicyLayerSpaceCeiling {
		t.Fatalf("on a tie the ceiling is named as the source: %+v", got)
	}
	if got := merged.Tools["read"]; got.Level != ToolLevelAsk || got.Source != ToolPolicyLayerAgent {
		t.Fatalf("an agent-only restriction is named agent: %+v", got)
	}
	// An agent can tighten what the ceiling allows.
	tight := MergeToolPolicy(
		toolSet(ToolPolicyLayerSpaceCeiling, 1, entry("read", ToolLevelAllow)),
		toolSet(ToolPolicyLayerAgent, 1, entry("read", ToolLevelBlock)))
	if got := tight.Tools["read"]; got.Level != ToolLevelBlock || got.Source != ToolPolicyLayerAgent {
		t.Fatalf("an agent block over a ceiling allow wins: %+v", got)
	}
	if merged.CeilingRevision != 2 || merged.AgentRevision != 5 {
		t.Fatalf("revisions = %d/%d", merged.CeilingRevision, merged.AgentRevision)
	}
}

func TestMergeToolPolicyEmptyAndNilLayers(t *testing.T) {
	merged := MergeToolPolicy(nil, nil)
	if len(merged.Tools) != 0 || merged.Digest == "" {
		t.Fatalf("empty merge: %+v", merged)
	}
	if got := merged.LevelFor("any"); got.Level != ToolLevelAllow || got.Source != ToolPolicySourceDefault {
		t.Fatalf("no layer speaks: allow from default, got %+v", got)
	}
	only := MergeToolPolicy(nil, toolSet(ToolPolicyLayerAgent, 1, entry("x", ToolLevelBlock)))
	if got := only.LevelFor("x"); got.Level != ToolLevelBlock || got.Source != ToolPolicyLayerAgent {
		t.Fatalf("agent-only layer: %+v", got)
	}
}

func TestLevelForFallsBackToWildcardThenAllowDefault(t *testing.T) {
	m := MergedToolPolicy{Tools: map[string]MergedToolLevel{
		"*":    {Level: ToolLevelAsk, Source: ToolPolicyLayerSpaceCeiling},
		"deny": {Level: ToolLevelBlock, Source: ToolPolicyLayerAgent},
	}}
	if got := m.LevelFor("deny"); got.Level != ToolLevelBlock || got.Source != ToolPolicyLayerAgent {
		t.Fatalf("own entry: %+v", got)
	}
	if got := m.LevelFor("other"); got.Level != ToolLevelAsk || got.Source != ToolPolicyLayerSpaceCeiling {
		t.Fatalf("wildcard fallback: %+v", got)
	}
	if got := (MergedToolPolicy{}).LevelFor("other"); got.Level != ToolLevelAllow || got.Source != ToolPolicySourceDefault {
		t.Fatalf("default: %+v", got)
	}
}

func TestToolPolicyUpdateValidate(t *testing.T) {
	ok := ToolPolicyUpdate{Entries: []ToolPolicyEntry{entry("*", ToolLevelAsk), entry("mcp__srv__tool", ToolLevelBlock), entry("execute_provider_action:send", ToolLevelAllow)}}
	if err := ok.Validate(); err != nil {
		t.Fatalf("valid update refused: %v", err)
	}
	if err := (ToolPolicyUpdate{}).Validate(); err != nil {
		t.Fatalf("emptying a layer is valid: %v", err)
	}

	tooMany := make([]ToolPolicyEntry, 0, MaxToolPolicyEntries+1)
	for i := 0; i <= MaxToolPolicyEntries; i++ {
		tooMany = append(tooMany, entry("tool"+strings.Repeat("x", 1)+string(rune('a'+i%26))+strings.Repeat("y", i/26), ToolLevelAsk))
	}
	cases := map[string]ToolPolicyUpdate{
		"negative revision": {ExpectedRevision: -1},
		"duplicate key":     {Entries: []ToolPolicyEntry{entry("a", ToolLevelAsk), entry("a", ToolLevelBlock)}},
		"empty key":         {Entries: []ToolPolicyEntry{entry("", ToolLevelAsk)}},
		"key with space":    {Entries: []ToolPolicyEntry{entry("a b", ToolLevelAsk)}},
		"padded key":        {Entries: []ToolPolicyEntry{entry(" a", ToolLevelAsk)}},
		"control char key":  {Entries: []ToolPolicyEntry{entry("a\tb", ToolLevelAsk)}},
		"oversized key":     {Entries: []ToolPolicyEntry{entry(strings.Repeat("k", maxApprovalKeyLen+1), ToolLevelAsk)}},
		"unknown level":     {Entries: []ToolPolicyEntry{entry("a", "deny")}},
		"empty level":       {Entries: []ToolPolicyEntry{entry("a", "")}},
		"too many entries":  {Entries: tooMany},
	}
	for name, update := range cases {
		if err := update.Validate(); err == nil {
			t.Errorf("%s must be refused", name)
		}
	}
}

func TestToolPolicyDigestIsStableAndMovesWithLevelAndRevision(t *testing.T) {
	build := func(level ToolPolicyLevel, agentRevision int64) MergedToolPolicy {
		return MergeToolPolicy(
			toolSet(ToolPolicyLayerSpaceCeiling, 1, entry("a", ToolLevelAsk), entry("b", ToolLevelAllow)),
			toolSet(ToolPolicyLayerAgent, agentRevision, entry("c", level)))
	}
	base := build(ToolLevelAsk, 1)
	if base.Digest != build(ToolLevelAsk, 1).Digest || !strings.HasPrefix(base.Digest, "sha256:") {
		t.Fatalf("digest must be stable and prefixed: %q", base.Digest)
	}
	if base.Digest != ToolPolicyDigest(base) {
		t.Fatal("Digest must equal ToolPolicyDigest of the view")
	}
	if build(ToolLevelBlock, 1).Digest == base.Digest {
		t.Fatal("a level change must change the digest")
	}
	if build(ToolLevelAsk, 2).Digest == base.Digest {
		t.Fatal("a revision change must change the digest")
	}
	// Map iteration order must not leak into the digest.
	for i := 0; i < 20; i++ {
		if build(ToolLevelAsk, 1).Digest != base.Digest {
			t.Fatal("digest depends on map order")
		}
	}
}

func TestGateViewDigestWithToolsChangesWithToolsAndPolicy(t *testing.T) {
	policy := DefaultApprovalPolicy("room-1", "org-1")
	none := MergeToolPolicy(nil, nil)
	blocked := MergeToolPolicy(toolSet(ToolPolicyLayerSpaceCeiling, 1, entry("shell", ToolLevelBlock)), nil)

	base := GateViewDigestWithTools(policy, none)
	if base != GateViewDigestWithTools(policy, none) {
		t.Fatal("gate view digest must be deterministic")
	}
	if base == GateViewDigestWithTools(policy, blocked) {
		t.Fatal("a tool level must change the gate view digest")
	}
	if base == ApprovalGateViewDigest(policy) {
		t.Fatal("the v2 digest must differ from the tool-less digest")
	}
	changed := policy
	changed.Revision++
	if base == GateViewDigestWithTools(changed, none) {
		t.Fatal("a policy revision change must change the gate view digest")
	}
	changed = policy
	changed.QuarantineMode = "other-mode"
	if base == GateViewDigestWithTools(changed, none) {
		t.Fatal("a quarantine mode change must change the gate view digest")
	}
}

func TestToolPolicyAgentKeyRules(t *testing.T) {
	if key, err := toolPolicyAgentKey(ToolPolicyLayerSpaceCeiling, ""); err != nil || key != "" {
		t.Fatalf("ceiling: %q %v", key, err)
	}
	if _, err := toolPolicyAgentKey(ToolPolicyLayerSpaceCeiling, "agent-1"); err == nil {
		t.Fatal("the ceiling layer must refuse an agent")
	}
	if key, err := toolPolicyAgentKey(ToolPolicyLayerAgent, " agent-1 "); err != nil || key != "agent-1" {
		t.Fatalf("agent layer: %q %v", key, err)
	}
	if _, err := toolPolicyAgentKey(ToolPolicyLayerAgent, ""); err == nil {
		t.Fatal("the agent layer needs an agent")
	}
	if _, err := toolPolicyAgentKey("organisation", ""); err == nil {
		t.Fatal("unknown layers are refused")
	}
}
