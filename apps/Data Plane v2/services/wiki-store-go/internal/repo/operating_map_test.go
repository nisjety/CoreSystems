package repo

import (
	"encoding/json"
	"testing"
)

func TestDefaultOperatingMapVersionParsesDurableShape(t *testing.T) {
	runID := "run-1"
	raw := defaultOperatingMapVersion(&runID, []string{"doc-1", "doc-2"})

	parsed, err := parseOperatingMapVersion(raw, json.RawMessage(`["doc-1"]`), &runID)
	if err != nil {
		t.Fatalf("parse default operating map: %v", err)
	}

	if parsed.Confidence <= 0 || parsed.Confidence > 1 {
		t.Fatalf("confidence out of range: %f", parsed.Confidence)
	}

	var phases []map[string]any
	if err := json.Unmarshal(parsed.RolloutPhases, &phases); err != nil {
		t.Fatalf("decode rollout phases: %v", err)
	}
	if len(phases) != 3 {
		t.Fatalf("expected Assist/Ground/Act phases, got %d", len(phases))
	}

	var blueprints []map[string]any
	if err := json.Unmarshal(parsed.AgentBlueprints, &blueprints); err != nil {
		t.Fatalf("decode blueprints: %v", err)
	}
	if len(blueprints) == 0 {
		t.Fatal("expected at least one agent blueprint")
	}

	var refs []string
	if err := json.Unmarshal(parsed.EvidenceRefs, &refs); err != nil {
		t.Fatalf("decode evidence refs: %v", err)
	}
	if len(refs) != 2 {
		t.Fatalf("expected durable evidence refs, got %#v", refs)
	}
}

func TestParseOperatingMapVersionClampsConfidenceAndFallsBackEvidence(t *testing.T) {
	raw := json.RawMessage(`{
		"departments": [],
		"workflows": [],
		"agent_blueprints": [],
		"rollout_phases": [],
		"risk_overlays": [],
		"learning_modules": [],
		"roi_notes": [],
		"confidence": 2.5
	}`)

	parsed, err := parseOperatingMapVersion(raw, json.RawMessage(`["doc-1"]`), nil)
	if err != nil {
		t.Fatalf("parse operating map: %v", err)
	}
	if parsed.Confidence != 1 {
		t.Fatalf("expected confidence clamp to 1, got %f", parsed.Confidence)
	}

	var refs []string
	if err := json.Unmarshal(parsed.EvidenceRefs, &refs); err != nil {
		t.Fatalf("decode evidence refs: %v", err)
	}
	if len(refs) != 1 || refs[0] != "doc-1" {
		t.Fatalf("expected proposal evidence fallback, got %#v", refs)
	}
}

func TestValidOperatingMapBlueprintRole(t *testing.T) {
	for _, role := range []string{"service", "sales", "ecommerce", "chatbot", "workflow"} {
		if !validOperatingMapBlueprintRole(role) {
			t.Fatalf("expected %s to be a supported blueprint role", role)
		}
	}
	if validOperatingMapBlueprintRole("finance") {
		t.Fatal("unexpected unsupported role accepted")
	}
}
