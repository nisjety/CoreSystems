package providerleads

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/integration"
)

const testInternalKey = "test-internal-key"

// fakeRepo is an in-memory Repository that enforces the same dedupe key the
// provider_leads UNIQUE constraint does: (org_id, provider_key, provider_lead_id).
type fakeRepo struct {
	mu    sync.Mutex
	store map[string]ProviderLead
	fail  bool
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{store: map[string]ProviderLead{}}
}

func (r *fakeRepo) UpsertLeads(_ context.Context, leads []ProviderLead) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.fail {
		return 0, fmt.Errorf("simulated persist failure")
	}
	upserted := 0
	for _, lead := range leads {
		if strings.TrimSpace(lead.OrgID) == "" || strings.TrimSpace(lead.ProviderLeadID) == "" {
			continue
		}
		key := lead.OrgID + "|" + lead.ProviderKey + "|" + lead.ProviderLeadID
		r.store[key] = lead
		upserted++
	}
	return upserted, nil
}

func (r *fakeRepo) DeleteByOrg(_ context.Context, orgID string) (int64, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	var deleted int64
	for key, lead := range r.store {
		if lead.OrgID == orgID {
			delete(r.store, key)
			deleted++
		}
	}
	return deleted, nil
}

func (r *fakeRepo) size() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.store)
}

func (r *fakeRepo) leads() []ProviderLead {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]ProviderLead, 0, len(r.store))
	for _, lead := range r.store {
		out = append(out, lead)
	}
	return out
}

type fakeAudit struct {
	mu     sync.Mutex
	events []SyncAudit
}

func (a *fakeAudit) PublishProviderLeadSync(_ context.Context, ev SyncAudit) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, ev)
}

func (a *fakeAudit) all() []SyncAudit {
	a.mu.Lock()
	defer a.mu.Unlock()
	return append([]SyncAudit(nil), a.events...)
}

// executeCall records one POST /api/v1/actions/execute the mock gateway saw.
type executeCall struct {
	ConnectionID string
	Operation    string
	Params       map[string]any
}

// mockGateway emulates integration-corev2: the {success,data} envelope, the
// X-Internal-API-Key gate, GET /api/v1/connections, and
// POST /api/v1/actions/execute whose result payloads are shaped from the
// LinkedIn executor's real pass-through handling (rest/leadForms and
// rest/leadFormResponses collection envelopes).
type mockGateway struct {
	server      *httptest.Server
	mu          sync.Mutex
	connections []integration.Connection
	executes    []executeCall
}

