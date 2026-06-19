package registry

import (
	"errors"
	"sync"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

func newReg(t *testing.T) *Registry {
	t.Helper()
	return NewRegistry()
}

func TestNewRegistry_SeedsAllCapabilities(t *testing.T) {
	r := newReg(t)
	items, hasMore := r.List("", "", "", 200)
	// Seed set has grown past the original 8; assert we have at least the
	// minimum catalog (browser/sandbox/memory/retrieval/tool/inference/skill/plugin)
	// plus the newer kinds (mcp/model/policy/memory_adapter/safety/command).
	if len(items) < 8 {
		t.Fatalf("expected at least 8 seeded capabilities, got %d", len(items))
	}
	if hasMore {
		t.Fatalf("expected hasMore=false when limit exceeds total")
	}
}

func TestNewRegistry_SeedsV2ParityInferenceCapabilities(t *testing.T) {
	r := newReg(t)
	required := map[string]bool{
		"cap.inference.embedding":          true,
		"cap.inference.model-catalog":      true,
		"cap.inference.speech":             true,
		"cap.inference.image":              true,
		"cap.inference.translation":        true,
		"cap.inference.document-intel":     true,
		"cap.inference.language-analytics": true,
		"cap.inference.realtime":           true,
		"cap.inference.video":              true,
	}
	for id, enabled := range required {
		capability, err := r.Get(id, "")
		if err != nil {
			t.Fatalf("expected seeded capability %s: %v", id, err)
		}
		if capability.Kind != models.KindInference {
			t.Fatalf("expected %s kind=inference, got %s", id, capability.Kind)
		}
		if capability.Enabled != enabled {
			t.Fatalf("expected %s enabled=%v, got %v", id, enabled, capability.Enabled)
		}
	}
}

func TestNewRegistry_SeedsOperatingMapGenerateCapability(t *testing.T) {
	r := newReg(t)
	capability, err := r.Get("operating_map.generate", "")
	if err != nil {
		t.Fatalf("expected seeded operating map capability: %v", err)
	}
	if capability.Kind != models.KindInference {
		t.Fatalf("expected kind=inference, got %s", capability.Kind)
	}
	if capability.RiskLevel != models.RiskMedium {
		t.Fatalf("expected risk=medium, got %s", capability.RiskLevel)
	}
	if capability.Scope != "workspace" {
		t.Fatalf("expected workspace scope, got %s", capability.Scope)
	}
	if !capability.Enabled {
		t.Fatalf("expected operating_map.generate to be enabled")
	}
}

func TestValidateSkill_SkillExistsAndIsValid(t *testing.T) {
	r := newReg(t)
	capability, errs, err := r.ValidateSkill("cap.skill.summarize")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(errs) != 0 {
		t.Fatalf("expected no validation errors, got %v", errs)
	}
	if capability.Kind != models.KindSkill {
		t.Fatalf("expected kind=skill, got %s", capability.Kind)
	}
}

func TestValidateSkill_RejectsNonSkill(t *testing.T) {
	r := newReg(t)
	_, errs, err := r.ValidateSkill("cap.tool.http")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(errs) == 0 {
		t.Fatalf("expected validation errors for non-skill capability")
	}
}

func TestCheckPromotion_ValidatesScopeTransition(t *testing.T) {
	r := newReg(t)
	capability, checks, err := r.CheckPromotion("cap.skill.summarize", "agent", "workspace")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if capability.Scope != "agent" {
		t.Fatalf("expected current scope agent, got %s", capability.Scope)
	}
	if len(checks) < 4 {
		t.Fatalf("expected promotion checks, got %v", checks)
	}
}

func TestPromoteSkill_UpdatesScope(t *testing.T) {
	r := newReg(t)
	updated, checks, err := r.PromoteSkill("cap.skill.summarize", "agent", "workspace")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if updated.Scope != "workspace" {
		t.Fatalf("expected updated scope workspace, got %s", updated.Scope)
	}
	if checks[len(checks)-1] != "registry_updated" {
		t.Fatalf("expected registry_updated check, got %v", checks)
	}

	reloaded, err := r.Get("cap.skill.summarize", "")
	if err != nil {
		t.Fatalf("reload promoted skill: %v", err)
	}
	if reloaded.Scope != "workspace" {
		t.Fatalf("expected persisted scope workspace, got %s", reloaded.Scope)
	}
}

func TestGet_EmptyID_ReturnsInvalidArgument(t *testing.T) {
	r := newReg(t)
	_, err := r.Get("", "")
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got %v", err)
	}
}

