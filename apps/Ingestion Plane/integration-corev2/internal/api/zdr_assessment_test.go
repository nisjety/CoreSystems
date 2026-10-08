package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sort"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// The digest of the ChatGPT subscription assessment at catalog_version 1, as the contract's
// own vector and the JavaScript reference serialiser give it.
const subscriptionDigestCatalog1 = "7d865ca0c6296627837c0603192debcd2c217bcb17b884738fe99befdb810fbd"

func drainAudit(t *testing.T, repo store.Repository) []store.AuditEvent {
	t.Helper()
	var events []store.AuditEvent
	for {
		event, found, err := repo.ClaimAuditEvent(context.Background())
		if err != nil {
			t.Fatalf("claim audit event: %v", err)
		}
		if !found {
			return events
		}
		events = append(events, event)
	}
}

func auditOfType(events []store.AuditEvent, eventType string) []store.AuditEvent {
	var out []store.AuditEvent
	for _, event := range events {
		if event.EventType == eventType {
			out = append(out, event)
		}
	}
	return out
}

// zdrLoginRunner is a Codex runner whose device sign-in completes when the test says so.
type zdrLoginRunner struct {
	apiCodexRunner
	process *zdrLoginProcess
	manager *codexsubscription.Manager
}

type zdrLoginProcess struct{ status string }

func (p *zdrLoginProcess) Poll(context.Context) (codexsubscription.ProcessLoginStatus, error) {
	return codexsubscription.ProcessLoginStatus{Status: p.status}, nil
}

func (p *zdrLoginProcess) Close() error { return nil }

func (r *zdrLoginRunner) BeginDeviceLogin(context.Context, string) (codexsubscription.LoginProcess, codexsubscription.DeviceCode, error) {
	return r.process, codexsubscription.DeviceCode{VerificationURL: "https://auth.example.test/device", UserCode: "ABCD-1234"}, nil
}

func newZDRLoginServer(t *testing.T, repo store.Repository) (*zdrLoginRunner, func(method, path, body string) (int, string)) {
	t.Helper()
	cfg, memory, service := testOAuthStack(t)
	runner := &zdrLoginRunner{process: &zdrLoginProcess{status: "pending"}}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	runner.manager = manager
	if repo == nil {
		repo = memory
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager})
	call := func(method, path, body string) (int, string) {
		req := httptest.NewRequest(method, path, strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", "dev-key")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test %s %s: %v", method, path, err)
		}
		defer resp.Body.Close()
		raw, _ := io.ReadAll(resp.Body)
		return resp.StatusCode, string(raw)
	}
	return runner, call
}

type connectResponse struct {
	Data struct {
		Connection store.Connection `json:"connection"`
		Login      struct {
			LoginID string `json:"loginId"`
		} `json:"login"`
	} `json:"data"`
}

func startSubscription(t *testing.T, call func(method, path, body string) (int, string)) connectResponse {
	t.Helper()
	status, body := call(http.MethodPost, "/api/v1/model-subscriptions/openai-codex/connect", `{"organizationId":"org-1","userId":"user-1","userEmail":"u@example.test"}`)
	if status != http.StatusAccepted {
		t.Fatalf("connect status = %d, body %s", status, body)
	}
	var decoded connectResponse
	if err := json.Unmarshal([]byte(body), &decoded); err != nil || decoded.Data.Connection.ID == "" || decoded.Data.Login.LoginID == "" {
		t.Fatalf("connect response = %s (err %v)", body, err)
	}
	return decoded
}

