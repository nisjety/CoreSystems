package registry

import (
	"context"
	"errors"
	"testing"

	"github.com/google/uuid"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// fakeModelsLoader lets tests exercise ModelsCache without a real Postgres
// pool, mirroring the fake Source pattern used for the capabilities Registry
// (see capabilitySourceStub in the server package).
type fakeModelsLoader struct {
	items []*Model
	err   error
	calls int
}

func (f *fakeModelsLoader) List(_ context.Context, _ ModelsFilter) ([]*Model, error) {
	f.calls++
	if f.err != nil {
		return nil, f.err
	}
	return f.items, nil
}

var (
	orgA = uuid.MustParse("11111111-1111-4111-8111-111111111111")
	orgB = uuid.MustParse("22222222-2222-4222-8222-222222222222")
)

func TestNewModelsCache_NilLoader(t *testing.T) {
	if _, err := NewModelsCache(context.Background(), nil); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got %v", err)
	}
}

func TestNewModelsCache_PropagatesLoadError(t *testing.T) {
	loader := &fakeModelsLoader{err: errors.New("boom")}
	if _, err := NewModelsCache(context.Background(), loader); err == nil {
		t.Fatal("expected initial load failure to propagate")
	}
}

func TestModelsCache_ListAsCapabilitiesForOrg_GlobalAndTenantOverlay(t *testing.T) {
	global := &Model{OrgID: uuid.Nil, Provider: "openai", Name: "gpt-4o", Enabled: true, PrivacyTier: models.PrivacyTierUnspecified}
	tenantOnly := &Model{OrgID: orgA, Provider: "acme", Name: "custom", Enabled: true, PrivacyTier: models.PrivacyTierEUResident, Residency: "eu"}
	disabled := &Model{OrgID: uuid.Nil, Provider: "openai", Name: "disabled-model", Enabled: false}
	otherTenant := &Model{OrgID: orgB, Provider: "acme", Name: "other-org-only", Enabled: true}

	loader := &fakeModelsLoader{items: []*Model{global, tenantOnly, disabled, otherTenant}}
	cache, err := NewModelsCache(context.Background(), loader)
	if err != nil {
		t.Fatalf("NewModelsCache: %v", err)
	}

	caps, err := cache.ListAsCapabilitiesForOrg(context.Background(), orgA.String())
	if err != nil {
		t.Fatalf("ListAsCapabilitiesForOrg: %v", err)
	}
	ids := make(map[string]*models.Capability, len(caps))
	for _, c := range caps {
		ids[c.ID] = c
	}
	if _, ok := ids["cap.model.openai.gpt-4o"]; !ok {
		t.Fatal("expected global model visible to org A")
	}
	if _, ok := ids["cap.model.acme.custom"]; !ok {
		t.Fatal("expected org A's own model visible")
	}
	if _, ok := ids["cap.model.openai.disabled-model"]; ok {
		t.Fatal("disabled model must not be listed")
	}
	if _, ok := ids["cap.model.acme.other-org-only"]; ok {
		t.Fatal("org B's model must not leak to org A")
	}
	if got := ids["cap.model.acme.custom"].PrivacyTier; got != models.PrivacyTierEUResident {
		t.Fatalf("expected privacy tier eu_resident, got %q", got)
	}
	if got := ids["cap.model.acme.custom"].Residency; got != "eu" {
		t.Fatalf("expected residency eu, got %q", got)
	}
}

func TestModelsCache_GetByCapabilityIDForOrg_TenantWinsOverGlobal(t *testing.T) {
	global := &Model{OrgID: uuid.Nil, Provider: "openai", Name: "gpt-4o", Version: "global-version"}
	tenant := &Model{OrgID: orgA, Provider: "openai", Name: "gpt-4o", Version: "org-a-version"}
	loader := &fakeModelsLoader{items: []*Model{global, tenant}}
	cache, err := NewModelsCache(context.Background(), loader)
	if err != nil {
		t.Fatalf("NewModelsCache: %v", err)
	}

	got, err := cache.GetByCapabilityIDForOrg(context.Background(), "cap.model.openai.gpt-4o", orgA.String())
	if err != nil {
		t.Fatalf("GetByCapabilityIDForOrg: %v", err)
	}
	if got.Version != "org-a-version" {
		t.Fatalf("expected tenant-owned row to win, got version %q", got.Version)
	}

	got, err = cache.GetByCapabilityIDForOrg(context.Background(), "cap.model.openai.gpt-4o", orgB.String())
	if err != nil {
		t.Fatalf("GetByCapabilityIDForOrg for org without an override: %v", err)
	}
	if got.Version != "global-version" {
		t.Fatalf("expected global fallback for org without an override, got version %q", got.Version)
	}
}

func TestModelsCache_GetByCapabilityIDForOrg_NotFoundAndMalformed(t *testing.T) {
	cache, err := NewModelsCache(context.Background(), &fakeModelsLoader{})
	if err != nil {
		t.Fatalf("NewModelsCache: %v", err)
	}

	if _, err := cache.GetByCapabilityIDForOrg(context.Background(), "bogus", orgA.String()); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound for bad prefix, got %v", err)
	}
	if _, err := cache.GetByCapabilityIDForOrg(context.Background(), "cap.model.foo", orgA.String()); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument for malformed id, got %v", err)
	}
	if _, err := cache.GetByCapabilityIDForOrg(context.Background(), "cap.model.openai.gpt-4o", orgA.String()); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound for unknown model, got %v", err)
	}
}

func TestModelsCache_Reload_SwapsSnapshotAndDoesNotHitLoaderOnReads(t *testing.T) {
	loader := &fakeModelsLoader{items: []*Model{{OrgID: uuid.Nil, Provider: "openai", Name: "gpt-4o", Enabled: true}}}
	cache, err := NewModelsCache(context.Background(), loader)
	if err != nil {
		t.Fatalf("NewModelsCache: %v", err)
	}
	if loader.calls != 1 {
		t.Fatalf("expected exactly one load at construction, got %d", loader.calls)
	}

	caps, err := cache.ListAsCapabilitiesForOrg(context.Background(), orgA.String())
	if err != nil || len(caps) != 1 {
		t.Fatalf("expected 1 cached capability before reload, got %d (err=%v)", len(caps), err)
	}
	if loader.calls != 1 {
		t.Fatalf("read must not hit the loader; calls = %d", loader.calls)
	}

	loader.items = []*Model{
		{OrgID: uuid.Nil, Provider: "openai", Name: "gpt-4o", Enabled: true},
		{OrgID: uuid.Nil, Provider: "anthropic", Name: "claude-3-5-sonnet", Enabled: true},
	}
	if err := cache.Reload(context.Background()); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	if loader.calls != 2 {
		t.Fatalf("expected Reload to call the loader once more, got %d", loader.calls)
	}

	caps, err = cache.ListAsCapabilitiesForOrg(context.Background(), orgA.String())
	if err != nil || len(caps) != 2 {
		t.Fatalf("expected 2 cached capabilities after reload, got %d (err=%v)", len(caps), err)
	}
}
