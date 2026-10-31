package api

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

const complianceTestRegistryFormat = `[{"principal":"verevon-gateway","audience":"audit-core","token":"%s","scopes":["audit:read:self","audit:compliance:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"%s","scopes":["audit:write"],"planes":["ingestion"]}]`

func complianceTestRegistry() string {
	return fmt.Sprintf(complianceTestRegistryFormat, testGatewayToken, testWriterToken)
}

// complianceStub is the store the compliance routes read from. It records what
// the handlers asked for so the tests can assert that a refused request never
// reached it, and what self-audit rows were written.
type complianceStub struct {
	reads    int
	filter   store.ComplianceFilter
	page     store.CompliancePage
	pageErr  error
	manifest store.ComplianceManifest
	keys     []store.ComplianceKey
	report   verifier.Report
	verified []verifier.Options
	audit    []*events.AuditEvent
	auditErr error
}

func (s *complianceStub) ListAudit(context.Context, store.AuditFilter) ([]store.AuditRow, error) {
	return nil, nil
}
func (s *complianceStub) ListUsage(context.Context, store.UsageFilter) ([]store.UsageRow, error) {
	return nil, nil
}
func (s *complianceStub) SummariseUsage(context.Context, string, time.Time, time.Time) ([]store.UsageSummary, error) {
	return nil, nil
}
func (s *complianceStub) InsertUsageFromSource(context.Context, *events.UsageEvent, string) (bool, error) {
	return true, nil
}
func (s *complianceStub) InsertAuditFromSource(_ context.Context, ev *events.AuditEvent, _ string) (bool, error) {
	if s.auditErr != nil {
		return false, s.auditErr
	}
	s.audit = append(s.audit, ev)
	return true, nil
}
func (s *complianceStub) ComplianceEvents(_ context.Context, f store.ComplianceFilter) (store.CompliancePage, error) {
	s.reads++
	s.filter = f
	return s.page, s.pageErr
}
func (s *complianceStub) ComplianceManifest(context.Context, string, time.Time, time.Time) (store.ComplianceManifest, error) {
	s.reads++
	return s.manifest, nil
}
func (s *complianceStub) ComplianceKeys(context.Context, string) ([]store.ComplianceKey, error) {
	s.reads++
	return s.keys, nil
}
func (s *complianceStub) VerifyOrg(_ context.Context, _ string, opts verifier.Options) (verifier.Report, error) {
	s.reads++
	s.verified = append(s.verified, opts)
	return s.report, nil
}

var complianceNonce atomic.Int64

// signComplianceRequest signs a gateway delegation for the request as the
// gateway does: v3, bound to method, URI, user, org, role and body.
func signComplianceRequest(request *http.Request, orgID, role string, body []byte) {
	nonce := fmt.Sprintf("compliance-nonce-%016d", complianceNonce.Add(1))
	timestamp := time.Now().UTC().Format(time.RFC3339)
	bodyDigest := sha256.Sum256(body)
	bodySHA := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	request.Header.Set("X-Service-Id", "verevon-gateway")
	request.Header.Set("X-Service-Token", testGatewayToken)
	request.Header.Set("X-User-Id", "user-a")
	request.Header.Set("X-Org-Id", orgID)
	request.Header.Set("X-User-Role", role)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", timestamp)
	request.Header.Set("X-Delegation-Nonce", nonce)
	request.Header.Set("X-Delegation-Body-SHA256", bodySHA)
	canonical := strings.Join([]string{
		"v3", "verevon-gateway", "audit-core", timestamp, nonce,
		request.Method, request.URL.RequestURI(), "user-a", orgID, role, bodySHA,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(testGatewayToken))
	_, _ = mac.Write([]byte(canonical))
	request.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
}

func complianceRouter(t *testing.T, stub *complianceStub) http.Handler {
	t.Helper()
	handler, err := New(stub, complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	router := chi.NewRouter()
	handler.Mount(router)
	return router
}

// complianceGet issues a signed GET as the given role; org is both the delegated
// org and the org_id query parameter unless the target already carries one.
func complianceGet(router http.Handler, target, org, role string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, target, nil)
	signComplianceRequest(request, org, role, nil)
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	return recorder
}

func decodeEnvelope(t *testing.T, recorder *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var envelope map[string]any
	if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
		t.Fatalf("response is not JSON: %v: %s", err, recorder.Body.String())
	}
	return envelope
}

func envelopeErrorCode(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	failure, _ := decodeEnvelope(t, recorder)["error"].(map[string]any)
	code, _ := failure["code"].(string)
	return code
}

const complianceEventsURL = "/v1/compliance/audit/events?org_id=org-a"

