package webhookorg

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

type fakeConnStore struct {
	connections []store.Connection
	findCalls   int
	updates     map[string]map[string]string
	updateErr   error
}

func (f *fakeConnStore) FindConnectionByWebhookAccount(_ context.Context, providerKeys []string, accountID string) (store.Connection, error) {
	f.findCalls++
	allowed := map[string]bool{}
	for _, key := range providerKeys {
		allowed[key] = true
	}
	for _, conn := range f.connections {
		if !allowed[conn.ProviderKey] || conn.DeletedAt != nil {
			continue
		}
		if conn.Status != "active" && conn.Status != "needs_refresh" {
			continue
		}
		if testMatches(conn, accountID) {
			return conn, nil
		}
	}
	return store.Connection{}, store.ErrNotFound
}

func (f *fakeConnStore) ListConnections(_ context.Context, filter store.ConnectionFilter) ([]store.Connection, error) {
	var out []store.Connection
	for _, conn := range f.connections {
		if filter.ProviderKey == "" || conn.ProviderKey == filter.ProviderKey {
			out = append(out, conn)
		}
	}
	return out, nil
}

func (f *fakeConnStore) UpdateConnectionProviderContext(_ context.Context, id string, providerContext map[string]string) (store.Connection, error) {
	if f.updateErr != nil {
		return store.Connection{}, f.updateErr
	}
	if f.updates == nil {
		f.updates = map[string]map[string]string{}
	}
	f.updates[id] = providerContext
	for i := range f.connections {
		if f.connections[i].ID == id {
			f.connections[i].ProviderContext = providerContext
			return f.connections[i], nil
		}
	}
	return store.Connection{}, store.ErrNotFound
}

type fakeAssets struct {
	ids            map[string][]string // connection id → asset ids
	calls          int
	subscribeCalls int
	err            error
	errs           []error
}

func (f *fakeAssets) SubscribeWebhookAccounts(_ context.Context, _ store.Connection, _ []string) error {
	f.subscribeCalls++
	return f.err
}

func (f *fakeAssets) ListWebhookAccountIDs(_ context.Context, conn store.Connection) ([]string, error) {
	f.calls++
	if len(f.errs) >= f.calls && f.errs[f.calls-1] != nil {
		return nil, f.errs[f.calls-1]
	}
	if f.err != nil {
		return nil, f.err
	}
	return f.ids[conn.ID], nil
}

func TestResolve_RetriesEnrichmentAfterTransientGraphFailure(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_retry", Status: "active",
		Capabilities: []string{"social.inbox.read"},
	}}}
	assets := &fakeAssets{
		ids:  map[string][]string{"conn_meta": {"page_retry"}},
		errs: []error{errors.New("graph temporarily unavailable"), nil},
	}
	r := &Resolver{Store: st, Meta: assets, NegativeTTL: time.Nanosecond}
	payload := mustPayload(t, `{"object":"page","entry":[{"id":"page_retry"}]}`)

	if _, ok := r.Resolve(context.Background(), "meta", payload); ok {
		t.Fatal("first resolve should fail while Graph is unavailable")
	}
	time.Sleep(time.Millisecond)
	resolution, ok := r.Resolve(context.Background(), "meta", payload)
	if !ok || resolution.OrganizationID != "org_retry" {
		t.Fatalf("second resolve = %+v, %v; want recovered org", resolution, ok)
	}
	if assets.calls != 2 {
		t.Fatalf("graph sweeps = %d, want retry after transient failure", assets.calls)
	}
}

func testMatches(conn store.Connection, accountID string) bool {
	if conn.ProviderAccountID == accountID || conn.TenantID == accountID {
		return true
	}
	for _, id := range strings.Split(conn.ProviderContext["webhook_account_ids"], ",") {
		if strings.TrimSpace(id) == accountID {
			return true
		}
	}
	return false
}

func mustPayload(t *testing.T, raw string) map[string]any {
	t.Helper()
	var payload map[string]any
	if err := json.Unmarshal([]byte(raw), &payload); err != nil {
		t.Fatalf("payload fixture: %v", err)
	}
	return payload
}