func TestStartingASubscriptionRecordsItsZDRCapabilityAndAuditsItInTheSameTransaction(t *testing.T) {
	_, repo, _ := testOAuthStack(t)
	runner, call := newZDRLoginServer(t, repo)
	_ = runner

	started := startSubscription(t, call)
	record, err := repo.GetConnectionZDR(t.Context(), started.Data.Connection.ID)
	if err != nil {
		t.Fatalf("a started subscription has no ZDR record: %v", err)
	}
	if record.Capability != zdr.CapabilityNotSupported || record.Basis != zdr.BasisProviderTermsKnown || record.BasisRef != "openai-codex-subscription@1" ||
		record.ProviderKey != codexsubscription.ProviderKey || record.AuthMode != "codex_chatgpt_device_code" || record.CatalogVersion != 1 || record.Digest != subscriptionDigestCatalog1 {
		t.Fatalf("record = %+v", record)
	}

	events := drainAudit(t, repo)
	assessed := auditOfType(events, "subscription_connection.zdr_assessed")
	if len(assessed) != 1 {
		t.Fatalf("zdr_assessed events = %d, want exactly 1 (events: %+v)", len(assessed), events)
	}
	event := assessed[0]
	if event.ConnectionID != started.Data.Connection.ID || event.OrganizationID != "org-1" || event.UserID != "user-1" || event.ProviderKey != codexsubscription.ProviderKey {
		t.Fatalf("audit event identity = %+v", event)
	}
	wantKeys := []string{"basis", "basisRef", "capability", "catalogVersion", "change", "digest", "revokedAcknowledgements", "waiverRoute"}
	var gotKeys []string
	for key := range event.Metadata {
		gotKeys = append(gotKeys, key)
	}
	sort.Strings(gotKeys)
	if strings.Join(gotKeys, ",") != strings.Join(wantKeys, ",") {
		t.Fatalf("audit metadata keys = %v, want exactly the content-free set %v", gotKeys, wantKeys)
	}
	if event.Metadata["change"] != "created" || event.Metadata["capability"] != "not_supported" || event.Metadata["digest"] != subscriptionDigestCatalog1 || event.Metadata["waiverRoute"] != true {
		t.Fatalf("audit metadata = %+v", event.Metadata)
	}
	if len(auditOfType(events, "subscription_connection.login_started")) != 1 {
		t.Fatalf("login_started event missing: %+v", events)
	}
}

func TestBecomingConnectedAssessesAgainWithoutRewritingAnUnchangedRecord(t *testing.T) {
	_, repo, _ := testOAuthStack(t)
	runner, call := newZDRLoginServer(t, repo)
	started := startSubscription(t, call)
	before, err := repo.GetConnectionZDR(t.Context(), started.Data.Connection.ID)
	if err != nil {
		t.Fatalf("record after start: %v", err)
	}
	drainAudit(t, repo)

	runner.process.status = "connected"
	status, body := call(http.MethodGet, "/api/v1/model-subscriptions/openai-codex/connect/"+started.Data.Connection.ID+"/"+started.Data.Login.LoginID, "")
	if status != http.StatusOK {
		t.Fatalf("poll status = %d, body %s", status, body)
	}
	connection, err := repo.GetConnection(t.Context(), started.Data.Connection.ID)
	if err != nil || connection.Status != "active" {
		t.Fatalf("connection after connected = (%+v, %v)", connection, err)
	}
	after, err := repo.GetConnectionZDR(t.Context(), started.Data.Connection.ID)
	if err != nil || after != before {
		t.Fatalf("an unchanged assessment at connected rewrote the record: before %+v after %+v (%v)", before, after, err)
	}
	if assessed := auditOfType(drainAudit(t, repo), "subscription_connection.zdr_assessed"); len(assessed) != 0 {
		t.Fatalf("an unchanged assessment at connected wrote %d zdr_assessed audit rows, want none", len(assessed))
	}
}

func TestBecomingConnectedRecordsTheAssessmentOfAConnectionThatHadNone(t *testing.T) {
	// One of the live pending connections: it predates the feature, so it has no record, and it
	// finishes its sign-in before the start-up reconcile has seen it.
	_, repo, _ := testOAuthStack(t)
	runner, call := newZDRLoginServer(t, repo)
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-predates", ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: "org-1", UserID: "user-1", Status: "pending", Capabilities: []string{codexsubscription.Capability},
	}); err != nil {
		t.Fatalf("seed: %v", err)
	}
	login, err := runner.manager.StartLogin(t.Context(), "conn-predates")
	if err != nil {
		t.Fatalf("StartLogin: %v", err)
	}
	if _, err := repo.GetConnectionZDR(t.Context(), "conn-predates"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("the pre-feature connection already has a record: %v", err)
	}

	runner.process.status = "connected"
	status, body := call(http.MethodGet, "/api/v1/model-subscriptions/openai-codex/connect/conn-predates/"+login.LoginID, "")
	if status != http.StatusOK {
		t.Fatalf("poll: %d %s", status, body)
	}
	record, err := repo.GetConnectionZDR(t.Context(), "conn-predates")
	if err != nil {
		t.Fatalf("the connected connection has no record: %v", err)
	}
	if record.Capability != zdr.CapabilityNotSupported || record.Digest != subscriptionDigestCatalog1 {
		t.Fatalf("record = %+v", record)
	}
	if assessed := auditOfType(drainAudit(t, repo), "subscription_connection.zdr_assessed"); len(assessed) != 1 || assessed[0].Metadata["change"] != "created" {
		t.Fatalf("zdr_assessed events = %+v, want one created", assessed)
	}
}

