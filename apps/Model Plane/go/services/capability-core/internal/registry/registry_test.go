package registry

import (
	"errors"
	"strings"
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
		capability, err := r.GetForOrg(id, "", "triodelab")
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
	capability, err := r.GetForOrg("operating_map.generate", "", "triodelab")
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

// TestNewRegistry_SeedsSandboxCommandCapabilitySeparatelyFromShell proves the
// hermetic sandbox executor has its own registered, enabled, low-risk
// capability and that registering it did not soften cap.command.shell. Policy
// returns `ask` for high risk, so if shell ever drifted to low the human gate
// on arbitrary command execution would silently disappear.
func TestNewRegistry_SeedsSandboxCommandCapabilitySeparatelyFromShell(t *testing.T) {
	r := newReg(t)

	sandbox, err := r.GetForOrg("cap.command.sandbox", "", "triodelab")
	if err != nil {
		t.Fatalf("expected seeded cap.command.sandbox: %v", err)
	}
	if sandbox.Kind != models.KindCommand {
		t.Fatalf("expected kind=command, got %s", sandbox.Kind)
	}
	if sandbox.RiskLevel != models.RiskLow {
		t.Fatalf("expected cap.command.sandbox risk=low, got %s", sandbox.RiskLevel)
	}
	if !sandbox.Enabled {
		t.Fatal("expected cap.command.sandbox to be enabled")
	}
	if sandbox.IdempotencyKey != idempotencyPrefix+"cap.command.sandbox" {
		t.Fatalf("unexpected idempotency key %q", sandbox.IdempotencyKey)
	}

	shell, err := r.GetForOrg("cap.command.shell", "", "triodelab")
	if err != nil {
		t.Fatalf("expected seeded cap.command.shell: %v", err)
	}
	if shell.RiskLevel != models.RiskHigh {
		t.Fatalf("cap.command.shell must stay risk=high, got %s", shell.RiskLevel)
	}
	if sandbox.ID == shell.ID {
		t.Fatal("sandbox and shell must be distinct capabilities")
	}

	// The low-risk classification only holds because of the hermetic
	// constraints, so the description has to keep stating them: an operator
	// reading the catalog must be able to see why this one is not gated.
	for _, constraint := range []string{
		"read-only root filesystem",
		"networking disabled",
		"wall-clock timeout",
		"throwaway workspace",
		"secret-scrubbed",
	} {
		if !strings.Contains(sandbox.Description, constraint) {
			t.Fatalf("cap.command.sandbox description must state %q", constraint)
		}
	}
}

func TestValidateSkill_SkillExistsAndIsValid(t *testing.T) {
	r := newReg(t)
	capability, errs, err := r.ValidateSkillForOrg("cap.skill.summarize", "triodelab")
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
	_, errs, err := r.ValidateSkillForOrg("cap.tool.http", "triodelab")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(errs) == 0 {
		t.Fatalf("expected validation errors for non-skill capability")
	}
}

func TestCheckPromotion_ValidatesScopeTransition(t *testing.T) {
	r := newReg(t)
	capability, checks, err := r.CheckPromotionForOrg("cap.skill.summarize", "agent", "workspace", "triodelab")
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
	updated, checks, err := r.PromoteSkillForOrg("cap.skill.summarize", "agent", "workspace", "triodelab")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if updated.Scope != "workspace" {
		t.Fatalf("expected updated scope workspace, got %s", updated.Scope)
	}
	if checks[len(checks)-1] != "registry_updated" {
		t.Fatalf("expected registry_updated check, got %v", checks)
	}

	reloaded, err := r.GetForOrg("cap.skill.summarize", "", "triodelab")
	if err != nil {
		t.Fatalf("reload promoted skill: %v", err)
	}
	if reloaded.Scope != "workspace" {
		t.Fatalf("expected persisted scope workspace, got %s", reloaded.Scope)
	}
}

func TestGet_EmptyID_ReturnsInvalidArgument(t *testing.T) {
	r := newReg(t)
	_, err := r.GetForOrg("", "", "triodelab")
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got %v", err)
	}
}

func TestGet_UnknownID_ReturnsNotFound(t *testing.T) {
	r := newReg(t)
	_, err := r.GetForOrg("cap.does.not.exist", "", "triodelab")
	if !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound, got %v", err)
	}
}