func TestResolve_SlackTeamIDMatchesTenantID(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_slack", ProviderKey: "slack", OrganizationID: "org_9",
		TenantID: "T0EXAMPLE", Status: "active",
	}}}
	r := &Resolver{Store: st}

	payload := mustPayload(t, `{"type":"event_callback","team_id":"T0EXAMPLE","event":{"type":"message"}}`)
	resolution, ok := r.Resolve(context.Background(), "slack", payload)
	if !ok {
		t.Fatal("expected slack team_id to resolve")
	}
	if resolution.OrganizationID != "org_9" || resolution.ConnectionID != "conn_slack" {
		t.Fatalf("resolution = %+v", resolution)
	}
}

func TestResolve_MetaEntryIDMatchesEnrichedContext(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_7", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "111,222333444,555"},
	}}}
	r := &Resolver{Store: st}

	payload := mustPayload(t, `{"object":"page","entry":[{"id":"222333444","messaging":[{}]}]}`)
	resolution, ok := r.Resolve(context.Background(), "meta", payload)
	if !ok {
		t.Fatal("expected page entry.id to resolve via enrichment")
	}
	if resolution.OrganizationID != "org_7" {
		t.Fatalf("resolution = %+v", resolution)
	}
}

func TestResolve_WhatsAppPhoneNumberID(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "whatsapp", OrganizationID: "org_wa", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "WABA_UNKNOWN,15550001111"},
	}}}
	r := &Resolver{Store: st}

	payload := mustPayload(t, `{
		"object": "whatsapp_business_account",
		"entry": [{"id": "WABA_UNKNOWN", "changes": [{"value": {"metadata": {"phone_number_id": "15550001111"}}}]}]
	}`)
	resolution, ok := r.Resolve(context.Background(), "whatsapp", payload)
	if !ok {
		t.Fatal("expected phone_number_id to resolve")
	}
	if resolution.OrganizationID != "org_wa" {
		t.Fatalf("resolution = %+v", resolution)
	}
}

func TestResolve_LazyEnrichmentSweepsAndPersists(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_enriched", Status: "active",
		ProviderAccountID: "fb-user-1",
		Capabilities:      []string{"social.inbox.read"},
	}}}
	assets := &fakeAssets{ids: map[string][]string{
		"conn_meta": {"page_1", "ig_17841400000000000", "waba_1", "phone_1"},
	}}
	r := &Resolver{Store: st, Meta: assets}

	payload := mustPayload(t, `{"object":"instagram","entry":[{"id":"ig_17841400000000000","messaging":[{}]}]}`)
	resolution, ok := r.Resolve(context.Background(), "instagram", payload)
	if !ok {
		t.Fatal("expected enrichment sweep to resolve the IG account")
	}
	if resolution.OrganizationID != "org_enriched" {
		t.Fatalf("resolution = %+v", resolution)
	}
	if assets.calls != 1 {
		t.Errorf("graph sweeps = %d, want 1", assets.calls)
	}
	persisted := st.updates["conn_meta"]["webhook_account_ids"]
	if persisted != "page_1,ig_17841400000000000,waba_1,phone_1" {
		t.Errorf("persisted enrichment = %q", persisted)
	}
	resolution, ok = r.Resolve(context.Background(), "instagram", payload)
	if !ok || resolution.OrganizationID != "org_enriched" {
		t.Fatalf("immediate second resolve = %+v, %v; want durable binding without stale negative cache", resolution, ok)
	}
}