func newMockGateway(t *testing.T, connections []integration.Connection) *mockGateway {
	t.Helper()
	g := &mockGateway{connections: connections}
	mux := http.NewServeMux()

	writeEnvelope := func(w http.ResponseWriter, data any) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": data})
	}
	requireKey := func(w http.ResponseWriter, r *http.Request) bool {
		if r.Header.Get("X-Internal-API-Key") != testInternalKey {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"success": false,
				"error":   map[string]any{"code": "unauthorized", "message": "bad internal key"},
			})
			return false
		}
		return true
	}

	mux.HandleFunc("/api/v1/connections", func(w http.ResponseWriter, r *http.Request) {
		if !requireKey(w, r) {
			return
		}
		if got := r.URL.Query().Get("providerKey"); got != ProviderKeyLinkedIn {
			t.Errorf("connections listed with providerKey=%q, want %q", got, ProviderKeyLinkedIn)
		}
		orgFilter := r.URL.Query().Get("organizationId")
		g.mu.Lock()
		filtered := make([]integration.Connection, 0, len(g.connections))
		for _, conn := range g.connections {
			if orgFilter == "" || conn.OrganizationID == orgFilter {
				filtered = append(filtered, conn)
			}
		}
		g.mu.Unlock()
		writeEnvelope(w, map[string]any{"connections": filtered})
	})

	mux.HandleFunc("/api/v1/actions/execute", func(w http.ResponseWriter, r *http.Request) {
		if !requireKey(w, r) {
			return
		}
		var body struct {
			ConnectionID string         `json:"connectionId"`
			Operation    string         `json:"operation"`
			Params       map[string]any `json:"params"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Errorf("execute body decode: %v", err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		g.mu.Lock()
		g.executes = append(g.executes, executeCall{ConnectionID: body.ConnectionID, Operation: body.Operation, Params: body.Params})
		g.mu.Unlock()

		var result any
		switch body.Operation {
		case "linkedin.lead.forms":
			// Shape: GET {base}/rest/leadForms?q=owner (executor passes the
			// provider collection envelope through verbatim).
			result = map[string]any{
				"elements": []map[string]any{
					{"id": 901, "name": "Demo request"},
				},
				"paging": map[string]any{"count": 100, "start": 0, "total": 1},
			}
		case "linkedin.lead.responses":
			// Shape: GET {base}/rest/leadFormResponses?q=leadForm.
			result = map[string]any{
				"elements": []map[string]any{
					{
						"id":                   "urn:li:leadFormResponse:501",
						"submittedAt":          int64(1719999999000),
						"leadType":             "SPONSORED",
						"versionedLeadFormUrn": "urn:li:versionedLeadForm:(urn:li:leadForm:901,1)",
						"formResponse": map[string]any{
							"answers": []map[string]any{
								{"questionId": 1, "answer": map[string]any{"textQuestionAnswer": map[string]any{"answer": "Ola Nordmann"}}},
								{"questionId": 2, "answer": map[string]any{"textQuestionAnswer": map[string]any{"answer": "ola@example.no"}}},
							},
						},
					},
					{
						"id":          "urn:li:leadFormResponse:502",
						"submittedAt": int64(1720000010000),
						"formResponse": map[string]any{
							"answers": []map[string]any{
								{"questionId": 1, "answer": map[string]any{"textQuestionAnswer": map[string]any{"answer": "Kari Nordmann"}}},
							},
						},
					},
				},
				"paging": map[string]any{"count": 100, "start": 0, "total": 2},
			}
		default:
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"success": false,
				"error":   map[string]any{"code": "unsupported_operation", "message": body.Operation},
			})
			return
		}
		writeEnvelope(w, map[string]any{"action": map[string]any{
			"providerKey": ProviderKeyLinkedIn,
			"operation":   body.Operation,
			"result":      result,
		}})
	})

	g.server = httptest.NewServer(mux)
	t.Cleanup(g.server.Close)
	return g
}

func (g *mockGateway) client() *integration.Client {
	return integration.NewClient(integration.Config{
		BaseURL:        g.server.URL,
		InternalAPIKey: testInternalKey,
	})
}

func (g *mockGateway) executeCalls() []executeCall {
	g.mu.Lock()
	defer g.mu.Unlock()
	return append([]executeCall(nil), g.executes...)
}

func linkedinConnection() integration.Connection {
	return integration.Connection{
		ID:             "conn_li_1",
		ProviderKey:    ProviderKeyLinkedIn,
		OrganizationID: "org-acme",
		Status:         "connected",
		Capabilities:   []string{"social.profile.read", CapabilityLeadsRead},
		ProviderContext: map[string]string{
			"organizationUrn": "urn:li:organization:123",
		},
	}
}

func TestSyncPersistsLinkedInLeadFormResponses(t *testing.T) {
	gateway := newMockGateway(t, []integration.Connection{linkedinConnection()})
	repo := newFakeRepo()
	auditSink := &fakeAudit{}
	syncer := NewSyncer(gateway.client(), repo)
	syncer.SetAudit(auditSink)

	result, err := syncer.Sync(context.Background(), "", "")
	if err != nil {
		t.Fatalf("Sync: %v", err)
	}
	if result.Connections != 1 || result.Forms != 1 || result.LeadsFetched != 2 || result.LeadsUpserted != 2 {
		t.Fatalf("unexpected result: %+v", result)
	}
	if repo.size() != 2 {
		t.Fatalf("repo size = %d, want 2", repo.size())
	}

	for _, lead := range repo.leads() {
		if lead.OrgID != "org-acme" || lead.ConnectionID != "conn_li_1" || lead.ProviderKey != ProviderKeyLinkedIn {
			t.Fatalf("lead scoping wrong: %+v", lead)
		}
		if lead.FormID != "901" || lead.FormName != "Demo request" {
			t.Fatalf("form metadata wrong: %+v", lead)
		}
		if lead.SubmittedAt == nil {
			t.Fatalf("submittedAt not parsed: %+v", lead)
		}
		if !strings.Contains(string(lead.Fields), "textQuestionAnswer") {
			t.Fatalf("raw answers not preserved: %s", lead.Fields)
		}
	}

	// Verify the exact operations + params the actions gateway saw.
	calls := gateway.executeCalls()
	if len(calls) != 2 {
		t.Fatalf("execute calls = %d, want 2 (forms + responses)", len(calls))
	}
	if calls[0].Operation != "linkedin.lead.forms" {
		t.Fatalf("first operation = %q", calls[0].Operation)
	}
	if owner, _ := calls[0].Params["owner"].(string); owner != "urn:li:organization:123" {
		t.Fatalf("lead.forms owner = %v", calls[0].Params["owner"])
	}
	if calls[1].Operation != "linkedin.lead.responses" {
		t.Fatalf("second operation = %q", calls[1].Operation)
	}
	if leadForm, _ := calls[1].Params["leadForm"].(string); leadForm != "urn:li:leadForm:901" {
		t.Fatalf("lead.responses leadForm = %v", calls[1].Params["leadForm"])
	}

	events := auditSink.all()
	if len(events) != 1 {
		t.Fatalf("audit events = %d, want 1", len(events))
	}
	ev := events[0]
	if ev.OrgID != "org-acme" || ev.Outcome != "ok" || ev.LeadsUpserted != 2 || ev.Forms != 1 {
		t.Fatalf("audit event wrong: %+v", ev)
	}
}

func TestSyncIsIdempotentAcrossReruns(t *testing.T) {
	gateway := newMockGateway(t, []integration.Connection{linkedinConnection()})
	repo := newFakeRepo()
	syncer := NewSyncer(gateway.client(), repo)

	for run := 1; run <= 2; run++ {
		result, err := syncer.Sync(context.Background(), "", "")
		if err != nil {
			t.Fatalf("Sync run %d: %v", run, err)
		}
		if result.LeadsUpserted != 2 {
			t.Fatalf("run %d upserted = %d, want 2 (upsert refresh)", run, result.LeadsUpserted)
		}
	}
	// The dedupe key (org_id, provider_key, provider_lead_id) keeps re-synced
	// leads from duplicating.
	if repo.size() != 2 {
		t.Fatalf("repo size after two runs = %d, want 2", repo.size())
	}
}

func TestSyncHonestSkipWhenNoLinkedInConnections(t *testing.T) {
	gateway := newMockGateway(t, nil)
	repo := newFakeRepo()
	auditSink := &fakeAudit{}
	syncer := NewSyncer(gateway.client(), repo)
	syncer.SetAudit(auditSink)

	result, err := syncer.Sync(context.Background(), "", "")
	if err != nil {
		t.Fatalf("Sync: %v", err)
	}
	if result.Connections != 0 || result.LeadsFetched != 0 || result.LeadsUpserted != 0 {
		t.Fatalf("expected zero-count result, got %+v", result)
	}
	if len(result.Skipped) != 1 || !strings.Contains(result.Skipped[0], "no linkedin connections") {
		t.Fatalf("expected honest skip reason, got %v", result.Skipped)
	}
	if calls := gateway.executeCalls(); len(calls) != 0 {
		t.Fatalf("no actions should execute on skip, got %d", len(calls))
	}
	if repo.size() != 0 {
		t.Fatalf("nothing should persist on skip, repo size = %d", repo.size())
	}
}

func TestSyncSkipsConnectionWithoutLeadsCapability(t *testing.T) {
	conn := linkedinConnection()
	conn.Capabilities = []string{"social.profile.read", "social.post.write"}
	gateway := newMockGateway(t, []integration.Connection{conn})
	repo := newFakeRepo()
	auditSink := &fakeAudit{}
	syncer := NewSyncer(gateway.client(), repo)
	syncer.SetAudit(auditSink)

	result, err := syncer.Sync(context.Background(), "", "")
	if err != nil {
		t.Fatalf("Sync: %v", err)
	}
	if result.Connections != 0 || result.LeadsUpserted != 0 {
		t.Fatalf("expected zero-count result, got %+v", result)
	}
	if len(result.Skipped) != 1 || !strings.Contains(result.Skipped[0], CapabilityLeadsRead) {
		t.Fatalf("expected capability skip reason, got %v", result.Skipped)
	}
	if calls := gateway.executeCalls(); len(calls) != 0 {
		t.Fatalf("no actions should execute without the leads capability, got %d", len(calls))
	}
	events := auditSink.all()
	if len(events) != 1 || events[0].Outcome != "skipped" {
		t.Fatalf("expected one skipped audit event, got %+v", events)
	}
}

func TestSyncGatewayFailure_PublishesFailedAuditEvent(t *testing.T) {
	gateway := newMockGateway(t, nil)
	gateway.server.Close() // force a transport-level failure on ListConnections
	repo := newFakeRepo()
	auditSink := &fakeAudit{}
	syncer := NewSyncer(gateway.client(), repo)
	syncer.SetAudit(auditSink)

	_, err := syncer.Sync(context.Background(), "org-acme", "")
	if err == nil {
		t.Fatal("expected an error when the gateway is unreachable")
	}
	events := auditSink.all()
	if len(events) != 1 {
		t.Fatalf("expected one audit event for the whole-run failure, got %+v", events)
	}
	if events[0].Outcome != "failed" {
		t.Fatalf("outcome = %q, want failed", events[0].Outcome)
	}
	if events[0].OrgID != "org-acme" {
		t.Errorf("org_id = %q, want org-acme (the filter passed in)", events[0].OrgID)
	}
	if len(events[0].Skipped) != 1 || !strings.Contains(events[0].Skipped[0], "list linkedin connections") {
		t.Errorf("expected the gateway error recorded in Skipped, got %+v", events[0].Skipped)
	}
}

func TestSyncSkipsConnectionWithoutResolvableOwner(t *testing.T) {
	conn := linkedinConnection()
	conn.ProviderContext = nil
	conn.ProviderAccountID = "AbC123xyz" // member sub, not a numeric org id
	gateway := newMockGateway(t, []integration.Connection{conn})
	repo := newFakeRepo()
	syncer := NewSyncer(gateway.client(), repo)

	result, err := syncer.Sync(context.Background(), "", "")
	if err != nil {
		t.Fatalf("Sync: %v", err)
	}
	if len(result.Skipped) != 1 || !strings.Contains(result.Skipped[0], "owner urn") {
		t.Fatalf("expected owner skip reason, got %v", result.Skipped)
	}
	if calls := gateway.executeCalls(); len(calls) != 0 {
		t.Fatalf("no actions should execute without an owner, got %d", len(calls))
	}
}

func TestSyncOwnerOverrideWins(t *testing.T) {
	conn := linkedinConnection()
	conn.ProviderContext = nil
	gateway := newMockGateway(t, []integration.Connection{conn})
	repo := newFakeRepo()
	syncer := NewSyncer(gateway.client(), repo)

	result, err := syncer.Sync(context.Background(), "org-acme", "urn:li:sponsoredAccount:777")
	if err != nil {
		t.Fatalf("Sync: %v", err)
	}
	if result.LeadsUpserted != 2 {
		t.Fatalf("upserted = %d, want 2", result.LeadsUpserted)
	}
	calls := gateway.executeCalls()
	if len(calls) == 0 {
		t.Fatal("expected execute calls")
	}
	if owner, _ := calls[0].Params["owner"].(string); owner != "urn:li:sponsoredAccount:777" {
		t.Fatalf("owner override not applied: %v", calls[0].Params["owner"])
	}
}

func TestResolveOwner(t *testing.T) {
	tests := []struct {
		name     string
		conn     integration.Connection
		override string
		want     string
	}{
		{
			name:     "override wins",
			conn:     integration.Connection{ProviderContext: map[string]string{"organizationUrn": "urn:li:organization:1"}},
			override: "urn:li:organization:2",
			want:     "urn:li:organization:2",
		},
		{
			name: "organizationUrn from context",
			conn: integration.Connection{ProviderContext: map[string]string{"organizationUrn": "urn:li:organization:123"}},
			want: "urn:li:organization:123",
		},
		{
			name: "numeric context value normalized",
			conn: integration.Connection{ProviderContext: map[string]string{"organization": "456"}},
			want: "urn:li:organization:456",
		},
		{
			name: "numeric provider account id",
			conn: integration.Connection{ProviderAccountID: "789"},
			want: "urn:li:organization:789",
		},
		{
			name: "unresolvable",
			conn: integration.Connection{ProviderAccountID: "member-sub-abc"},
			want: "",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := resolveOwner(tt.conn, tt.override); got != tt.want {
				t.Fatalf("resolveOwner = %q, want %q", got, tt.want)
			}
		})
	}
}