func TestGet_UnknownID_ReturnsNotFound(t *testing.T) {
	r := newReg(t)
	_, err := r.Get("cap.does.not.exist", "")
	if !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound, got %v", err)
	}
}

func TestGet_ExactVersionMatch(t *testing.T) {
	r := newReg(t)
	c, err := r.Get("cap.memory.search", "1.1.0")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if c.Version != "1.1.0" {
		t.Fatalf("expected version 1.1.0, got %s", c.Version)
	}
}

func TestGet_VersionMismatch(t *testing.T) {
	r := newReg(t)
	_, err := r.Get("cap.memory.search", "9.9.9")
	if !errors.Is(err, domain.ErrVersionMismatch) {
		t.Fatalf("expected ErrVersionMismatch, got %v", err)
	}
}

func TestGet_EmptyConstraint_ReturnsAnyVersion(t *testing.T) {
	r := newReg(t)
	c, err := r.Get("cap.memory.search", "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if c.ID != "cap.memory.search" {
		t.Fatalf("expected cap.memory.search, got %s", c.ID)
	}
}

func TestList_DefaultLimit_ZeroClampsTo50(t *testing.T) {
	r := newReg(t)
	items, _ := r.List("", "", "", 0)
	if len(items) == 0 || len(items) > 50 {
		t.Fatalf("expected seed count in (0, 50] with zero-clamped default limit, got %d", len(items))
	}
}

func TestList_ExcessiveLimit_ClampsTo50(t *testing.T) {
	r := newReg(t)
	items, _ := r.List("", "", "", 500)
	// Clamp is max 50; seed count is ≤ 50 so we should receive all seeds.
	if len(items) == 0 || len(items) > 50 {
		t.Fatalf("expected seed count in (0, 50] with excessive-limit clamp, got %d", len(items))
	}
}

func TestList_KindFilter_Memory(t *testing.T) {
	r := newReg(t)
	items, _ := r.List(string(models.KindMemory), "", "", 200)
	if len(items) != 2 {
		t.Fatalf("expected 2 memory capabilities, got %d", len(items))
	}
	for _, c := range items {
		if c.Kind != models.KindMemory {
			t.Fatalf("expected Kind=memory, got %s (id=%s)", c.Kind, c.ID)
		}
	}
}

func TestList_Query_MatchesNameCaseInsensitive(t *testing.T) {
	r := newReg(t)
	items, _ := r.List("", "MEMORY", "", 200)
	if len(items) == 0 {
		t.Fatalf("expected case-insensitive query to match memory capabilities")
	}
	// Query substring "MEMORY" legitimately matches both KindMemory and
	// KindMemoryAdapter (e.g. cap.memory-adapter.redis). Both are valid hits;
	// only reject hits that share neither prefix.
	for _, c := range items {
		if c.Kind != models.KindMemory && c.Kind != models.KindMemoryAdapter {
			t.Fatalf("unexpected kind %s for id %s", c.Kind, c.ID)
		}
	}
}

func TestList_Query_MatchesDescription(t *testing.T) {
	r := newReg(t)
	items, _ := r.List("", "sandbox", "", 200)
	found := false
	for _, c := range items {
		if c.ID == "cap.sandbox.exec" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected query 'sandbox' to surface cap.sandbox.exec, got %+v", items)
	}
}