func TestAStartThatCannotRecordItsAssessmentCreatesNoConnectionAndStartsNoLogin(t *testing.T) {
	_, memory, _ := testOAuthStack(t)
	repo := zdrFailingRepository{Repository: memory, failApply: true}
	runner, call := newZDRLoginServer(t, repo)
	runner.process.status = "pending"
	status, body := call(http.MethodPost, "/api/v1/model-subscriptions/openai-codex/connect", `{"organizationId":"org-1","userId":"user-1"}`)
	if status != http.StatusServiceUnavailable || !strings.Contains(body, "connection_persist_failed") {
		t.Fatalf("status = %d, body %s, want 503 connection_persist_failed", status, body)
	}
	connections, err := memory.ListConnections(t.Context(), store.ConnectionFilter{OrganizationID: "org-1"})
	if err != nil || len(connections) != 0 {
		t.Fatalf("a connection survived its failed assessment: %+v (%v)", connections, err)
	}
	if events := drainAudit(t, memory); len(events) != 0 {
		t.Fatalf("a failed start left audit rows behind: %+v", events)
	}
}

func TestAStartWhoseAuditRowCannotPersistLeavesNoRecordEither(t *testing.T) {
	_, memory, _ := testOAuthStack(t)
	repo := zdrFailingRepository{Repository: memory, failAssessedAudit: true}
	_, call := newZDRLoginServer(t, repo)
	status, body := call(http.MethodPost, "/api/v1/model-subscriptions/openai-codex/connect", `{"organizationId":"org-1","userId":"user-1"}`)
	if status != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, body %s, want 503", status, body)
	}
	connections, _ := memory.ListConnections(t.Context(), store.ConnectionFilter{OrganizationID: "org-1"})
	if len(connections) != 0 {
		t.Fatalf("connections = %+v, want none", connections)
	}
}

// zdrFailingRepository fails the ZDR step of every audit transaction, or only the audit row
// the assessment writes, so the atomicity of the assessment can be shown from the route.
type zdrFailingRepository struct {
	store.Repository
	failApply         bool
	failAssessedAudit bool
	failApplyFor      string
}

type zdrFailingTransaction struct {
	store.AuditTransaction
	failApply         bool
	failAssessedAudit bool
	failApplyFor      string
}

func (tx zdrFailingTransaction) ApplyConnectionZDRAssessment(ctx context.Context, assessment store.ConnectionZDRAssessment) (store.ConnectionZDRApplyResult, error) {
	if tx.failApply || (tx.failApplyFor != "" && assessment.ConnectionID == tx.failApplyFor) {
		return store.ConnectionZDRApplyResult{}, errors.New("zdr store unavailable")
	}
	return tx.AuditTransaction.ApplyConnectionZDRAssessment(ctx, assessment)
}

func (tx zdrFailingTransaction) InsertAuditEvent(ctx context.Context, event store.AuditEvent) error {
	if tx.failAssessedAudit && event.EventType == auditEventZDRAssessed {
		return errors.New("audit insert unavailable")
	}
	return tx.AuditTransaction.InsertAuditEvent(ctx, event)
}

func (r zdrFailingRepository) WithAuditTransaction(ctx context.Context, fn func(store.AuditTransaction) error) error {
	return r.Repository.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		return fn(zdrFailingTransaction{AuditTransaction: tx, failApply: r.failApply, failAssessedAudit: r.failAssessedAudit, failApplyFor: r.failApplyFor})
	})
}

// ---- generic routes cannot write the record --------------------------------------------