func TestComplianceRoutesRefuseMembersAndViewersBeforeTouchingTheStore(t *testing.T) {
	for _, target := range []string{
		complianceEventsURL,
		"/v1/compliance/audit/manifest?org_id=org-a",
		"/v1/compliance/audit/keys?org_id=org-a",
		"/v1/compliance/audit/export-status?org_id=org-a",
	} {
		for _, role := range []string{"member", "viewer", "", "OWNER-ish", "billing"} {
			stub := &complianceStub{}
			recorder := complianceGet(complianceRouter(t, stub), target, "org-a", role)
			if role == "" {
				// An empty role cannot even form a delegation: the gateway always signs one.
				if recorder.Code != http.StatusForbidden {
					t.Fatalf("%s as empty role: status %d", target, recorder.Code)
				}
				continue
			}
			if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "compliance_role_required" {
				t.Fatalf("%s as %q: status %d body %s", target, role, recorder.Code, recorder.Body.String())
			}
			if stub.reads != 0 || len(stub.audit) != 0 {
				t.Fatalf("%s as %q reached the store (reads=%d audit=%d)", target, role, stub.reads, len(stub.audit))
			}
		}
	}

	stub := &complianceStub{}
	router := complianceRouter(t, stub)
	request := httptest.NewRequest(http.MethodPost, "/v1/compliance/audit/verify?org_id=org-a", strings.NewReader(`{}`))
	signComplianceRequest(request, "org-a", "member", []byte(`{}`))
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusForbidden || len(stub.verified) != 0 {
		t.Fatalf("member verify: status %d, verified %d times", recorder.Code, len(stub.verified))
	}
}

func TestComplianceEventsServeOwnersAndAdminsAndAuditTheRead(t *testing.T) {
	for _, role := range []string{"owner", "admin"} {
		stub := &complianceStub{page: store.CompliancePage{
			Events:  []store.ComplianceEvent{{OrgID: "org-a", ChainSeq: 1, Event: "tool_action"}, {OrgID: "org-a", ChainSeq: 2, Event: "tool_action"}},
			HeadSeq: 9,
		}}
		recorder := complianceGet(complianceRouter(t, stub),
			complianceEventsURL+"&event=tool_action&plane=model&since=2026-10-01T00:00:00Z", "org-a", role)
		if recorder.Code != http.StatusOK {
			t.Fatalf("%s: status %d: %s", role, recorder.Code, recorder.Body.String())
		}
		envelope := decodeEnvelope(t, recorder)
		meta := envelope["meta"].(map[string]any)
		if envelope["error"] != nil || meta["count"] != float64(2) || meta["limit"] != float64(200) ||
			meta["head_seq"] != float64(9) || meta["next_cursor"] != nil {
			t.Fatalf("%s: envelope = %s", role, recorder.Body.String())
		}
		if stub.filter.OrgID != "org-a" || stub.filter.Event != "tool_action" || stub.filter.Plane != "model" ||
			stub.filter.Since.IsZero() || stub.filter.Limit != 200 {
			t.Fatalf("%s: filter = %+v", role, stub.filter)
		}

		if len(stub.audit) != 1 {
			t.Fatalf("%s: %d self-audit rows, want 1", role, len(stub.audit))
		}
		row := stub.audit[0]
		if row.Event != "compliance_audit_read" || row.Plane != "control" || row.Producer != "audit-core" ||
			row.OrgID != "org-a" || row.UserID != "user-a" || row.ActorRole != role || row.Outcome != "ok" {
			t.Fatalf("%s: self-audit row = %+v", role, row)
		}
		if row.Details["route"] != "events" || row.Details["count"] != 2 {
			t.Fatalf("%s: self-audit details = %v", role, row.Details)
		}
		filters, _ := row.Details["filters"].(map[string]string)
		if filters["event"] != "tool_action" || filters["plane"] != "model" || filters["since"] == "" {
			t.Fatalf("%s: self-audit filters = %v", role, row.Details["filters"])
		}
		if err := row.Validate(); err != nil {
			t.Fatalf("%s: the self-audit row would be rejected by the insert path: %v", role, err)
		}
	}
}

