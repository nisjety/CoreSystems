package roadmap

import (
	"testing"
)

var validStatuses = map[string]bool{
	"yes":     true,
	"partial": true,
	"no":      true,
}

func TestCatalog_HasAllSections(t *testing.T) {
	c := Load()

	if len(c.ServiceChecklist) == 0 {
		t.Error("ServiceChecklist is empty")
	}
	if len(c.BackendRuntime) == 0 {
		t.Error("BackendRuntime is empty")
	}
	if len(c.ProductShell) == 0 {
		t.Error("ProductShell is empty")
	}
	if len(c.ClaudeDonorRoadmap) == 0 {
		t.Error("ClaudeDonorRoadmap is empty")
	}
	if len(c.ModelPlaneV2Parity) == 0 {
		t.Error("ModelPlaneV2Parity is empty")
	}
}

func TestCatalog_StatusesValid(t *testing.T) {
	c := Load()
	sections := map[string][]Feature{
		"ServiceChecklist":   c.ServiceChecklist,
		"BackendRuntime":     c.BackendRuntime,
		"ProductShell":       c.ProductShell,
		"ClaudeDonorRoadmap": c.ClaudeDonorRoadmap,
		"ModelPlaneV2Parity": c.ModelPlaneV2Parity,
	}
	for name, feats := range sections {
		for _, f := range feats {
			if !validStatuses[f.Status] {
				t.Errorf("%s: feature %q has invalid status %q (want yes|partial|no)", name, f.ID, f.Status)
			}
			if f.ID == "" {
				t.Errorf("%s: feature with empty ID", name)
			}
			if f.Name == "" {
				t.Errorf("%s: feature %q has empty Name", name, f.ID)
			}
		}
	}
}

func TestCatalog_ServiceChecklistCoversAllServices(t *testing.T) {
	required := []string{
		"model-gateway",
		"session-core",
		"inference-core",
		"execution-core",
		"orchestrator-core",
		"capability-core",
		"sandbox-manager",
		"browser-broker",
		"letta-bridge",
	}
	c := Load()
	present := make(map[string]bool, len(c.ServiceChecklist))
	for _, f := range c.ServiceChecklist {
		present[f.ID] = true
	}
	for _, svc := range required {
		if !present[svc] {
			t.Errorf("ServiceChecklist missing required service %q", svc)
		}
	}
}

func TestCatalog_IDsUniqueWithinSection(t *testing.T) {
	c := Load()
	sections := map[string][]Feature{
		"ServiceChecklist":   c.ServiceChecklist,
		"BackendRuntime":     c.BackendRuntime,
		"ProductShell":       c.ProductShell,
		"ClaudeDonorRoadmap": c.ClaudeDonorRoadmap,
		"ModelPlaneV2Parity": c.ModelPlaneV2Parity,
	}
	for name, feats := range sections {
		seen := make(map[string]bool, len(feats))
		for _, f := range feats {
			if seen[f.ID] {
				t.Errorf("%s: duplicate ID %q", name, f.ID)
			}
			seen[f.ID] = true
		}
	}
}

func TestCatalog_ModelPlaneV2ParityCoversCriticalAISurfaces(t *testing.T) {
	required := []string{
		"ai.embeddings",
		"ai.images",
		"ai.speech",
		"ai.translation",
		"ai.document-intelligence",
		"providers.extended",
	}
	c := Load()
	present := make(map[string]bool, len(c.ModelPlaneV2Parity))
	for _, f := range c.ModelPlaneV2Parity {
		present[f.ID] = true
	}
	for _, feature := range required {
		if !present[feature] {
			t.Errorf("ModelPlaneV2Parity missing required feature %q", feature)
		}
	}
}