func TestNoGenericConnectionRouteCanWriteOrReadTheZDRRecord(t *testing.T) {
	_, repo, _ := testOAuthStack(t)
	_, call := newZDRLoginServer(t, repo)
	started := startSubscription(t, call)
	id := started.Data.Connection.ID
	before, err := repo.GetConnectionZDR(t.Context(), id)
	if err != nil {
		t.Fatalf("record: %v", err)
	}

	// Every field a client could use to try to set a capability, on the generic capability patch.
	hostile := `{"capabilities":["model.inference"],"zdrCapability":"supported","zdr_capability":"supported","capability":"supported","zdrDigest":"` +
		strings.Repeat("0", 64) + `","digest":"` + strings.Repeat("0", 64) + `","basis":"customer_asserted","zdr":{"capability":"supported"}}`
	status, body := call(http.MethodPatch, "/api/v1/connections/"+id+"/capabilities", hostile)
	if status != http.StatusOK {
		t.Fatalf("capability patch status = %d, body %s", status, body)
	}
	// And on the Codex connect route itself.
	status, body = call(http.MethodPost, "/api/v1/model-subscriptions/openai-codex/connect",
		`{"organizationId":"org-1","userId":"user-1","zdrCapability":"supported","providerContext":{"zdr_capability":"supported","auth_mode":"api_key"}}`)
	if status != http.StatusAccepted {
		t.Fatalf("connect with hostile fields status = %d, body %s", status, body)
	}
	var second connectResponse
	if err := json.Unmarshal([]byte(body), &second); err != nil {
		t.Fatal(err)
	}

	after, err := repo.GetConnectionZDR(t.Context(), id)
	if err != nil || after != before {
		t.Fatalf("the generic capability patch moved the record: before %+v after %+v (%v)", before, after, err)
	}
	other, err := repo.GetConnectionZDR(t.Context(), second.Data.Connection.ID)
	if err != nil || other.Capability != zdr.CapabilityNotSupported || other.Digest != subscriptionDigestCatalog1 {
		t.Fatalf("a client-supplied field changed the assessment of a new connection: %+v (%v)", other, err)
	}

	// The generic read paths do not carry the record either: it is read through its own routes
	// (Z-13), never as a field of a connection.
	for _, path := range []string{"/api/v1/connections/" + id, "/api/v1/connections?organizationId=org-1", "/api/v1/connections/" + id + "/capabilities"} {
		status, body := call(http.MethodGet, path, "")
		if status != http.StatusOK {
			t.Fatalf("GET %s = %d %s", path, status, body)
		}
		if strings.Contains(strings.ToLower(body), "zdr") {
			t.Errorf("GET %s exposes a ZDR field: %s", path, body)
		}
	}
}

// ---- the assessment's inputs are server-side facts ---------------------------------------

func TestTheAssessmentIgnoresWhatTheConnectionsOwnProviderContextClaims(t *testing.T) {
	repo := store.NewMemoryRepository()
	connection, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-claims", ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
		ProviderContext: map[string]string{"auth_mode": "api_key", "zdr_capability": "supported", "zdr_digest": strings.Repeat("a", 64)},
	})
	if err != nil {
		t.Fatal(err)
	}
	var applied store.ConnectionZDRApplyResult
	var ok bool
	if err := repo.WithAuditTransaction(t.Context(), func(tx store.AuditTransaction) error {
		var inner error
		applied, ok, inner = applyConnectionZDR(t.Context(), tx, func(event store.AuditEvent) error { return tx.InsertAuditEvent(t.Context(), event) }, connection)
		return inner
	}); err != nil || !ok {
		t.Fatalf("applyConnectionZDR = (applied %v, %v)", ok, err)
	}
	if applied.Record.Capability != zdr.CapabilityNotSupported || applied.Record.AuthMode != "codex_chatgpt_device_code" || applied.Record.Digest != subscriptionDigestCatalog1 {
		t.Fatalf("record = %+v: the connection's own provider_context steered its assessment", applied.Record)
	}
}

