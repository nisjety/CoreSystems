package cost

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/golang-jwt/jwt/v5"
)

// ── the golden: the exact body the forwarder posts ───────────────────────────

const goldenPath = "../../testdata/forwarder-record-request.golden.json"

// goldenFile is testdata/forwarder-record-request.golden.json. It pins the exact bytes
// BuildRecordRequest produces for four reference rows, and is the contract with cost-core:
// cost-core decodes this body strictly, so a field it does not name is a 400. CR-T4 vendors a
// byte-for-byte copy under cost-core's tree (internal packages cannot be imported across the
// two modules), checks the pin, and posts every `body` through cost-core's real handler,
// authorizer and decoder; scripts/check-usage-contract.sh fails when the copies differ.
type goldenFile struct {
	Contract  string       `json:"contract"`
	Endpoint  string       `json:"endpoint"`
	Audience  string       `json:"audience"`
	Principal string       `json:"principal"`
	Scope     string       `json:"scope"`
	Note      string       `json:"note"`
	Cases     []goldenCase `json:"cases"`
	Fields    []string     `json:"fields"`
}

type goldenCase struct {
	Name string          `json:"name"`
	Row  goldenRow       `json:"row"`
	Body json.RawMessage `json:"body"`
}

type goldenRow struct {
	OrgID           string `json:"org_id"`
	Model           string `json:"model"`
	Feature         string `json:"feature"`
	ProviderUsed    string `json:"provider_used"`
	InputTokens     int64  `json:"input_tokens"`
	OutputTokens    int64  `json:"output_tokens"`
	CacheReadTokens int64  `json:"cache_read_tokens"`
	Units           *int64 `json:"units,omitempty"`
	CostBasis       string `json:"cost_basis"`
	Estimated       bool   `json:"estimated"`
	RequestID       string `json:"request_id"`
	IdempotencyKey  string `json:"idempotency_key"`
}

func (r goldenRow) pending() PendingRow {
	return PendingRow{
		OrgID: r.OrgID, Model: r.Model, Feature: r.Feature, ProviderUsed: r.ProviderUsed,
		InputTokens: r.InputTokens, OutputTokens: r.OutputTokens, CacheReadTokens: r.CacheReadTokens,
		Units: r.Units, CostBasis: r.CostBasis, Estimated: r.Estimated, RequestID: r.RequestID,
		IdempotencyKey: r.IdempotencyKey,
	}
}

func i64(v int64) *int64 { return &v }

func referenceRows() []struct {
	name string
	row  goldenRow
} {
	digest := func(seed string) string {
		sum := sha256.Sum256([]byte(seed))
		return hex.EncodeToString(sum[:])
	}
	row := func(org, model, feature, provider string, in, out, cache int64, units *int64, basis string, estimated bool, seed string) goldenRow {
		d := digest(seed)
		return goldenRow{
			OrgID: org, Model: model, Feature: feature, ProviderUsed: provider,
			InputTokens: in, OutputTokens: out, CacheReadTokens: cache, Units: units,
			CostBasis: basis, Estimated: estimated, RequestID: d,
			// What the consumer stores: the organization, then the producer's key.
			IdempotencyKey: org + ":usage:" + feature + ":" + d,
		}
	}
	return []struct {
		name string
		row  goldenRow
	}{
		{"llm-metered-contextual-retrieval", row("org-golden-a", "gpt-4o-mini-2024-07-18", "contextual-retrieval", "azure-openai", 3672, 177, 2048, nil, "metered", false, "golden-ctx")},
		{"embedding-estimated-has-no-cost-basis", row("org-golden-a", "embed-v-4-0", "embedding", "azure-cohere", 9100, 0, 0, nil, "estimated", true, "golden-embed")},
		{"web-rerank-units", row("org-golden-b", "rerank-v4.0-pro", "web-rerank", "azure-cohere", 0, 0, 0, i64(3), "metered", false, "golden-rerank")},
		{"graph-extraction-zero-retention-organization", row("org-golden-zdr", "gpt-4o", "graph-extraction", "azure-openai", 4000, 780, 0, nil, "metered", false, "golden-graph")},
	}
}

// costCoreFields are the JSON fields cost-core's record endpoint names, once CR-T4 has added
// feature and units. Anything the forwarder posts outside this set is a 400 there.
var costCoreFields = []string{
	"org_id", "user_id", "run_id", "request_id", "model", "provider_used", "input_tokens", "output_tokens",
	"cost_usd", "idempotency_key", "cache_read_input_tokens", "cache_creation_input_tokens", "cost_basis",
	"feature", "units",
}

func buildGolden() goldenFile {
	golden := goldenFile{
		Contract:  "data-orchestrator forwarder to cost-core: POST /api/v1/cost/record",
		Endpoint:  "POST /api/v1/cost/record",
		Audience:  "cost-core",
		Principal: "data-orchestrator",
		Scope:     "cost:write",
		Note: "Each body is posted exactly as written (compact JSON, this field order) with a bearer minted for the " +
			"organization in its org_id. cost-core must answer 202 for every body, and again for a second post of " +
			"the same body without recording a second row. The idempotency key starts with the organization " +
			"because cost-core's unique index on it is global.",
		Fields: costCoreFields,
	}
	for _, reference := range referenceRows() {
		body, err := json.Marshal(BuildRecordRequest(reference.row.pending()))
		if err != nil {
			panic(err)
		}
		golden.Cases = append(golden.Cases, goldenCase{Name: reference.name, Row: reference.row, Body: body})
	}
	return golden
}

func goldenPin(text string) string {
	sum := sha256.Sum256([]byte(strings.ReplaceAll(text, "\r\n", "\n")))
	return hex.EncodeToString(sum[:])
}