func TestList_Pagination_LimitOne_AfterID(t *testing.T) {
	r := newReg(t)
	page1, hasMore := r.List("", "", "", 1)
	if len(page1) != 1 {
		t.Fatalf("expected page1 len=1, got %d", len(page1))
	}
	if !hasMore {
		t.Fatalf("expected hasMore=true on first page")
	}
	page2, _ := r.List("", "", page1[0].ID, 1)
	if len(page2) != 1 {
		t.Fatalf("expected page2 len=1, got %d", len(page2))
	}
	if page2[0].ID == page1[0].ID {
		t.Fatalf("expected afterID to advance cursor, got duplicate %s", page2[0].ID)
	}
}

func TestList_Query_NoMatch_ReturnsEmpty(t *testing.T) {
	r := newReg(t)
	items, hasMore := r.List("", "zzznevermatch", "", 200)
	if len(items) != 0 {
		t.Fatalf("expected empty result, got %d", len(items))
	}
	if hasMore {
		t.Fatalf("expected hasMore=false on empty filter")
	}
}

// ---------------------------------------------------------------------------
// Dynamic reload tests (RED).
// ---------------------------------------------------------------------------

type fakeSource struct {
	mu   sync.Mutex
	caps []*models.Capability
}

func (f *fakeSource) Load() ([]*models.Capability, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]*models.Capability, len(f.caps))
	for i, c := range f.caps {
		cp := *c
		out[i] = &cp
	}
	return out, nil
}

func (f *fakeSource) SetCaps(caps []*models.Capability) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.caps = caps
}

func newCap(id, name, kind string) *models.Capability {
	return &models.Capability{
		ID:          id,
		Name:        name,
		Kind:        kind,
		Version:     "1.0.0",
		Description: "test capability " + id,
		RiskLevel:   models.RiskLow,
		LazyLoad:    false,
		Scope:       "global",
		Enabled:     true,
	}
}

func TestRegistry_Reload_AddsNewCapability(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{newCap("cap.test.a", "A", models.KindTool)}}
	r, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	if _, err := r.Get("cap.test.b", ""); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected not-found pre-reload, got %v", err)
	}
	src.SetCaps([]*models.Capability{
		newCap("cap.test.a", "A", models.KindTool),
		newCap("cap.test.b", "B", models.KindSkill),
	})
	if err := r.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if _, err := r.Get("cap.test.b", ""); err != nil {
		t.Fatalf("expected cap.test.b after reload, got %v", err)
	}
}

func TestRegistry_Reload_RemovesDeletedCapability(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{
		newCap("cap.test.a", "A", models.KindTool),
		newCap("cap.test.b", "B", models.KindSkill),
	}}
	r, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	src.SetCaps([]*models.Capability{newCap("cap.test.a", "A", models.KindTool)})
	if err := r.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if _, err := r.Get("cap.test.b", ""); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected cap.test.b removed after reload, got %v", err)
	}
}

func TestRegistry_Reload_AppliesModifiedFields(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{newCap("cap.test.a", "A", models.KindTool)}}
	r, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	modified := newCap("cap.test.a", "A-v2", models.KindTool)
	modified.Version = "2.0.0"
	modified.RiskLevel = models.RiskHigh
	src.SetCaps([]*models.Capability{modified})
	if err := r.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	got, err := r.Get("cap.test.a", "")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.Name != "A-v2" || got.Version != "2.0.0" || got.RiskLevel != models.RiskHigh {
		t.Fatalf("modified fields not applied: %+v", got)
	}
}

func TestRegistry_Reload_ConcurrentReadsSafe(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{
		newCap("cap.test.a", "A", models.KindTool),
		newCap("cap.test.b", "B", models.KindSkill),
	}}
	r, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	var wg sync.WaitGroup
	stop := make(chan struct{})
	for i := 0; i < 4; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				select {
				case <-stop:
					return
				default:
					_, _ = r.List("", "", "", 100)
				}
			}
		}()
	}
	for i := 0; i < 50; i++ {
		src.SetCaps([]*models.Capability{
			newCap("cap.test.a", "A", models.KindTool),
			newCap("cap.test.c", "C", models.KindRetrieval),
		})
		if err := r.Reload(); err != nil {
			close(stop)
			wg.Wait()
			t.Fatalf("Reload iter %d: %v", i, err)
		}
	}
	close(stop)
	wg.Wait()
}