func TestResolveMetaBatchFailsClosedAcrossOrganizations(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{
		{ID: "conn-a", ProviderKey: "meta", OrganizationID: "org-a", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-a"}},
		{ID: "conn-b", ProviderKey: "meta", OrganizationID: "org-b", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-b"}},
	}}
	r := &Resolver{Store: st}
	payload := mustPayload(t, `{"object":"page","entry":[{"id":"page-a"},{"id":"page-b"}]}`)
	if resolution, ok := r.Resolve(t.Context(), "meta", payload); ok {
		t.Fatalf("resolution = %+v, want cross-tenant batch rejected", resolution)
	}
}

func TestResolvePayloadsPartitionsMetaBatchAcrossOrganizations(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{
		{ID: "conn-a", ProviderKey: "meta", OrganizationID: "org-a", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-a"}},
		{ID: "conn-b", ProviderKey: "meta", OrganizationID: "org-b", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-b"}},
	}}
	r := &Resolver{Store: st}
	payload := mustPayload(t, `{"object":"page","entry":[{"id":"page-a"},{"id":"page-b"}]}`)
	resolved, ok := r.ResolvePayloads(t.Context(), "meta", payload)
	if !ok || len(resolved) != 2 {
		t.Fatalf("resolved=%+v ok=%v, want two tenant partitions", resolved, ok)
	}
	if resolved[0].Resolution.OrganizationID != "org-a" || len(resolved[0].Payload["entry"].([]any)) != 1 ||
		resolved[1].Resolution.OrganizationID != "org-b" || len(resolved[1].Payload["entry"].([]any)) != 1 {
		t.Fatalf("unexpected partitions: %+v", resolved)
	}
}

func TestProvisionConnectionPersistsWebhookAccountIDs(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_meta", Status: "active",
		Capabilities: []string{"social.inbox.read"},
	}}}
	assets := &fakeAssets{ids: map[string][]string{"conn_meta": {"page-1", "ig-1", "waba-1"}}}
	r := &Resolver{Store: st, Meta: assets}

	updated, err := r.ProvisionConnection(t.Context(), st.connections[0])
	if err != nil {
		t.Fatalf("ProvisionConnection error: %v", err)
	}
	if updated.ProviderContext["webhook_account_ids"] != "page-1,ig-1,waba-1" {
		t.Fatalf("provider context = %v", updated.ProviderContext)
	}
	if assets.subscribeCalls != 1 {
		t.Fatalf("subscription calls = %d, want 1", assets.subscribeCalls)
	}
}

func TestProvisionConnectionRejectsAssetOwnedByAnotherConnectionBeforeSubscribe(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{
		{ID: "conn-existing", ProviderKey: "meta", OrganizationID: "org-a", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-shared"}},
		{ID: "conn-new", ProviderKey: "meta", OrganizationID: "org-b", Status: "active", Capabilities: []string{"social.inbox.read"}},
	}}
	assets := &fakeAssets{ids: map[string][]string{"conn-new": {"page-shared"}}}
	r := &Resolver{Store: st, Meta: assets}
	if _, err := r.ProvisionConnection(t.Context(), st.connections[1]); err == nil {
		t.Fatal("ProvisionConnection error = nil, want duplicate ownership rejection")
	}
	if assets.subscribeCalls != 0 {
		t.Fatalf("subscription calls = %d, want none before ownership validation", assets.subscribeCalls)
	}
}

func TestResolve_EnrichmentSweepRunsOncePerConnection(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_x", Status: "active",
		Capabilities: []string{"social.inbox.read"},
	}}}
	assets := &fakeAssets{ids: map[string][]string{"conn_meta": {"other_page"}}}
	// Zero NegativeTTL default would cache the account miss and mask the
	// sweep-once behavior; disable caching effects with tiny TTLs is not
	// possible (defaults kick in), so use distinct account ids per call.
	r := &Resolver{Store: st, Meta: assets}

	first := mustPayload(t, `{"object":"page","entry":[{"id":"unknown_1"}]}`)
	if _, ok := r.Resolve(context.Background(), "meta", first); ok {
		t.Fatal("unknown_1 should not resolve")
	}
	second := mustPayload(t, `{"object":"page","entry":[{"id":"unknown_2"}]}`)
	if _, ok := r.Resolve(context.Background(), "meta", second); ok {
		t.Fatal("unknown_2 should not resolve")
	}
	if assets.calls != 1 {
		t.Errorf("graph sweeps = %d, want exactly 1 (sweep-once per process)", assets.calls)
	}
}