func TestForwarderRequestMatchesItsGoldenAndPin(t *testing.T) {
	want := buildGolden()
	if os.Getenv("UPDATE_FORWARDER_GOLDEN") != "" {
		text, err := json.MarshalIndent(want, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		out := append(text, '\n')
		if err := os.WriteFile(filepath.FromSlash(goldenPath), out, 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.FromSlash(goldenPath+".sha256"), []byte(goldenPin(string(out))+"\n"), 0o644); err != nil {
			t.Fatal(err)
		}
		t.Log("forwarder golden regenerated: CR-T4 vendors it; copy the file and its pin under cost-core and run scripts/check-usage-contract.sh")
	}
	raw, err := os.ReadFile(filepath.FromSlash(goldenPath))
	if err != nil {
		t.Fatalf("the forwarder golden is missing: %v (generate it with UPDATE_FORWARDER_GOLDEN=1). A missing contract is a failure, never a skip", err)
	}
	pin, err := os.ReadFile(filepath.FromSlash(goldenPath + ".sha256"))
	if err != nil {
		t.Fatalf("the forwarder golden has no pin: %v", err)
	}
	if got := goldenPin(string(raw)); got != strings.TrimSpace(string(pin)) {
		t.Fatalf("the golden was edited without regenerating its pin (sha256 %s, pinned %s)", got, strings.TrimSpace(string(pin)))
	}
	var got goldenFile
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("parse the golden: %v", err)
	}
	if len(got.Cases) != len(want.Cases) {
		t.Fatalf("the golden has %d cases, the reference rows are %d", len(got.Cases), len(want.Cases))
	}
	for i, wantCase := range want.Cases {
		var compact bytes.Buffer
		if err := json.Compact(&compact, got.Cases[i].Body); err != nil {
			t.Fatalf("case %s body: %v", wantCase.Name, err)
		}
		if got.Cases[i].Name != wantCase.Name || !bytes.Equal(compact.Bytes(), wantCase.Body) {
			t.Errorf("case %s: the forwarder now posts\n  %s\nand the golden says\n  %s\nIf the change is intended, regenerate with UPDATE_FORWARDER_GOLDEN=1 and hand the new golden to the cost-core build (CR-T4)", wantCase.Name, wantCase.Body, compact.Bytes())
		}
	}
}

// What the forwarder may post, and what it must not: every field is one cost-core names, the
// organization is in the key (cost-core's index is global), no price is computed here and no
// user is named.
func TestForwarderBodyOnlyNamesFieldsCostCoreAcceptsAndEmbedsTheOrganization(t *testing.T) {
	allowed := map[string]bool{}
	for _, field := range costCoreFields {
		allowed[field] = true
	}
	// Every json tag of the struct, so a field added to it is checked, not only the ones the
	// reference rows happen to fill.
	typ := reflect.TypeOf(RecordRequest{})
	for i := 0; i < typ.NumField(); i++ {
		name := strings.Split(typ.Field(i).Tag.Get("json"), ",")[0]
		if !allowed[name] {
			t.Errorf("RecordRequest posts %q, which cost-core does not name: it would answer 400 for every row", name)
		}
		if name == "user_id" || name == "cost_usd" || name == "run_id" {
			t.Errorf("RecordRequest names %q: usage is attributed to the producer, and priced by cost-core", name)
		}
	}
	for _, reference := range referenceRows() {
		body := BuildRecordRequest(reference.row.pending())
		if !strings.HasPrefix(body.IdempotencyKey, "dataplane:"+reference.row.OrgID+":") {
			t.Errorf("%s: key %q does not start with dataplane:<org>:", reference.name, body.IdempotencyKey)
		}
		if body.Feature == "" {
			t.Errorf("%s: a row went out with no feature: its spend would count against the chat budget", reference.name)
		}
		if reference.row.Estimated && body.CostBasis != "" {
			t.Errorf("%s: an estimate was posted as %q: a report would present it as a measurement", reference.name, body.CostBasis)
		}
		if !reference.row.Estimated && body.CostBasis != "metered" {
			t.Errorf("%s: a metered row was posted with basis %q", reference.name, body.CostBasis)
		}
	}
}

// ── stand-ins that apply the real rules ──────────────────────────────────────

// authCore is Auth Core's mint for the cost-core audience, with the rules of
// plane-token.controller.ts that matter here: the credential and service id must match, the
// body is read for orgId, scopes and a reason of 3 to 500 characters, an organization the
// directory does not hold is refused, and the token carries the claims issuePlaneToken puts
// on a service token.
type authCore struct {
	t         *testing.T
	server    *httptest.Server
	key       *rsa.PrivateKey
	issuer    string
	serviceID string
	apiKey    string
	known     map[string]bool
	ttl       time.Duration

	mu     sync.Mutex
	mints  map[string]int
	status int // when set, answer this instead
	wrong  string
}

func newAuthCore(t *testing.T, key *rsa.PrivateKey, orgs ...string) *authCore {
	t.Helper()
	a := &authCore{
		t: t, key: key, issuer: "http://auth-core:3011/api/convex-auth", serviceID: "data-orchestrator",
		apiKey: strings.Repeat("k", 40), known: map[string]bool{}, ttl: 5 * time.Minute, mints: map[string]int{},
	}
	for _, org := range orgs {
		a.known[org] = true
	}
	a.server = httptest.NewServer(http.HandlerFunc(a.serve))
	t.Cleanup(a.server.Close)
	return a
}

func (a *authCore) url() string { return a.server.URL + "/api/cost-core/internal-token" }

func (a *authCore) mintCount(org string) int {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.mints[org]
}