func TestOnlyAnExactModelProviderConnectionIsAssessed(t *testing.T) {
	repo := store.NewMemoryRepository()
	cases := map[string]store.Connection{
		"microsoft":              {ID: "c-ms", ProviderKey: "microsoft", ConnectorType: "microsoft", OrganizationID: "org-1", UserID: "u", Status: "active"},
		"slack":                  {ID: "c-sl", ProviderKey: "slack", ConnectorType: "slack", OrganizationID: "org-1", UserID: "u", Status: "active"},
		"alias of the model key": {ID: "c-alias", ProviderKey: "codex-subscription", ConnectorType: codexsubscription.ConnectorType, OrganizationID: "org-1", UserID: "u", Status: "active"},
		"model key, other type":  {ID: "c-type", ProviderKey: codexsubscription.ProviderKey, ConnectorType: "something-else", OrganizationID: "org-1", UserID: "u", Status: "active"},
		"upper-cased model key":  {ID: "c-case", ProviderKey: "OpenAI-Codex-Subscription", ConnectorType: codexsubscription.ConnectorType, OrganizationID: "org-1", UserID: "u", Status: "active"},
	}
	for name, connection := range cases {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("%s: seed: %v", name, err)
		}
		var ok bool
		if err := repo.WithAuditTransaction(t.Context(), func(tx store.AuditTransaction) error {
			var inner error
			_, ok, inner = applyConnectionZDR(t.Context(), tx, func(event store.AuditEvent) error { return tx.InsertAuditEvent(t.Context(), event) }, connection)
			return inner
		}); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if ok {
			t.Errorf("%s was assessed as a model connection", name)
		}
		if _, err := repo.GetConnectionZDR(t.Context(), connection.ID); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("%s has a ZDR record: %v", name, err)
		}
	}
}

// ---- the start-up reconcile (the backfill) ---------------------------------------------

// seedLiveShapedConnections reproduces the live integration database on 2026-10-06: seven
// ChatGPT subscription rows (2 active, 2 pending, 1 needs_refresh, 2 deleted) across two
// organisations, plus connections of other providers.
func seedLiveShapedConnections(t *testing.T, repo store.Repository) (live []string, deleted []string, others []string) {
	t.Helper()
	type row struct {
		id, org, user, status string
		deleted               bool
	}
	subscription := []row{
		{"live-active-1", "org-a", "user-1", "active", false},
		{"live-active-2", "org-a", "user-2", "active", false},
		{"live-pending-1", "org-a", "user-3", "pending", false},
		{"live-pending-2", "org-b", "user-4", "pending", false},
		{"live-needs-refresh", "org-b", "user-5", "needs_refresh", false},
		{"live-deleted-1", "org-a", "user-6", "deleted", true},
		{"live-deleted-2", "org-b", "user-7", "deleted", true},
	}
	for _, r := range subscription {
		if _, err := repo.UpsertConnection(t.Context(), store.Connection{
			ID: r.id, ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
			OrganizationID: r.org, UserID: r.user, Status: r.status, Capabilities: []string{codexsubscription.Capability},
			ProviderContext: map[string]string{"auth_mode": "chatgpt_device_code"},
		}); err != nil {
			t.Fatalf("seed %s: %v", r.id, err)
		}
		if r.deleted {
			if _, err := repo.MarkConnectionDeleted(t.Context(), r.id); err != nil {
				t.Fatalf("delete %s: %v", r.id, err)
			}
			deleted = append(deleted, r.id)
		} else {
			live = append(live, r.id)
		}
	}
	for _, c := range []store.Connection{
		{ID: "other-ms", ProviderKey: "microsoft", ConnectorType: "microsoft", OrganizationID: "org-a", UserID: "user-1", Status: "active"},
		{ID: "other-slack", ProviderKey: "slack", ConnectorType: "slack", OrganizationID: "org-b", UserID: "user-4", Status: "active"},
	} {
		if _, err := repo.UpsertConnection(t.Context(), c); err != nil {
			t.Fatalf("seed %s: %v", c.ID, err)
		}
		others = append(others, c.ID)
	}
	return live, deleted, others
}