func TestComplianceReadsAreRefusedForAnotherOrgAndForAReplayedDelegation(t *testing.T) {
	stub := &complianceStub{}
	router := complianceRouter(t, stub)

	// The delegation names org-a; asking for org-b is refused before any role check.
	recorder := complianceGet(router, "/v1/compliance/audit/events?org_id=org-b", "org-a", "owner")
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("org mismatch: status %d", recorder.Code)
	}

	request := httptest.NewRequest(http.MethodGet, complianceEventsURL, nil)
	signComplianceRequest(request, "org-a", "owner", nil)
	replay := request.Clone(context.Background())
	first := httptest.NewRecorder()
	router.ServeHTTP(first, request)
	second := httptest.NewRecorder()
	router.ServeHTTP(second, replay)
	if first.Code != http.StatusOK || second.Code != http.StatusForbidden {
		t.Fatalf("original %d, replay %d; want 200 then 403", first.Code, second.Code)
	}
	if stub.reads != 1 {
		t.Fatalf("the replay reached the store: %d reads", stub.reads)
	}
}

func TestComplianceEventsLimitIsCappedAndInvalidQueriesAreRefused(t *testing.T) {
	stub := &complianceStub{}
	router := complianceRouter(t, stub)

	recorder := complianceGet(router, complianceEventsURL+"&limit=5000", "org-a", "owner")
	if recorder.Code != http.StatusOK || stub.filter.Limit != 1000 {
		t.Fatalf("limit=5000: status %d, store limit %d, want 1000", recorder.Code, stub.filter.Limit)
	}
	if limit := decodeEnvelope(t, recorder)["meta"].(map[string]any)["limit"]; limit != float64(1000) {
		t.Fatalf("meta.limit = %v, want the cap", limit)
	}

	for target, code := range map[string]string{
		complianceEventsURL + "&limit=0":                                               "invalid_limit",
		complianceEventsURL + "&limit=abc":                                             "invalid_limit",
		complianceEventsURL + "&since=yesterday":                                       "invalid_time",
		complianceEventsURL + "&since=2026-10-02T00:00:00Z&until=2026-10-01T00:00:00Z": "invalid_time",
		complianceEventsURL + "&cursor=not-a-cursor":                                   "invalid_cursor",
		complianceEventsURL + "&sort=desc":                                             "invalid_query",
		complianceEventsURL + "&event=a&event=b":                                       "invalid_query",
		complianceEventsURL + "&user_id=" + strings.Repeat("u", 129):                   "invalid_query",
	} {
		stub.reads = 0
		recorder := complianceGet(router, target, "org-a", "owner")
		if recorder.Code != http.StatusBadRequest || envelopeErrorCode(t, recorder) != code {
			t.Fatalf("%s: status %d code %q, want 400 %q", target, recorder.Code, envelopeErrorCode(t, recorder), code)
		}
		if stub.reads != 0 {
			t.Fatalf("%s reached the store", target)
		}
	}
}

func TestComplianceCursorResumesAfterTheLastSeqAndIsBoundToTheOrg(t *testing.T) {
	stub := &complianceStub{page: store.CompliancePage{
		Events:  []store.ComplianceEvent{{ChainSeq: 3}},
		NextSeq: 3, HeadSeq: 10,
	}}
	router := complianceRouter(t, stub)

	recorder := complianceGet(router, complianceEventsURL+"&limit=1", "org-a", "owner")
	next, _ := decodeEnvelope(t, recorder)["meta"].(map[string]any)["next_cursor"].(string)
	if next == "" {
		t.Fatalf("a page with more rows must return next_cursor: %s", recorder.Body.String())
	}

	stub.page = store.CompliancePage{Events: []store.ComplianceEvent{{ChainSeq: 4}}, HeadSeq: 10}
	recorder = complianceGet(router, complianceEventsURL+"&cursor="+next, "org-a", "owner")
	if recorder.Code != http.StatusOK || stub.filter.AfterSeq != 3 {
		t.Fatalf("cursor continuation: status %d after_seq %d, want 200 and 3", recorder.Code, stub.filter.AfterSeq)
	}
	if cursor := decodeEnvelope(t, recorder)["meta"].(map[string]any)["next_cursor"]; cursor != nil {
		t.Fatalf("the last page must not return a cursor, got %v", cursor)
	}

	// The same cursor presented under another org is refused, not silently reset.
	stub.reads = 0
	recorder = complianceGet(router, "/v1/compliance/audit/events?org_id=org-b&cursor="+next, "org-b", "owner")
	if recorder.Code != http.StatusBadRequest || envelopeErrorCode(t, recorder) != "invalid_cursor" || stub.reads != 0 {
		t.Fatalf("foreign cursor: status %d code %q reads %d", recorder.Code, envelopeErrorCode(t, recorder), stub.reads)
	}
}