func (a *authCore) serve(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost || r.URL.Path != "/api/cost-core/internal-token" {
		http.NotFound(w, r)
		return
	}
	if r.Header.Get("x-service-id") != a.serviceID || r.Header.Get("x-service-api-key") != a.apiKey {
		http.Error(w, "Service principal is not authorized", http.StatusForbidden)
		return
	}
	var body struct {
		OrgID  string   `json:"orgId"`
		Scopes []string `json:"scopes"`
		Reason string   `json:"reason"`
	}
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&body); err != nil {
		http.Error(w, "bad request", http.StatusBadRequest)
		return
	}
	if len(strings.TrimSpace(body.Reason)) < 3 || len(body.Reason) > 500 || len(body.Scopes) != 1 || body.Scopes[0] != "cost:write" {
		http.Error(w, "Service principal is not authorized", http.StatusForbidden)
		return
	}
	a.mu.Lock()
	status, wrong := a.status, a.wrong
	a.mu.Unlock()
	if status != 0 {
		http.Error(w, "unavailable", status)
		return
	}
	if !a.known[body.OrgID] { // tenancy: directory
		http.Error(w, "Service principal is not authorized", http.StatusForbidden)
		return
	}
	a.mu.Lock()
	a.mints[body.OrgID]++
	n := a.mints[body.OrgID]
	a.mu.Unlock()
	org := body.OrgID
	if wrong != "" {
		org = wrong
	}
	now := time.Now()
	claims := jwt.MapClaims{
		"iss": a.issuer, "aud": "cost-core", "sub": "service:" + a.serviceID, "service_id": "service:" + a.serviceID,
		"principal_type": "service", "org_id": org, "scopes": []string{"cost:write"}, "zdr": false,
		"jti": fmt.Sprintf("%s-%d", org, n), "iat": now.Unix(), "nbf": now.Unix() - 5, "exp": now.Add(a.ttl).Unix(),
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = "test-key"
	signed, err := token.SignedString(a.key)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{
		"token": signed, "expiresAt": now.Add(a.ttl).UTC().Format(time.RFC3339), "expiresInSeconds": int(a.ttl.Seconds()),
		"issuer": a.issuer, "audience": "cost-core",
	})
}

// costCoreEntry is what cost-core would write.
type costCoreEntry struct {
	OrgID, ProducerID, RequestID, Model, ProviderUsed, CostBasis, IdempotencyKey, Feature string
	InputTokens, OutputTokens, CacheReadTokens                                            int64
	Units                                                                                 *int64
}

// costCore applies the rules of cost-core's record endpoint as they are in
// internal/server/server.go and internal/ledger/ledger.go: the bearer is verified (signature,
// audience, expiry) and CostAuthorizer's rule for a service principal (cost:write, nothing
// else) applies; the body is read with DisallowUnknownFields and one object, at most 64 KiB;
// the write is pinned to the organization in the token; cost_basis is metered, subscription or
// absent; a service principal's subject becomes the producer id; every string is canonical
// and bounded, every count non-negative; and the idempotency key is unique across ALL
// organizations. feature and units are the fields CR-T4 adds.
type costCore struct {
	t      *testing.T
	server *httptest.Server
	key    *rsa.PublicKey

	mu       sync.Mutex
	entries  []costCoreEntry
	seen     map[string]bool
	requests int
	respond  []int // statuses to answer first, one per request, instead of the rules
	auths    []string
}

func newCostCore(t *testing.T, key *rsa.PublicKey) *costCore {
	t.Helper()
	c := &costCore{t: t, key: key, seen: map[string]bool{}}
	c.server = httptest.NewServer(http.HandlerFunc(c.serve))
	t.Cleanup(c.server.Close)
	return c
}

func (c *costCore) recorded() []costCoreEntry {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]costCoreEntry(nil), c.entries...)
}

func (c *costCore) requestCount() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.requests
}

func (c *costCore) answerFirst(statuses ...int) {
	c.mu.Lock()
	c.respond = statuses
	c.mu.Unlock()
}