func TestGet_ExactVersionMatch(t *testing.T) {
	r := newReg(t)
	c, err := r.GetForOrg("cap.memory.search", "1.1.0", "triodelab")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if c.Version != "1.1.0" {
		t.Fatalf("expected version 1.1.0, got %s", c.Version)
	}
}

func TestGet_VersionMismatch(t *testing.T) {
	r := newReg(t)
	_, err := r.GetForOrg("cap.memory.search", "9.9.9", "triodelab")
	if !errors.Is(err, domain.ErrVersionMismatch) {
		t.Fatalf("expected ErrVersionMismatch, got %v", err)
	}
}

func TestGet_EmptyConstraint_ReturnsAnyVersion(t *testing.T) {
	r := newReg(t)
	c, err := r.GetForOrg("cap.memory.search", "", "triodelab")
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

// newCap builds a synthetic capability seeded with OrgID "global" so any
// caller org resolves it through GetForOrg's global-fallback branch — these
// reload tests exercise Reload/Get mechanics, not tenant ownership, which
// TestRegistryTenantViewsRejectForeignCapabilities and
// TestRegistryTenantViewsRequireOrganization cover separately and always
// override OrgID explicitly regardless of this default.
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
		OrgID:       "global",
		Enabled:     true,
	}
}

func TestRegistry_Reload_AddsNewCapability(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{newCap("cap.test.a", "A", models.KindTool)}}
	r, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	if _, err := r.GetForOrg("cap.test.b", "", "test-org"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected not-found pre-reload, got %v", err)
	}
	src.SetCaps([]*models.Capability{
		newCap("cap.test.a", "A", models.KindTool),
		newCap("cap.test.b", "B", models.KindSkill),
	})
	if err := r.Reload(); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if _, err := r.GetForOrg("cap.test.b", "", "test-org"); err != nil {
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
	if _, err := r.GetForOrg("cap.test.b", "", "test-org"); !errors.Is(err, domain.ErrCapabilityNotFound) {
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
	got, err := r.GetForOrg("cap.test.a", "", "test-org")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.Name != "A-v2" || got.Version != "2.0.0" || got.RiskLevel != models.RiskHigh {
		t.Fatalf("modified fields not applied: %+v", got)
	}
}

func TestRegistry_ReloadRejectsUnknownRiskWithoutReplacingCurrentCatalog(t *testing.T) {
	src := &fakeSource{caps: []*models.Capability{newCap("cap.test.safe", "Safe", models.KindTool)}}
	registry, err := NewFromSource(src)
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	invalid := newCap("cap.test.invalid", "Invalid", models.KindTool)
	invalid.RiskLevel = "critical-ish"
	src.SetCaps([]*models.Capability{invalid})

	if err := registry.Reload(); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("Reload error = %v, want ErrInvalidArgument", err)
	}
	if _, err := registry.GetForOrg("cap.test.safe", "", "test-org"); err != nil {
		t.Fatalf("failed reload replaced prior safe catalog: %v", err)
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

func TestRegistryTenantViewsRejectForeignCapabilities(t *testing.T) {
	t.Parallel()

	global := newCap("cap.shared", "Global", models.KindTool)
	global.OrgID = "global"
	orgOverride := newCap("cap.shared", "Org A", models.KindTool)
	orgOverride.OrgID = "org-a"
	foreign := newCap("cap.foreign", "Org B", models.KindSkill)
	foreign.OrgID = "org-b"
	foreign.Scope = "agent"
	ownedSkill := newCap("cap.owned-skill", "Org A Skill", models.KindSkill)
	ownedSkill.OrgID = "org-a"
	ownedSkill.Scope = "agent"
	disabledGlobal := newCap("cap.disabled-override", "Global enabled", models.KindTool)
	disabledGlobal.OrgID = "global"
	disabledOverride := newCap("cap.disabled-override", "Org A disabled", models.KindTool)
	disabledOverride.OrgID = "org-a"
	disabledOverride.Enabled = false

	r, err := NewFromSource(&fakeSource{caps: []*models.Capability{
		global, foreign, orgOverride, ownedSkill, disabledGlobal, disabledOverride,
	}})
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}

	got, err := r.GetForOrg("cap.shared", "", "org-a")
	if err != nil {
		t.Fatalf("GetForOrg tenant override: %v", err)
	}
	if got.OrgID != "org-a" || got.Name != "Org A" {
		t.Fatalf("tenant override = %+v", got)
	}

	got, err = r.GetForOrg("cap.shared", "", "org-c")
	if err != nil {
		t.Fatalf("GetForOrg global fallback: %v", err)
	}
	if got.OrgID != "global" {
		t.Fatalf("global fallback = %+v", got)
	}

	if _, err := r.GetForOrg("cap.foreign", "", "org-a"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("foreign GetForOrg error = %v", err)
	}
	if _, _, err := r.ValidateSkillForOrg("cap.foreign", "org-a"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("foreign ValidateSkillForOrg error = %v", err)
	}
	if _, _, err := r.CheckPromotionForOrg("cap.foreign", "agent", "workspace", "org-a"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("foreign CheckPromotionForOrg error = %v", err)
	}
	if _, _, err := r.PromoteSkillForOrg("cap.foreign", "agent", "workspace", "org-a"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("foreign PromoteSkillForOrg error = %v", err)
	}
	validated, validationErrors, err := r.ValidateSkillForOrg("cap.owned-skill", "org-a")
	if err != nil || len(validationErrors) != 0 || validated.OrgID != "org-a" {
		t.Fatalf("owned ValidateSkillForOrg = capability=%+v errors=%v err=%v", validated, validationErrors, err)
	}
	checked, checks, err := r.CheckPromotionForOrg("cap.owned-skill", "agent", "workspace", "org-a")
	if err != nil || checked.OrgID != "org-a" || !containsAll(checks, "skill_valid", "source_scope_matches", "target_scope_valid") {
		t.Fatalf("owned CheckPromotionForOrg = capability=%+v checks=%v err=%v", checked, checks, err)
	}
	if unchanged, failedChecks, err := r.PromoteSkillForOrg("cap.owned-skill", "workspace", "user", "org-a"); err != nil || unchanged.Scope != "agent" || !containsAll(failedChecks, "source_scope_mismatch") {
		t.Fatalf("mismatched PromoteSkillForOrg = capability=%+v checks=%v err=%v", unchanged, failedChecks, err)
	}
	promoted, promotionChecks, err := r.PromoteSkillForOrg("cap.owned-skill", "agent", "workspace", "org-a")
	if err != nil || promoted.Scope != "workspace" || !containsAll(promotionChecks, "registry_updated") {
		t.Fatalf("owned PromoteSkillForOrg = capability=%+v checks=%v err=%v", promoted, promotionChecks, err)
	}
	reloaded, err := r.GetForOrg("cap.owned-skill", "", "org-a")
	if err != nil || reloaded.Scope != "workspace" {
		t.Fatalf("promoted tenant capability was not retained: capability=%+v err=%v", reloaded, err)
	}

	items, _ := r.ListForOrg("org-a", "", "", "", 200)
	seen := map[string]*models.Capability{}
	for _, item := range items {
		if item.OrgID == "org-b" {
			t.Fatalf("foreign capability leaked in tenant list: %+v", item)
		}
		if previous := seen[item.ID]; previous != nil {
			t.Fatalf("duplicate capability id %q in tenant list: %+v and %+v", item.ID, previous, item)
		}
		seen[item.ID] = item
	}
	if seen["cap.shared"] == nil || seen["cap.shared"].OrgID != "org-a" {
		t.Fatalf("tenant override missing from list: %+v", seen)
	}
	if seen["cap.owned-skill"] == nil || seen["cap.foreign"] != nil {
		t.Fatalf("tenant list = %+v", seen)
	}
	if seen["cap.disabled-override"] != nil {
		t.Fatalf("disabled tenant override fell back to global entry: %+v", seen["cap.disabled-override"])
	}
}

func TestRegistryTenantViewsRequireOrganization(t *testing.T) {
	t.Parallel()

	r := newReg(t)
	if _, err := r.GetForOrg("cap.memory.search", "", ""); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("GetForOrg missing org error = %v", err)
	}
	if items, more := r.ListForOrg("", "", "", "", 50); items != nil || more {
		t.Fatalf("ListForOrg missing org = %+v, %v", items, more)
	}
	if _, _, err := r.ValidateSkillForOrg("cap.skill.summarize", ""); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("ValidateSkillForOrg missing org error = %v", err)
	}
	if _, _, err := r.CheckPromotionForOrg("cap.skill.summarize", "agent", "workspace", ""); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("CheckPromotionForOrg missing org error = %v", err)
	}
	if _, _, err := r.PromoteSkillForOrg("cap.skill.summarize", "agent", "workspace", ""); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("PromoteSkillForOrg missing org error = %v", err)
	}
}