func TestResolve_RetriesEnrichmentAfterProviderContextWriteFailure(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "meta", OrganizationID: "org_retry", Status: "active",
		Capabilities: []string{"social.inbox.read"},
	}}, updateErr: errors.New("database temporarily unavailable")}
	assets := &fakeAssets{ids: map[string][]string{"conn_meta": {"page_retry"}}}
	r := &Resolver{Store: st, Meta: assets, NegativeTTL: time.Nanosecond}
	payload := mustPayload(t, `{"object":"page","entry":[{"id":"page_retry"}]}`)

	if _, ok := r.Resolve(t.Context(), "meta", payload); ok {
		t.Fatal("first resolve should fail while provider context cannot be persisted")
	}
	st.updateErr = nil
	time.Sleep(time.Millisecond)
	resolution, ok := r.Resolve(t.Context(), "meta", payload)
	if !ok || resolution.OrganizationID != "org_retry" {
		t.Fatalf("second resolve = %+v, %v; want recovered org", resolution, ok)
	}
	if assets.calls != 2 {
		t.Fatalf("graph sweeps = %d, want retry after persistence failure", assets.calls)
	}
}

func TestResolve_NegativeCacheAvoidsRepeatLookups(t *testing.T) {
	st := &fakeConnStore{}
	r := &Resolver{Store: st, NegativeTTL: time.Minute}

	payload := mustPayload(t, `{"type":"event_callback","team_id":"T_UNKNOWN"}`)
	for range 3 {
		if _, ok := r.Resolve(context.Background(), "slack", payload); ok {
			t.Fatal("unknown team should not resolve")
		}
	}
	if st.findCalls != 1 {
		t.Errorf("store lookups = %d, want 1 (negative cache)", st.findCalls)
	}
}

func TestResolve_UnknownProviderIsNoop(t *testing.T) {
	st := &fakeConnStore{}
	r := &Resolver{Store: st}
	if _, ok := r.Resolve(context.Background(), "github", mustPayload(t, `{"team_id":"x"}`)); ok {
		t.Fatal("github must not resolve (no extractor)")
	}
	if st.findCalls != 0 {
		t.Errorf("store lookups = %d, want 0", st.findCalls)
	}
}

func TestResolve_NumericEntryIDs(t *testing.T) {
	st := &fakeConnStore{connections: []store.Connection{{
		ID: "conn_meta", ProviderKey: "facebook", OrganizationID: "org_n", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "108500000000000"},
	}}}
	r := &Resolver{Store: st}
	// Meta docs show entry.id as a string, but defensively handle unquoted ids.
	payload := mustPayload(t, `{"object":"page","entry":[{"id":108500000000000}]}`)
	if _, ok := r.Resolve(context.Background(), "facebook", payload); !ok {
		t.Fatal("numeric entry.id should resolve")
	}
}

func TestResolve_TransientStoreErrorNotNegativeCached(t *testing.T) {
	boom := errors.New("db down")
	st := &erroringStore{err: boom}
	r := &Resolver{Store: st}

	payload := mustPayload(t, `{"type":"event_callback","team_id":"T1"}`)
	if _, ok := r.Resolve(context.Background(), "slack", payload); ok {
		t.Fatal("should not resolve during store outage")
	}
	if _, ok := r.Resolve(context.Background(), "slack", payload); ok {
		t.Fatal("should not resolve during store outage")
	}
	if st.calls != 2 {
		t.Errorf("store lookups = %d, want 2 (transient errors are retried, not cached)", st.calls)
	}
}

type erroringStore struct {
	err   error
	calls int
}

func (e *erroringStore) FindConnectionByWebhookAccount(context.Context, []string, string) (store.Connection, error) {
	e.calls++
	return store.Connection{}, e.err
}

func (e *erroringStore) ListConnections(context.Context, store.ConnectionFilter) ([]store.Connection, error) {
	return nil, e.err
}

func (e *erroringStore) UpdateConnectionProviderContext(context.Context, string, map[string]string) (store.Connection, error) {
	return store.Connection{}, e.err
}