type costCoreRequest struct {
	OrgID                    string  `json:"org_id"`
	UserID                   string  `json:"user_id"`
	RunID                    string  `json:"run_id"`
	RequestID                string  `json:"request_id"`
	Model                    string  `json:"model"`
	ProviderUsed             string  `json:"provider_used"`
	InputTokens              int64   `json:"input_tokens"`
	OutputTokens             int64   `json:"output_tokens"`
	CostUSD                  float64 `json:"cost_usd"`
	IdempotencyKey           string  `json:"idempotency_key"`
	CacheReadInputTokens     int64   `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int64   `json:"cache_creation_input_tokens"`
	CostBasis                string  `json:"cost_basis"`
	Feature                  string  `json:"feature"`
	Units                    *int64  `json:"units"`
}

var featurePattern = regexp.MustCompile(`^[a-z][a-z0-9-]{0,62}$`)

func canonical(value string, limit int) bool {
	if value == "" {
		return true
	}
	if !utf8.ValidString(value) || value != strings.TrimSpace(value) || len(value) > limit {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

func (c *costCore) serve(w http.ResponseWriter, r *http.Request) {
	c.mu.Lock()
	c.requests++
	c.auths = append(c.auths, r.Header.Get("x-service-api-key"))
	var forced int
	if len(c.respond) > 0 {
		forced, c.respond = c.respond[0], c.respond[1:]
	}
	c.mu.Unlock()
	if forced != 0 {
		http.Error(w, "forced", forced)
		return
	}
	if r.Method != http.MethodPost || r.URL.Path != "/api/v1/cost/record" {
		http.NotFound(w, r)
		return
	}
	// Bearer: signature, audience, expiry.
	tokenString, ok := strings.CutPrefix(r.Header.Get("Authorization"), "Bearer ")
	if !ok || tokenString == "" {
		http.Error(w, "verified identity is required", http.StatusUnauthorized)
		return
	}
	claims := jwt.MapClaims{}
	token, err := jwt.ParseWithClaims(tokenString, claims, func(*jwt.Token) (any, error) { return c.key, nil },
		jwt.WithValidMethods([]string{"RS256"}), jwt.WithAudience("cost-core"), jwt.WithExpirationRequired())
	if err != nil || !token.Valid {
		http.Error(w, "verified identity is required", http.StatusUnauthorized)
		return
	}
	// CostAuthorizer: a service principal may write with cost:write and do nothing else.
	if claims["principal_type"] != "service" {
		http.Error(w, "user principal is not authorized for this cost operation", http.StatusForbidden)
		return
	}
	hasScope := false
	if scopes, ok := claims["scopes"].([]any); ok {
		for _, scope := range scopes {
			hasScope = hasScope || scope == "cost:write"
		}
	}
	if !hasScope {
		http.Error(w, "cost:write scope is required", http.StatusForbidden)
		return
	}
	// decodeRequestJSON.
	var req costCoreRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 64<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&req); err != nil {
		http.Error(w, fmt.Sprintf("invalid request body: %v", err), http.StatusBadRequest)
		return
	}
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		http.Error(w, "request body must contain one JSON object", http.StatusBadRequest)
		return
	}
	// pinOrganization.
	verifiedOrg, _ := claims["org_id"].(string)
	if requested := strings.TrimSpace(req.OrgID); requested != "" && requested != verifiedOrg {
		http.Error(w, "organization scope does not match verified token", http.StatusForbidden)
		return
	}
	req.OrgID = verifiedOrg
	if req.OrgID == "" {
		http.Error(w, "org_id is required", http.StatusBadRequest)
		return
	}
	switch strings.ToLower(strings.TrimSpace(req.CostBasis)) {
	case "", "metered", "subscription":
	default:
		http.Error(w, "cost_basis must be metered or subscription", http.StatusBadRequest)
		return
	}
	producer, _ := claims["sub"].(string)
	// ValidateEntry.
	if !canonical(req.OrgID, 256) || !canonical(req.UserID, 256) || !canonical(req.RunID, 256) || !canonical(req.RequestID, 256) ||
		!canonical(req.Model, 256) || !canonical(req.ProviderUsed, 256) || !canonical(producer, 256) ||
		!canonical(req.IdempotencyKey, 512) || req.UserID == "" && producer == "" ||
		req.InputTokens < 0 || req.OutputTokens < 0 || req.InputTokens > 1_000_000_000_000 || req.OutputTokens > 1_000_000_000_000 ||
		req.CacheReadInputTokens < 0 || req.CacheCreationInputTokens < 0 || req.CostUSD < 0 ||
		(req.Feature != "" && !featurePattern.MatchString(req.Feature)) || (req.Units != nil && *req.Units < 0) {
		http.Error(w, "invalid accounting entry", http.StatusBadRequest)
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	// The unique index on a non-empty key is global, not per organization.
	if req.IdempotencyKey == "" || !c.seen[req.IdempotencyKey] {
		if req.IdempotencyKey != "" {
			c.seen[req.IdempotencyKey] = true
		}
		c.entries = append(c.entries, costCoreEntry{
			OrgID: req.OrgID, ProducerID: producer, RequestID: req.RequestID, Model: req.Model, ProviderUsed: req.ProviderUsed,
			CostBasis: strings.ToLower(strings.TrimSpace(req.CostBasis)), IdempotencyKey: req.IdempotencyKey, Feature: req.Feature,
			InputTokens: req.InputTokens, OutputTokens: req.OutputTokens, CacheReadTokens: req.CacheReadInputTokens, Units: req.Units,
		})
	}
	w.WriteHeader(http.StatusAccepted)
	_, _ = w.Write([]byte(`{"recorded":true}`))
}

// ── a queue with the leasing behaviour of cost_events ────────────────────────

type queuedRow struct {
	row       PendingRow
	forwarded bool
	nextAt    time.Time
	leaseTill time.Time
	class     string
}

// memoryQueue behaves like the claim, mark and backlog statements for the purposes of the
// forwarder's logic: due rows are leased oldest first, a forwarded row never comes back, a
// failed one waits for its next attempt. The statements themselves are run against a real
// Postgres by integration_test.go.
type memoryQueue struct {
	mu            sync.Mutex
	now           func() time.Time
	rows          []*queuedRow
	failMark      int // fail this many MarkForwarded calls first
	claims        int
	claimedTwice  map[int64]int
	forwardedMark int
}

func newMemoryQueue(now func() time.Time, rows ...PendingRow) *memoryQueue {
	q := &memoryQueue{now: now, claimedTwice: map[int64]int{}}
	for i, row := range rows {
		row.ID = int64(i + 1)
		row.CreatedAt = now()
		q.rows = append(q.rows, &queuedRow{row: row})
	}
	return q
}

func (q *memoryQueue) Claim(_ context.Context, limit int, lease time.Duration) ([]PendingRow, error) {
	q.mu.Lock()
	defer q.mu.Unlock()
	q.claims++
	var out []PendingRow
	for _, r := range q.rows {
		if len(out) == limit {
			break
		}
		if r.forwarded || r.nextAt.After(q.now()) || r.leaseTill.After(q.now()) {
			continue
		}
		r.leaseTill = q.now().Add(lease)
		q.claimedTwice[r.row.ID]++
		out = append(out, r.row)
	}
	return out, nil
}

func (q *memoryQueue) find(id int64) *queuedRow {
	for _, r := range q.rows {
		if r.row.ID == id {
			return r
		}
	}
	return nil
}

func (q *memoryQueue) MarkForwarded(_ context.Context, id int64) error {
	q.mu.Lock()
	defer q.mu.Unlock()
	if q.failMark > 0 {
		q.failMark--
		return errors.New("database unavailable")
	}
	r := q.find(id)
	r.forwarded, r.leaseTill, r.class = true, time.Time{}, ""
	q.forwardedMark++
	return nil
}

func (q *memoryQueue) MarkFailed(_ context.Context, id int64, class string, next time.Time) error {
	q.mu.Lock()
	defer q.mu.Unlock()
	r := q.find(id)
	r.row.Attempts++
	r.class, r.nextAt, r.leaseTill = class, next, time.Time{}
	return nil
}

func (q *memoryQueue) Backlog(context.Context) (int64, time.Duration, error) {
	q.mu.Lock()
	defer q.mu.Unlock()
	var pending int64
	var oldest time.Duration
	for _, r := range q.rows {
		if !r.forwarded {
			pending++
			oldest = max(oldest, q.now().Sub(r.row.CreatedAt))
		}
	}
	return pending, oldest, nil
}

func (q *memoryQueue) state(id int64) queuedRow {
	q.mu.Lock()
	defer q.mu.Unlock()
	return *q.find(id)
}

// ── the rig ──────────────────────────────────────────────────────────────────

type forwardRig struct {
	auth      *authCore
	cost      *costCore
	tokens    *AuthCoreTokens
	queue     *memoryQueue
	forwarder *Forwarder
	clock     *testClock
}

type testClock struct {
	mu sync.Mutex
	at time.Time
}

func (c *testClock) now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.at
}

func (c *testClock) advance(d time.Duration) {
	c.mu.Lock()
	c.at = c.at.Add(d)
	c.mu.Unlock()
}

var (
	rigKeyOnce sync.Once
	rigKey     *rsa.PrivateKey
)

func newForwardRig(t *testing.T, orgs []string, rows ...PendingRow) *forwardRig {
	t.Helper()
	rigKeyOnce.Do(func() {
		key, err := rsa.GenerateKey(rand.Reader, 2048)
		if err != nil {
			t.Fatalf("generate RSA key: %v", err)
		}
		rigKey = key
	})
	clock := &testClock{at: time.Now()}
	auth := newAuthCore(t, rigKey, orgs...)
	cost := newCostCore(t, &rigKey.PublicKey)
	tokens, err := NewAuthCoreTokens(auth.url(), "data-orchestrator", auth.apiKey, auth.issuer, nil)
	if err != nil {
		t.Fatalf("token client: %v", err)
	}
	tokens.now = clock.now
	queue := newMemoryQueue(clock.now, rows...)
	forwarder, err := NewForwarder(queue, tokens, nil, ForwarderConfig{BaseURL: cost.server.URL, BatchSize: 50, Lease: 2 * time.Minute})
	if err != nil {
		t.Fatalf("forwarder: %v", err)
	}
	forwarder.now = clock.now
	return &forwardRig{auth: auth, cost: cost, tokens: tokens, queue: queue, forwarder: forwarder, clock: clock}
}

func usageRow(org, feature, seed string) PendingRow {
	sum := sha256.Sum256([]byte(seed))
	digest := hex.EncodeToString(sum[:])
	return PendingRow{
		OrgID: org, EventType: "llm", Model: "gpt-4o-mini-2024-07-18", Feature: feature, ProviderUsed: "azure-openai",
		InputTokens: 1200, OutputTokens: 60, CacheReadTokens: 512, CostBasis: "metered", RequestID: digest,
		IdempotencyKey: org + ":usage:" + feature + ":" + digest,
	}
}

// ── what the forwarder does ──────────────────────────────────────────────────

func TestForwardedRowsAreAcceptedByCostCoreOnceWithTheirOrganizationInTheKeyAndTheirOwnBearer(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a", "org-b"},
		usageRow("org-a", "contextual-retrieval", "a1"),
		usageRow("org-b", "contextual-retrieval", "b1"),
		usageRow("org-a", "graph-extraction", "a2"),
		usageRow("org-b", "graph-extraction", "b2"),
	)
	result, err := rig.forwarder.RunOnce(context.Background())
	if err != nil || result != (PassResult{Claimed: 4, Forwarded: 4}) {
		t.Fatalf("pass = %+v, err = %v", result, err)
	}
	entries := rig.cost.recorded()
	if len(entries) != 4 {
		t.Fatalf("cost-core recorded %d entries, want 4", len(entries))
	}
	for _, entry := range entries {
		if !strings.HasPrefix(entry.IdempotencyKey, "dataplane:"+entry.OrgID+":") {
			t.Errorf("key %q does not embed its organization %q", entry.IdempotencyKey, entry.OrgID)
		}
		if entry.ProducerID != "service:data-orchestrator" {
			t.Errorf("producer %q, want the principal", entry.ProducerID)
		}
		if entry.Feature == "" || entry.CostBasis != "metered" || entry.InputTokens != 1200 || entry.OutputTokens != 60 || entry.CacheReadTokens != 512 {
			t.Errorf("entry %+v lost a field on the way", entry)
		}
	}
	// One bearer per organization, not per row.
	if rig.auth.mintCount("org-a") != 1 || rig.auth.mintCount("org-b") != 1 {
		t.Fatalf("mints: org-a=%d org-b=%d, want 1 each", rig.auth.mintCount("org-a"), rig.auth.mintCount("org-b"))
	}
	// The service credential goes to Auth Core and to nobody else.
	for _, header := range rig.cost.auths {
		if header != "" {
			t.Fatal("the service credential was sent to cost-core")
		}
	}
}

// Never twice after success: a forwarded row is not claimed again, and a second pass posts
// nothing.
func TestAForwardedRowIsNeverPostedAgain(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"}, usageRow("org-a", "contextual-retrieval", "a1"))
	for i := 0; i < 3; i++ {
		if _, err := rig.forwarder.RunOnce(context.Background()); err != nil {
			t.Fatalf("pass %d: %v", i+1, err)
		}
	}
	if rig.cost.requestCount() != 1 || len(rig.cost.recorded()) != 1 {
		t.Fatalf("cost-core saw %d requests and recorded %d rows, want 1 and 1", rig.cost.requestCount(), len(rig.cost.recorded()))
	}
}

// The window the design must survive: cost-core accepted the row and the bookkeeping failed.
// The row is posted again after its lease and cost-core records it once.
func TestAnAcceptedRowWhoseBookkeepingFailedIsPostedAgainAndRecordedOnce(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"}, usageRow("org-a", "contextual-retrieval", "a1"))
	rig.queue.failMark = 1
	first, _ := rig.forwarder.RunOnce(context.Background())
	if first.Forwarded != 0 || first.Failed != 1 {
		t.Fatalf("first pass = %+v", first)
	}
	// Still leased: another replica does not post it meanwhile.
	if again, _ := rig.forwarder.RunOnce(context.Background()); again.Claimed != 0 {
		t.Fatalf("a leased row was claimed again: %+v", again)
	}
	rig.clock.advance(3 * time.Minute)
	second, _ := rig.forwarder.RunOnce(context.Background())
	if second.Forwarded != 1 {
		t.Fatalf("second pass = %+v", second)
	}
	if rig.cost.requestCount() != 2 || len(rig.cost.recorded()) != 1 {
		t.Fatalf("cost-core saw %d requests and recorded %d rows, want 2 and 1: the second post must be a duplicate it ignores", rig.cost.requestCount(), len(rig.cost.recorded()))
	}
}

// The key is global at cost-core. Two organizations that produce the same producer key do not
// collide, because the organization is in what is posted.
func TestTheSameProducerKeyOfTwoOrganizationsIsTwoRows(t *testing.T) {
	a, b := usageRow("org-a", "contextual-retrieval", "same"), usageRow("org-b", "contextual-retrieval", "same")
	rig := newForwardRig(t, []string{"org-a", "org-b"}, a, b)
	if _, err := rig.forwarder.RunOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	if got := len(rig.cost.recorded()); got != 2 {
		t.Fatalf("cost-core recorded %d rows for two organizations with the same producer key, want 2", got)
	}
}

func TestTransientFailuresAreRetriedWithGrowingPausesThenForwarded(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"}, usageRow("org-a", "contextual-retrieval", "a1"))
	rig.cost.answerFirst(http.StatusInternalServerError, http.StatusTooManyRequests, http.StatusBadGateway)
	id := int64(1)
	var pauses []time.Duration
	for pass := 1; pass <= 3; pass++ {
		result, err := rig.forwarder.RunOnce(context.Background())
		if err != nil || result.Failed != 1 {
			t.Fatalf("pass %d = %+v, err = %v", pass, result, err)
		}
		state := rig.queue.state(id)
		if state.class != "unavailable" || state.row.Attempts != pass {
			t.Fatalf("pass %d: class=%q attempts=%d", pass, state.class, state.row.Attempts)
		}
		pause := state.nextAt.Sub(rig.clock.now())
		pauses = append(pauses, pause)
		// Not due yet: nothing is posted before its time.
		before := rig.cost.requestCount()
		if early, _ := rig.forwarder.RunOnce(context.Background()); early.Claimed != 0 || rig.cost.requestCount() != before {
			t.Fatalf("a row was posted before its pause had passed")
		}
		rig.clock.advance(pause + time.Second)
	}
	if !(pauses[0] == 30*time.Second && pauses[1] == time.Minute && pauses[2] == 2*time.Minute) {
		t.Fatalf("pauses = %v, want 30s, 1m, 2m", pauses)
	}
	result, _ := rig.forwarder.RunOnce(context.Background())
	if result.Forwarded != 1 || len(rig.cost.recorded()) != 1 {
		t.Fatalf("final pass = %+v, recorded %d", result, len(rig.cost.recorded()))
	}
}

func TestARowCostCoreRejectsBacksOffFurtherAndNeverBlocksTheOthers(t *testing.T) {
	good := usageRow("org-a", "contextual-retrieval", "good")
	bad := usageRow("org-a", "contextual-retrieval", "bad")
	bad.Feature = "Not A Feature" // cost-core's charset: a 400
	rig := newForwardRig(t, []string{"org-a"}, bad, good)
	result, err := rig.forwarder.RunOnce(context.Background())
	if err != nil || result.Forwarded != 1 || result.Failed != 1 {
		t.Fatalf("pass = %+v, err = %v", result, err)
	}
	state := rig.queue.state(1)
	if state.class != "rejected" {
		t.Fatalf("class = %q, want rejected", state.class)
	}
	if pause := state.nextAt.Sub(rig.clock.now()); pause != 5*time.Minute {
		t.Fatalf("a rejected row waits %v, want five minutes: retrying the same body changes nothing", pause)
	}
	if !rig.queue.state(2).forwarded {
		t.Fatal("a rejected row stopped the one behind it")
	}
}

func TestARefusedBearerIsDroppedAndTheOrganizationsOtherRowsWaitForAFreshOne(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a", "org-b"},
		usageRow("org-a", "contextual-retrieval", "a1"),
		usageRow("org-a", "contextual-retrieval", "a2"),
		usageRow("org-b", "contextual-retrieval", "b1"),
	)
	rig.cost.answerFirst(http.StatusUnauthorized)
	result, _ := rig.forwarder.RunOnce(context.Background())
	if result.Forwarded != 1 || result.Failed != 2 {
		t.Fatalf("pass = %+v: org-b is unaffected, both of org-a's rows wait", result)
	}
	if rig.cost.requestCount() != 2 {
		t.Fatalf("cost-core saw %d requests, want 2: the second row of a refused organization is not posted", rig.cost.requestCount())
	}
	for _, id := range []int64{1, 2} {
		if class := rig.queue.state(id).class; class != "unauthorized" {
			t.Fatalf("row %d class = %q", id, class)
		}
	}
	rig.clock.advance(time.Minute)
	result, _ = rig.forwarder.RunOnce(context.Background())
	if result.Forwarded != 2 || rig.auth.mintCount("org-a") != 2 {
		t.Fatalf("after the pause: %+v with %d mints of org-a, want both rows and a fresh bearer", result, rig.auth.mintCount("org-a"))
	}
}

func TestAnOrganizationWhoseBearerCannotBeMintedFailsAloneAndIsRetried(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"}, // org-b is not in the directory: Auth Core refuses to mint for it
		usageRow("org-a", "contextual-retrieval", "a1"),
		usageRow("org-b", "contextual-retrieval", "b1"),
	)
	result, err := rig.forwarder.RunOnce(context.Background())
	if err != nil || result.Forwarded != 1 || result.Failed != 1 {
		t.Fatalf("pass = %+v, err = %v", result, err)
	}
	if state := rig.queue.state(2); state.class != "token_unavailable" || state.forwarded {
		t.Fatalf("org-b's row: %+v", state)
	}
	if len(rig.cost.recorded()) != 1 || rig.cost.recorded()[0].OrgID != "org-a" {
		t.Fatal("a row went to cost-core without a bearer for its organization")
	}
}

func TestAPeriodWhenAuthCoreIsDownDoesNotLoseRows(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"}, usageRow("org-a", "contextual-retrieval", "a1"))
	rig.auth.status = http.StatusServiceUnavailable
	if result, _ := rig.forwarder.RunOnce(context.Background()); result.Failed != 1 || rig.cost.requestCount() != 0 {
		t.Fatalf("with Auth Core down: %+v, cost-core saw %d requests", result, rig.cost.requestCount())
	}
	rig.auth.mu.Lock()
	rig.auth.status = 0
	rig.auth.mu.Unlock()
	rig.clock.advance(time.Minute)
	if result, _ := rig.forwarder.RunOnce(context.Background()); result.Forwarded != 1 {
		t.Fatalf("after it came back: %+v", result)
	}
}

// A zero-retention organization's usage is forwarded like any other: it holds counters only.
func TestZeroRetentionUsageIsForwardedToo(t *testing.T) {
	rig := newForwardRig(t, []string{"org-zdr"}, usageRow("org-zdr", "graph-extraction", "z1"))
	if result, _ := rig.forwarder.RunOnce(context.Background()); result.Forwarded != 1 {
		t.Fatalf("pass = %+v", result)
	}
	if entries := rig.cost.recorded(); len(entries) != 1 || entries[0].OrgID != "org-zdr" || entries[0].Feature != "graph-extraction" {
		t.Fatalf("entries = %+v", entries)
	}
}

// The stand-in is strict like cost-core: a body with a field cost-core does not name is a 400,
// a bearer for another organization is a 403, and a bearer without cost:write is a 403. This
// is what proves the tests above exercise the real rules and not a permissive double.
func TestTheCostCoreStandInAppliesCostCoresRules(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a", "org-b"})
	bearerA, err := rig.tokens.Token(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("token: %v", err)
	}
	body := func(extra string) string {
		return fmt.Sprintf(`{"org_id":"org-a","model":"m","provider_used":"p","input_tokens":1,"output_tokens":1,"idempotency_key":"dataplane:org-a:k1","feature":"embedding"%s}`, extra)
	}
	post := func(bearer, payload string) int {
		request, _ := http.NewRequest(http.MethodPost, rig.cost.server.URL+recordPath, strings.NewReader(payload))
		request.Header.Set("Authorization", "Bearer "+bearer)
		response, err := http.DefaultClient.Do(request)
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		return response.StatusCode
	}
	if got := post(bearerA, body("")); got != http.StatusAccepted {
		t.Fatalf("a valid body: %d", got)
	}
	if got := post(bearerA, body("")); got != http.StatusAccepted || len(rig.cost.recorded()) != 1 {
		t.Fatalf("a replay: %d, recorded %d, want 202 and still one row", got, len(rig.cost.recorded()))
	}
	for name, payload := range map[string]string{
		"unknown field":     body(`,"document_id":"doc-1"`),
		"two objects":       body("") + body(""),
		"bad basis":         body(`,"cost_basis":"estimated"`),
		"bad feature":       body(`,"feature":"x"`)[:0] + strings.Replace(body(""), `"feature":"embedding"`, `"feature":"Embedding"`, 1),
		"negative tokens":   strings.Replace(body(""), `"input_tokens":1`, `"input_tokens":-1`, 1),
		"unterminated JSON": `{"org_id":`,
	} {
		if got := post(bearerA, payload); got != http.StatusBadRequest {
			t.Errorf("%s: %d, want 400", name, got)
		}
	}
	if got := post(bearerA, strings.Replace(body(""), `"org_id":"org-a"`, `"org_id":"org-b"`, 1)); got != http.StatusForbidden {
		t.Errorf("a body for another organization: %d, want 403", got)
	}
	if got := post("", body("")); got != http.StatusUnauthorized {
		t.Errorf("no bearer: %d, want 401", got)
	}
	if got := post(bearerA+"x", body("")); got != http.StatusUnauthorized {
		t.Errorf("a bearer with a broken signature: %d, want 401", got)
	}
	// A bearer without cost:write.
	claims := jwt.MapClaims{"iss": rig.auth.issuer, "aud": "cost-core", "sub": "service:data-orchestrator", "principal_type": "service",
		"org_id": "org-a", "scopes": []string{"cost:read"}, "exp": time.Now().Add(time.Minute).Unix(), "iat": time.Now().Unix()}
	readOnly, _ := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(rigKey)
	if got := post(readOnly, body("")); got != http.StatusForbidden {
		t.Errorf("a bearer with cost:read only: %d, want 403", got)
	}
	// And a user principal.
	claims["principal_type"], claims["scopes"] = "user", []string{"cost:write"}
	user, _ := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(rigKey)
	if got := post(user, body("")); got != http.StatusForbidden {
		t.Errorf("a user principal: %d, want 403", got)
	}
}

// The claim reads the small partial index of the migration and not the ledger: its predicate
// must be one the index's predicate implies. Postgres can only use a partial index when the
// query repeats its conditions, so this pins that the claim names every condition of the index,
// reading both from the files, with no database.
func TestTheClaimRepeatsTheConditionsOfTheForwardQueueIndex(t *testing.T) {
	_, source, _, _ := runtime.Caller(0)
	raw, err := os.ReadFile(filepath.Join(filepath.Dir(source), "..", "..", "..", "..", "infra", "postgres", "migrations", costEventsMigrationVersion))
	if err != nil {
		t.Fatalf("read the migration: %v", err)
	}
	sql := strings.ReplaceAll(string(raw), "\r\n", "\n")
	start := strings.Index(sql, "CREATE INDEX IF NOT EXISTS idx_cost_events_forward_queue")
	if start < 0 {
		t.Fatal("the migration has no idx_cost_events_forward_queue")
	}
	definition := sql[start : start+strings.Index(sql[start:], ";")]
	_, predicate, found := strings.Cut(definition, "WHERE ")
	if !found {
		t.Fatal("the forward queue index is not a partial index")
	}
	for _, condition := range strings.Split(predicate, " AND ") {
		condition = strings.TrimSpace(condition)
		if !strings.Contains(claimUsageRowsSQL, condition) {
			t.Errorf("the claim does not repeat the index condition %q: Postgres could not use the index", condition)
		}
	}
	for _, statement := range []string{markForwardedSQL, markFailedSQL} {
		if !strings.Contains(statement, "forwarded_at IS NULL") {
			t.Errorf("a bookkeeping statement can touch a row that is already forwarded: %s", strings.TrimSpace(statement))
		}
	}
}

func TestRetryDelayLadder(t *testing.T) {
	cases := []struct {
		class    string
		attempts int
		want     time.Duration
	}{
		{"unavailable", 0, 30 * time.Second}, {"unavailable", 1, time.Minute}, {"unavailable", 5, 16 * time.Minute},
		{"unavailable", 20, time.Hour}, {"token_unavailable", 40, time.Hour},
		{"rejected", 0, 5 * time.Minute}, {"rejected", 3, 40 * time.Minute}, {"rejected", 20, 6 * time.Hour},
	}
	for _, c := range cases {
		if got := retryDelay(c.class, c.attempts); got != c.want {
			t.Errorf("retryDelay(%s, %d) = %v, want %v", c.class, c.attempts, got, c.want)
		}
	}
}

func TestClassifyMapsCostCoresAnswers(t *testing.T) {
	want := map[int]string{
		200: "", 201: "", 202: "", 204: "",
		400: "rejected", 404: "rejected", 405: "rejected", 413: "rejected", 422: "rejected",
		401: "unauthorized", 403: "unauthorized",
		408: "unavailable", 429: "unavailable", 500: "unavailable", 502: "unavailable", 503: "unavailable",
	}
	var statuses []int
	for status := range want {
		statuses = append(statuses, status)
	}
	sort.Ints(statuses)
	for _, status := range statuses {
		if got := classify(status); got != want[status] {
			t.Errorf("classify(%d) = %q, want %q", status, got, want[status])
		}
	}
}

// Shutdown mid-pass is not a failure of the rows left: they keep their lease and are claimed
// again, not marked failed.
func TestShuttingDownMidPassLeavesTheRestUntouched(t *testing.T) {
	rig := newForwardRig(t, []string{"org-a"},
		usageRow("org-a", "contextual-retrieval", "a1"),
		usageRow("org-a", "contextual-retrieval", "a2"),
	)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := rig.forwarder.RunOnce(ctx); err == nil {
		t.Fatal("a cancelled pass reported success")
	}
	for _, id := range []int64{1, 2} {
		if state := rig.queue.state(id); state.class != "" || state.row.Attempts != 0 || state.forwarded {
			t.Fatalf("row %d was touched by a cancelled pass: %+v", id, state)
		}
	}
}

func TestForwarderRefusesToStartWithoutWhatItNeeds(t *testing.T) {
	if _, err := NewForwarder(nil, nil, nil, ForwarderConfig{BaseURL: "http://cost-core"}); err == nil {
		t.Fatal("a forwarder without a store was built")
	}
	if _, err := NewForwarder(newMemoryQueue(time.Now), &AuthCoreTokens{}, nil, ForwarderConfig{}); err == nil {
		t.Fatal("a forwarder without cost-core's address was built")
	}
}