func TestComplianceReadFailsClosedWhenItCannotBeAudited(t *testing.T) {
	stub := &complianceStub{
		page:     store.CompliancePage{Events: []store.ComplianceEvent{{ChainSeq: 1, Event: "secret_looking_row"}}},
		auditErr: errors.New("database is down"),
	}
	recorder := complianceGet(complianceRouter(t, stub), complianceEventsURL, "org-a", "owner")
	if recorder.Code != http.StatusInternalServerError || envelopeErrorCode(t, recorder) != "audit_write_failed" {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if strings.Contains(recorder.Body.String(), "secret_looking_row") {
		t.Fatal("rows were returned although the read could not be audited")
	}
}

func TestComplianceStoreFailureIsAnOpaque500(t *testing.T) {
	stub := &complianceStub{pageErr: errors.New("pq: relation audit_events is broken")}
	recorder := complianceGet(complianceRouter(t, stub), complianceEventsURL, "org-a", "owner")
	if recorder.Code != http.StatusInternalServerError || strings.Contains(recorder.Body.String(), "pq:") {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if len(stub.audit) != 0 {
		t.Fatal("a failed read must not be recorded as a completed read")
	}
}

// signManifestFor builds the manifest a store with a signing key would return.
func signManifestFor(t *testing.T, priv ed25519.PrivateKey) store.ComplianceManifest {
	t.Helper()
	m := store.ComplianceManifest{
		OrgID:         "org-a",
		GeneratedAt:   time.Date(2026, 10, 24, 12, 0, 0, 654321000, time.UTC),
		FirstSeq:      1,
		LastSeq:       7,
		Count:         7,
		FirstPrevHash: bytes.Repeat([]byte{0xa1}, chain.HashSize),
		LastRowHash:   bytes.Repeat([]byte{0xb2}, chain.HashSize),
		HeadSeq:       7,
		HeadHash:      bytes.Repeat([]byte{0xb2}, chain.HashSize),
	}
	signer := keySigner{id: "audit-key", key: priv}
	if err := store.SignManifest(&m, signer); err != nil {
		t.Fatal(err)
	}
	return m
}

type keySigner struct {
	id  string
	key ed25519.PrivateKey
}

func (s keySigner) SignPayload(payload []byte) (string, []byte, error) {
	return s.id, ed25519.Sign(s.key, payload), nil
}

func TestComplianceManifestSignatureVerifiesWithTheKeysEndpoint(t *testing.T) {
	priv := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{7}, ed25519.SeedSize))
	pub := priv.Public().(ed25519.PublicKey)
	stub := &complianceStub{
		manifest: signManifestFor(t, priv),
		keys: []store.ComplianceKey{{
			KeyID: "audit-key", Algorithm: "ed25519", Status: store.KeyStatusActive,
			PublicKey: base64.RawURLEncoding.EncodeToString(pub),
		}},
	}
	router := complianceRouter(t, stub)

	manifestResponse := complianceGet(router, "/v1/compliance/audit/manifest?org_id=org-a", "org-a", "admin")
	keysResponse := complianceGet(router, "/v1/compliance/audit/keys?org_id=org-a", "org-a", "admin")
	if manifestResponse.Code != http.StatusOK || keysResponse.Code != http.StatusOK {
		t.Fatalf("manifest %d keys %d", manifestResponse.Code, keysResponse.Code)
	}

	// Decode exactly what a client saves to disk, not the in-memory struct.
	var served struct {
		Data store.ComplianceManifest `json:"data"`
	}
	if err := json.Unmarshal(manifestResponse.Body.Bytes(), &served); err != nil {
		t.Fatal(err)
	}
	var keys struct {
		Data []store.ComplianceKey `json:"data"`
	}
	if err := json.Unmarshal(keysResponse.Body.Bytes(), &keys); err != nil {
		t.Fatal(err)
	}
	if len(keys.Data) != 1 || served.Data.KeyID != keys.Data[0].KeyID {
		t.Fatalf("manifest key %q is not listed by /keys: %+v", served.Data.KeyID, keys.Data)
	}
	published, err := base64.RawURLEncoding.DecodeString(keys.Data[0].PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	payload, err := store.ManifestPayload(served.Data)
	if err != nil {
		t.Fatal(err)
	}
	if !ed25519.Verify(published, payload, served.Data.Signature) {
		t.Fatal("the served manifest does not verify with the public key /keys serves")
	}

	served.Data.Count++
	tampered, _ := store.ManifestPayload(served.Data)
	if ed25519.Verify(published, tampered, served.Data.Signature) {
		t.Fatal("a manifest with an edited count still verifies")
	}
	if len(stub.audit) != 2 || stub.audit[0].Details["route"] != "manifest" || stub.audit[1].Details["route"] != "keys" {
		t.Fatalf("self-audit rows = %d", len(stub.audit))
	}
}

func TestComplianceVerifyReturnsTheReportAndAuditsIt(t *testing.T) {
	stub := &complianceStub{report: verifier.Report{
		OrgID: "org-a", Status: verifier.StatusBroken, Reason: verifier.ReasonRowHashMismatch,
		FirstBrokenSeq: 4, RowsVerified: 3,
	}}
	router := complianceRouter(t, stub)

	body := []byte(`{"from_seq":2,"max_rows":50}`)
	request := httptest.NewRequest(http.MethodPost, "/v1/compliance/audit/verify?org_id=org-a", bytes.NewReader(body))
	signComplianceRequest(request, "org-a", "owner", body)
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	report := decodeEnvelope(t, recorder)["data"].(map[string]any)
	if report["status"] != "broken" || report["first_broken_seq"] != float64(4) {
		t.Fatalf("report = %v; a broken chain must be reported as broken with its sequence", report)
	}
	if len(stub.verified) != 1 || stub.verified[0].FromSeq != 2 || stub.verified[0].MaxRows != 50 {
		t.Fatalf("verify options = %+v", stub.verified)
	}
	if len(stub.audit) != 1 || stub.audit[0].Event != "compliance_audit_verify" ||
		stub.audit[0].Details["status"] != "broken" || stub.audit[0].Details["first_broken_seq"] != int64(4) {
		t.Fatalf("self-audit = %+v", stub.audit)
	}

	// No body means the defaults, bounded.
	stub.verified = nil
	request = httptest.NewRequest(http.MethodPost, "/v1/compliance/audit/verify?org_id=org-a", nil)
	signComplianceRequest(request, "org-a", "admin", nil)
	recorder = httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusOK || len(stub.verified) != 1 || stub.verified[0].MaxRows != complianceVerifyDefaultRows {
		t.Fatalf("empty body: status %d options %+v", recorder.Code, stub.verified)
	}

	for name, bad := range map[string]string{
		"unknown field": `{"force":true}`,
		"trailing":      `{} {}`,
		"too many rows": fmt.Sprintf(`{"max_rows":%d}`, complianceVerifyMaxRows+1),
		"negative":      `{"from_seq":-1}`,
		"not hex":       `{"from_hash":"zz"}`,
	} {
		stub.verified = nil
		request = httptest.NewRequest(http.MethodPost, "/v1/compliance/audit/verify?org_id=org-a", strings.NewReader(bad))
		signComplianceRequest(request, "org-a", "owner", []byte(bad))
		recorder = httptest.NewRecorder()
		router.ServeHTTP(recorder, request)
		if recorder.Code != http.StatusBadRequest || len(stub.verified) != 0 {
			t.Fatalf("%s: status %d, verified %d times", name, recorder.Code, len(stub.verified))
		}
	}
}

func TestComplianceExportStatusIsEmptyUntilTheExporterExists(t *testing.T) {
	stub := &complianceStub{}
	recorder := complianceGet(complianceRouter(t, stub), "/v1/compliance/audit/export-status?org_id=org-a", "org-a", "owner")
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	data := decodeEnvelope(t, recorder)["data"].(map[string]any)
	if data["exporter_enabled"] != false || len(data["destinations"].([]any)) != 0 {
		t.Fatalf("export status = %v", data)
	}
	if len(stub.audit) != 1 || stub.audit[0].Details["route"] != "export_status" {
		t.Fatalf("self-audit = %+v", stub.audit)
	}
}

func TestComplianceKeysAreAnEmptyListNotNull(t *testing.T) {
	recorder := complianceGet(complianceRouter(t, &complianceStub{}), "/v1/compliance/audit/keys?org_id=org-a", "org-a", "owner")
	if recorder.Code != http.StatusOK || !strings.Contains(recorder.Body.String(), `"data":[]`) {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
}

func TestRequireComplianceRoleAcceptsOnlyOwnerAndAdmin(t *testing.T) {
	for role, want := range map[string]bool{
		"owner": true, "admin": true, "Owner": true, " admin ": true,
		"member": false, "viewer": false, "": false, "owner,member": false, "superadmin": false,
	} {
		if got := isComplianceRole(role); got != want {
			t.Fatalf("isComplianceRole(%q) = %t, want %t", role, got, want)
		}
	}

	request := httptest.NewRequest(http.MethodGet, complianceEventsURL, nil)
	recorder := httptest.NewRecorder()
	if _, ok := requireComplianceRole(recorder, request); ok || recorder.Code != http.StatusForbidden {
		t.Fatalf("a request with no authorization in its context must be refused, got ok=%t status=%d", ok, recorder.Code)
	}
}