func TestReconcileBackfillsTheLiveConnectionsAndOnlyThose(t *testing.T) {
	repo := store.NewMemoryRepository()
	live, deleted, others := seedLiveShapedConnections(t, repo)
	cfg := ServerConfig{Repo: repo}

	result, err := ReconcileConnectionZDR(t.Context(), cfg)
	if err != nil {
		t.Fatalf("ReconcileConnectionZDR: %v", err)
	}
	if result != (ZDRReconcileResult{Examined: 5, Created: 5}) {
		t.Fatalf("first pass = %+v, want 5 examined and 5 created", result)
	}
	for _, id := range live {
		record, err := repo.GetConnectionZDR(t.Context(), id)
		if err != nil {
			t.Fatalf("%s has no record: %v", id, err)
		}
		if record.Capability != zdr.CapabilityNotSupported || record.Basis != zdr.BasisProviderTermsKnown || record.Digest != subscriptionDigestCatalog1 || record.CatalogVersion != 1 {
			t.Errorf("%s record = %+v", id, record)
		}
		if acks, _ := repo.ListConnectionZDRAcknowledgements(t.Context(), id, false); len(acks) != 0 {
			t.Errorf("%s was given acknowledgements by the backfill: %+v", id, acks)
		}
	}
	for _, id := range append(append([]string{}, deleted...), others...) {
		if _, err := repo.GetConnectionZDR(t.Context(), id); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("%s must not be assessed (deleted or not a model connection): %v", id, err)
		}
	}
	events := auditOfType(drainAudit(t, repo), "subscription_connection.zdr_assessed")
	if len(events) != 5 {
		t.Fatalf("the backfill wrote %d zdr_assessed audit rows, want 5", len(events))
	}
	for _, event := range events {
		if event.Metadata["change"] != "created" || event.Metadata["digest"] != subscriptionDigestCatalog1 {
			t.Errorf("audit row = %+v", event)
		}
	}
}

func TestReconcileIsIdempotent(t *testing.T) {
	repo := store.NewMemoryRepository()
	live, _, _ := seedLiveShapedConnections(t, repo)
	cfg := ServerConfig{Repo: repo}
	if _, err := ReconcileConnectionZDR(t.Context(), cfg); err != nil {
		t.Fatalf("first pass: %v", err)
	}
	drainAudit(t, repo)
	before := map[string]store.ConnectionZDR{}
	for _, id := range live {
		before[id], _ = repo.GetConnectionZDR(t.Context(), id)
	}

	for pass := 2; pass <= 3; pass++ {
		result, err := ReconcileConnectionZDR(t.Context(), cfg)
		if err != nil {
			t.Fatalf("pass %d: %v", pass, err)
		}
		if result != (ZDRReconcileResult{Examined: 5, Unchanged: 5}) {
			t.Fatalf("pass %d = %+v, want 5 unchanged and nothing else", pass, result)
		}
		if events := drainAudit(t, repo); len(events) != 0 {
			t.Fatalf("pass %d wrote audit rows: %+v", pass, events)
		}
		for _, id := range live {
			after, _ := repo.GetConnectionZDR(t.Context(), id)
			if after != before[id] {
				t.Fatalf("pass %d rewrote the record of %s: %+v -> %+v", pass, id, before[id], after)
			}
		}
	}
}

func TestReconcileAfterACatalogChangeRevokesDependentAcknowledgementsAndAuditsEach(t *testing.T) {
	repo := store.NewMemoryRepository()
	live, _, _ := seedLiveShapedConnections(t, repo)
	// The previous catalog assessed the subscription as cannot_verify; a user acknowledged that.
	prevDigest, err := zdr.Digest(codexsubscription.ProviderKey, "codex_chatgpt_device_code", zdr.CapabilityCannotVerify, zdr.BasisNotAssessed, 1)
	if err != nil {
		t.Fatal(err)
	}
	target := live[0]
	if _, err := repo.ApplyConnectionZDRAssessment(t.Context(), store.ConnectionZDRAssessment{
		ConnectionID: target, ProviderKey: codexsubscription.ProviderKey, AuthMode: "codex_chatgpt_device_code",
		Capability: zdr.CapabilityCannotVerify, Basis: zdr.BasisNotAssessed, BasisRef: "fallback@1", CatalogVersion: 1, Digest: prevDigest,
	}); err != nil {
		t.Fatalf("seed previous assessment: %v", err)
	}
	text := "Vi kan ikke se avtalen din med leverandoeren."
	sum := sha256.Sum256([]byte(text))
	ack, created, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), store.NewConnectionZDRAcknowledgement{
		OrganizationID: "org-a", ConnectionID: target, UserID: "user-1", Digest: prevDigest, TextVersion: "2026-10-a", Locale: "no",
		TextSHA256: hex.EncodeToString(sum[:]), TextSnapshot: text, OrgZDRAtAck: true, AcknowledgedVia: store.ZDRAcknowledgedViaConnect,
	})
	if err != nil || !created {
		t.Fatalf("seed acknowledgement: %v", err)
	}
	drainAudit(t, repo)

	result, err := ReconcileConnectionZDR(t.Context(), ServerConfig{Repo: repo})
	if err != nil {
		t.Fatalf("reconcile: %v", err)
	}
	if result != (ZDRReconcileResult{Examined: 5, Created: 4, Changed: 1, Revoked: 1}) {
		t.Fatalf("result = %+v, want 4 created, 1 changed with its acknowledgement revoked", result)
	}
	record, _ := repo.GetConnectionZDR(t.Context(), target)
	if record.Capability != zdr.CapabilityNotSupported || record.Digest != subscriptionDigestCatalog1 {
		t.Fatalf("record after the change = %+v", record)
	}
	rows, _ := repo.ListConnectionZDRAcknowledgements(t.Context(), target, false)
	if len(rows) != 1 || rows[0].ID != ack.ID || rows[0].RevokedAt == nil || rows[0].RevokeReason != store.ZDRRevokeCapabilityChanged {
		t.Fatalf("ledger = %+v, want the acknowledgement revoked with reason capability_changed", rows)
	}

	events := drainAudit(t, repo)
	invalidated := auditOfType(events, "subscription_connection.zdr_acknowledgement_invalidated")
	if len(invalidated) != 1 {
		t.Fatalf("invalidated events = %+v, want exactly 1", invalidated)
	}
	if got := invalidated[0]; got.ConnectionID != target || got.UserID != "user-1" || got.OrganizationID != "org-a" ||
		got.Metadata["acknowledgementId"] != ack.ID || got.Metadata["reason"] != "capability_changed" || got.Metadata["digest"] != prevDigest || got.Metadata["currentDigest"] != subscriptionDigestCatalog1 {
		t.Fatalf("invalidated event = %+v", got)
	}
	var changed []store.AuditEvent
	for _, event := range auditOfType(events, "subscription_connection.zdr_assessed") {
		if event.Metadata["change"] == "changed" {
			changed = append(changed, event)
		}
	}
	if len(changed) != 1 || changed[0].ConnectionID != target || changed[0].Metadata["previousCapability"] != "cannot_verify" || changed[0].Metadata["previousDigest"] != prevDigest || changed[0].Metadata["revokedAcknowledgements"] != 1 {
		t.Fatalf("changed events = %+v", changed)
	}
}

func TestReconcileKeepsGoingPastAConnectionItCannotAssess(t *testing.T) {
	memory := store.NewMemoryRepository()
	live, _, _ := seedLiveShapedConnections(t, memory)
	failing := live[1]
	repo := zdrFailingRepository{Repository: memory, failApplyFor: failing}

	result, err := ReconcileConnectionZDR(t.Context(), ServerConfig{Repo: repo})
	if err == nil || !strings.Contains(err.Error(), failing) {
		t.Fatalf("error = %v, want it to name the connection that failed", err)
	}
	if result != (ZDRReconcileResult{Examined: 5, Created: 4, Failed: 1}) {
		t.Fatalf("result = %+v, want the other four assessed", result)
	}
	if _, getErr := memory.GetConnectionZDR(t.Context(), failing); !errors.Is(getErr, store.ErrNotFound) {
		t.Fatalf("the failed connection has a record: %v", getErr)
	}
	for _, id := range live {
		if id == failing {
			continue
		}
		if _, getErr := memory.GetConnectionZDR(t.Context(), id); getErr != nil {
			t.Errorf("%s was not assessed because another connection failed: %v", id, getErr)
		}
	}
}

func TestReconcileNeedsARepository(t *testing.T) {
	if _, err := ReconcileConnectionZDR(t.Context(), ServerConfig{}); err == nil {
		t.Fatal("ReconcileConnectionZDR without a repository did not fail")
	}
}

func TestOnlyModelProvidersAreInTheModelConnectionCatalogView(t *testing.T) {
	if authMode, ok := modelConnectionAuthMode(store.Connection{ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType}); !ok || authMode != "codex_chatgpt_device_code" {
		t.Fatalf("the ChatGPT subscription is not a model connection: (%q, %v)", authMode, ok)
	}
	if _, ok := modelConnectionAuthMode(store.Connection{ProviderKey: "microsoft", ConnectorType: "microsoft"}); ok {
		t.Fatal("microsoft is a model connection")
	}
}
