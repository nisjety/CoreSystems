package resources

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const tombstonePath = "/v1/internal/zdr-tombstones"

// newTombstoneServer builds the route tree as cmd/control/main.go does: the HMAC
// group, then MountZdrTombstones. require=false is the rollout mode in which the
// verifier lets an unsigned request through.
func newTombstoneServer(t *testing.T, require bool) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	db.ZdrTombstones().SetErasureMarkers(true) // what cmd/control does with the receiver flag
	verifier := httpx.NewHMACVerifier(testSecret, require)
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	r.Group(func(r chi.Router) {
		r.Use(verifier.Middleware)
		MountZdrTombstones(r, db)
	})
	return r, db
}

func signedTombstonePost(body []byte, nonce string) *http.Request {
	return signedRequest(http.MethodPost, tombstonePath, body, nonce)
}

// signedRequest signs method + target (path with query) + body the way the edge does
// (the canonical string the verifier rebuilds in httpx/hmac.go).
func signedRequest(method, target string, body []byte, nonce string) *http.Request {
	ts := time.Now().Unix()
	bodyHash := sha256.Sum256(body)
	canonical := method + "\n" + target + "\n" + hex.EncodeToString(bodyHash[:]) +
		"\n" + strconv.FormatInt(ts, 10) + "\n" + nonce
	mac := hmac.New(sha256.New, []byte(testSecret))
	_, _ = mac.Write([]byte(canonical))
	req := httptest.NewRequest(method, target, bytes.NewReader(body))
	req.Header.Set(httpx.HMACHeaderSig, "sig_v1="+base64.StdEncoding.EncodeToString(mac.Sum(nil)))
	req.Header.Set(httpx.HMACHeaderTS, strconv.FormatInt(ts, 10))
	req.Header.Set(httpx.HMACHeaderNonce, nonce)
	return req
}

var nonceCounter int

func nextNonce() string {
	nonceCounter++
	return "00000000000000000000000000" + strconv.Itoa(100000+nonceCounter)
}

// tombstoneBody is a valid body; mutate edits the decoded map before it is encoded.
func tombstoneBody(t *testing.T, mutate func(map[string]any)) []byte {
	t.Helper()
	m := map[string]any{
		"run_id":          "run_3f9c1",
		"org_id":          "org_a",
		"kind":            "scrape",
		"retention_class": "zdr",
		"outcome":         "ok",
		"driver":          "static",
		"started_at":      "2026-10-21T10:00:00Z",
		"ended_at":        "2026-10-21T10:00:02Z",
		"step_count":      3,
		"cost_units":      0.25,
	}
	if mutate != nil {
		mutate(m)
	}
	b, err := json.Marshal(m)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func serve(h http.Handler, req *http.Request) *httptest.ResponseRecorder {
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func countTombstones(t *testing.T, db store.DB, org string) int {
	t.Helper()
	rows, err := db.ZdrTombstones().ListByOrg(org, 100)
	if err != nil {
		t.Fatal(err)
	}
	return len(rows)
}

func TestZdrTombstoneStoresAValidSignedTombstone(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	rec := serve(h, signedTombstonePost(tombstoneBody(t, nil), nextNonce()))
	if rec.Code != http.StatusNoContent {
		t.Fatalf("status %d body %s", rec.Code, rec.Body.String())
	}
	rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10)
	if len(rows) != 1 || rows[0].RunID != "run_3f9c1" || rows[0].Kind != "scrape" ||
		rows[0].Driver == nil || *rows[0].Driver != "static" ||
		rows[0].StepCount == nil || *rows[0].StepCount != 3 {
		t.Fatalf("stored row does not match the body: %+v", rows)
	}
}

// The plan's TestZdrTombstoneRejectsAnyFieldOutsideTheAllowlist.
func TestZdrTombstoneRejectsAnyFieldOutsideTheAllowlist(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	for _, key := range []string{
		"url", "source_url", "title", "query", "q", "user_id", "text", "markdown", "error",
		"error_text", "digest", "fingerprint", "host", "zdr", "arbitrary_extra", "RUN_ID2",
	} {
		body := tombstoneBody(t, func(m map[string]any) { m[key] = "x" })
		rec := serve(h, signedTombstonePost(body, nextNonce()))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("an extra field %q must be a 400, got %d", key, rec.Code)
		}
	}
	if n := countTombstones(t, db, "org_a"); n != 0 {
		t.Fatalf("a refused body stored %d rows", n)
	}
}

// The plan's TestZdrTombstoneRejectsUrlLookingValues.
func TestZdrTombstoneRejectsUrlLookingValues(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	for name, mutate := range map[string]func(map[string]any){
		"in run_id":      func(m map[string]any) { m["run_id"] = "https://example.com/p" },
		"in org_id":      func(m map[string]any) { m["org_id"] = "ftp://org" },
		"in driver":      func(m map[string]any) { m["driver"] = "http://h" },
		"in error_class": func(m map[string]any) { m["error_class"] = "TIMEOUT://" },
		"in a new key":   func(m map[string]any) { m["a://b"] = 1 },
		"in a number's sibling object": func(m map[string]any) {
			m["note"] = map[string]any{"deep": []any{"see https://x.example"}}
		},
	} {
		rec := serve(h, signedTombstonePost(tombstoneBody(t, mutate), nextNonce()))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: want 400, got %d", name, rec.Code)
		}
		// The URL guard answers before any other check, with its own message, and
		// the answer never echoes the value.
		if !strings.Contains(rec.Body.String(), "looks like a URL") {
			t.Fatalf("%s: the URL guard did not answer: %s", name, rec.Body.String())
		}
		if strings.Contains(rec.Body.String(), "example") || strings.Contains(rec.Body.String(), "ftp") {
			t.Fatalf("%s: the refusal echoes the value: %s", name, rec.Body.String())
		}
	}
	// An escaped colon and slashes are decoded and refused too.
	escaped := bytes.Replace(tombstoneBody(t, nil), []byte(`"run_3f9c1"`), []byte(`"a\u003a\/\/b"`), 1)
	rec := serve(h, signedTombstonePost(escaped, nextNonce()))
	if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), "looks like a URL") {
		t.Fatalf("an escaped scheme separator must be refused by the URL guard: %d %s", rec.Code, rec.Body.String())
	}
	if n := countTombstones(t, db, "org_a"); n != 0 {
		t.Fatalf("a refused body stored %d rows", n)
	}
}

// The plan's TestZdrTombstoneIsIdempotentOnRunID.
func TestZdrTombstoneIsIdempotentOnRunID(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	first := serve(h, signedTombstonePost(tombstoneBody(t, nil), nextNonce()))
	second := serve(h, signedTombstonePost(tombstoneBody(t, func(m map[string]any) {
		m["outcome"] = "error"
		m["error_class"] = "TIMEOUT"
	}), nextNonce()))
	if first.Code != http.StatusNoContent || second.Code != http.StatusNoContent {
		t.Fatalf("both writes must answer 204, got %d and %d", first.Code, second.Code)
	}
	if first.Body.String() != second.Body.String() {
		t.Fatal("an insert and a duplicate must be indistinguishable (no existence oracle)")
	}
	rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10)
	if len(rows) != 1 || rows[0].Outcome != "ok" {
		t.Fatalf("first write must win and the duplicate must add nothing: %+v", rows)
	}
}

func TestZdrTombstoneRefusesValuesOutsideTheVocabulary(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	for name, mutate := range map[string]func(map[string]any){
		"kind":              func(m map[string]any) { m["kind"] = "crawl" },
		"retention_class":   func(m map[string]any) { m["retention_class"] = "standard" },
		"outcome":           func(m map[string]any) { m["outcome"] = "fine" },
		"driver":            func(m map[string]any) { m["driver"] = "puppeteer" },
		"error_class text":  func(m map[string]any) { m["error_class"] = "dns failure for the-site" },
		"run_id chars":      func(m map[string]any) { m["run_id"] = "run 1" },
		"ended_at":          func(m map[string]any) { m["ended_at"] = "yesterday" },
		"negative steps":    func(m map[string]any) { m["step_count"] = -1 },
		"negative cost":     func(m map[string]any) { m["cost_units"] = -1 },
		"started after end": func(m map[string]any) { m["started_at"] = "2026-10-21T11:00:00Z" },
		"missing org":       func(m map[string]any) { delete(m, "org_id") },
		"missing ended_at":  func(m map[string]any) { delete(m, "ended_at") },
	} {
		rec := serve(h, signedTombstonePost(tombstoneBody(t, mutate), nextNonce()))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: want 400, got %d %s", name, rec.Code, rec.Body.String())
		}
	}
	if n := countTombstones(t, db, "org_a"); n != 0 {
		t.Fatalf("a refused body stored %d rows", n)
	}
	// Trailing data after the object is refused too.
	rec := serve(h, signedTombstonePost(append(tombstoneBody(t, nil), []byte(`{"x":1}`)...), nextNonce()))
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("trailing data: want 400, got %d", rec.Code)
	}
}

func TestZdrTombstoneBodyOverTheCapIsRefused(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	// A body that is valid JSON for the allowlist and only too long: without the cap it would be stored.
	body := append(tombstoneBody(t, nil), bytes.Repeat([]byte(" "), maxZdrTombstoneBytes)...)
	rec := serve(h, signedTombstonePost(body, nextNonce()))
	if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), "too large") {
		t.Fatalf("an over-cap body must be refused as too large, got %d %s", rec.Code, rec.Body.String())
	}
	if n := countTombstones(t, db, "org_a"); n != 0 {
		t.Fatalf("an over-cap body stored %d rows", n)
	}
}

// The plan's unsigned case: enforced mode refuses in the verifier; rollout mode
// lets the request through the verifier, so the handler's own check refuses it.
func TestUnsignedTombstoneIs401(t *testing.T) {
	for name, require := range map[string]bool{"enforced": true, "rollout": false} {
		h, db := newTombstoneServer(t, require)
		req := httptest.NewRequest(http.MethodPost, tombstonePath, bytes.NewReader(tombstoneBody(t, nil)))
		rec := serve(h, req)
		if rec.Code != http.StatusUnauthorized {
			t.Fatalf("%s: an unsigned tombstone must be a 401, got %d", name, rec.Code)
		}
		if n := countTombstones(t, db, "org_a"); n != 0 {
			t.Fatalf("%s: an unsigned tombstone stored %d rows", name, n)
		}
	}
}

func TestZdrTombstoneForgedSignatureIs401AndReplayIsRefused(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	req := signedTombstonePost(tombstoneBody(t, nil), nextNonce())
	req.Header.Set(httpx.HMACHeaderSig, "sig_v1="+base64.StdEncoding.EncodeToString([]byte("not the mac")))
	if rec := serve(h, req); rec.Code != http.StatusUnauthorized {
		t.Fatalf("a forged signature must be a 401, got %d", rec.Code)
	}
	nonce := nextNonce()
	if rec := serve(h, signedTombstonePost(tombstoneBody(t, nil), nonce)); rec.Code != http.StatusNoContent {
		t.Fatalf("first signed post: %d", rec.Code)
	}
	if rec := serve(h, signedTombstonePost(tombstoneBody(t, nil), nonce)); rec.Code != http.StatusUnauthorized {
		t.Fatalf("a replayed nonce must be a 401, got %d", rec.Code)
	}
	if n := countTombstones(t, db, "org_a"); n != 1 {
		t.Fatalf("want exactly 1 row, got %d", n)
	}
}

// There is no HTTP read of tombstones. The requests below are SIGNED, so they pass
// the verifier and reach the router: a GET handler added to the group would answer
// 2xx here (an unsigned GET is refused by the verifier first and would prove
// nothing). A second check reads the route table itself.
func TestZdrTombstoneHasNoReadRoute(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	if _, err := db.ZdrTombstones().Put(store.ZdrTombstone{
		RunID: "run_3f9c1", OrgID: "org_a", Kind: "scrape", RetentionClass: store.ZdrTombstoneRetentionClass,
		Outcome: "ok", EndedAt: time.Now().UTC(),
	}); err != nil {
		t.Fatal(err)
	}
	for _, method := range []string{http.MethodGet, http.MethodHead, http.MethodPut, http.MethodPatch, http.MethodDelete} {
		for _, target := range []string{tombstonePath, tombstonePath + "/run_3f9c1", tombstonePath + "?org_id=org_a"} {
			rec := serve(h, signedRequest(method, target, nil, nextNonce()))
			if rec.Code >= 200 && rec.Code < 300 {
				t.Fatalf("a signed %s %s was served: %d %s", method, target, rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "run_3f9c1") {
				t.Fatalf("a signed %s %s leaked a tombstone: %s", method, target, rec.Body.String())
			}
		}
	}
	// The route table: the only handler under the tombstone path is POST.
	r := chi.NewRouter()
	MountZdrTombstones(r, db)
	var seen []string
	if err := chi.Walk(r, func(method, route string, _ http.Handler, _ ...func(http.Handler) http.Handler) error {
		if strings.Contains(route, "zdr-tombstones") {
			seen = append(seen, method+" "+route)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(seen, []string{"POST " + tombstonePath}) {
		t.Fatalf("the tombstone route table must be exactly POST %s, got %v", tombstonePath, seen)
	}
}

// schemaDoc is the parts of zdr-tombstone.schema.json this test reads.
type schemaDoc struct {
	AdditionalProperties bool                       `json:"additionalProperties"`
	Required             []string                   `json:"required"`
	Properties           map[string]json.RawMessage `json:"properties"`
}

func loadTombstoneSchema(t *testing.T) schemaDoc {
	t.Helper()
	raw, err := os.ReadFile("testdata/zdr-tombstone.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	pin, err := os.ReadFile("testdata/zdr-tombstone.schema.json.sha256")
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(raw)
	if got := hex.EncodeToString(sum[:]); got != strings.Fields(string(pin))[0] {
		t.Fatalf("the vendored schema drifted from its pin: %s", got)
	}
	var doc schemaDoc
	if err := json.Unmarshal(raw, &doc); err != nil {
		t.Fatal(err)
	}
	return doc
}

func enumOf(t *testing.T, raw json.RawMessage) []string {
	t.Helper()
	var e struct {
		Enum []string `json:"enum"`
	}
	if err := json.Unmarshal(raw, &e); err != nil {
		t.Fatal(err)
	}
	return e.Enum
}

func sorted(in []string) []string {
	out := append([]string(nil), in...)
	sort.Strings(out)
	return out
}

// The wire struct names exactly the schema's keys, requires exactly the schema's
// required set, and uses the schema's closed vocabularies. A field added to one and
// not the other fails here.
func TestZdrTombstoneStructMatchesSchema(t *testing.T) {
	doc := loadTombstoneSchema(t)
	if doc.AdditionalProperties {
		t.Fatal("the schema must forbid additional properties")
	}

	var keys, required []string
	rt := reflect.TypeOf(ZdrTombstoneWire{})
	for i := 0; i < rt.NumField(); i++ {
		tag := rt.Field(i).Tag.Get("json")
		name, opts, _ := strings.Cut(tag, ",")
		keys = append(keys, name)
		if !strings.Contains(opts, "omitempty") {
			required = append(required, name)
		}
	}
	var schemaKeys []string
	for k := range doc.Properties {
		schemaKeys = append(schemaKeys, k)
	}
	if !reflect.DeepEqual(sorted(keys), sorted(schemaKeys)) {
		t.Fatalf("struct keys %v differ from schema properties %v", sorted(keys), sorted(schemaKeys))
	}
	if !reflect.DeepEqual(sorted(required), sorted(doc.Required)) {
		t.Fatalf("struct required %v differ from schema required %v", sorted(required), sorted(doc.Required))
	}
	// The key "zdr" is reserved by change_webhook.go's gate and must never be an allowlisted key.
	for _, k := range keys {
		if k == "zdr" {
			t.Fatal(`the tombstone body must not use the key "zdr"`)
		}
	}

	for key, want := range map[string][]string{
		"kind":        store.ZdrTombstoneKinds,
		"outcome":     store.ZdrTombstoneOutcomes,
		"driver":      store.ZdrTombstoneDrivers,
		"error_class": store.ZdrTombstoneErrorClasses,
	} {
		if got := enumOf(t, doc.Properties[key]); !reflect.DeepEqual(sorted(got), sorted(want)) {
			t.Fatalf("%s: schema enum %v differs from the store vocabulary %v", key, sorted(got), sorted(want))
		}
	}
	var rc struct {
		Const string `json:"const"`
	}
	if err := json.Unmarshal(doc.Properties["retention_class"], &rc); err != nil || rc.Const != store.ZdrTombstoneRetentionClass {
		t.Fatalf("retention_class const %q differs from %q", rc.Const, store.ZdrTombstoneRetentionClass)
	}
}

// A signed tombstone for an org that was erased answers 204 like any other and stores
// nothing: the route must not become an oracle for "this org was erased".
func TestZdrTombstoneForAnErasedOrgAnswers204AndStoresNothing(t *testing.T) {
	h, db := newTombstoneServer(t, true)
	if rec := serve(h, signedTombstonePost(tombstoneBody(t, nil), nextNonce())); rec.Code != http.StatusNoContent {
		t.Fatalf("first post: %d", rec.Code)
	}
	if _, err := db.PurgeOrg("org_a"); err != nil {
		t.Fatal(err)
	}
	late := tombstoneBody(t, func(m map[string]any) { m["run_id"] = "run_after_erasure" })
	if rec := serve(h, signedTombstonePost(late, nextNonce())); rec.Code != http.StatusNoContent {
		t.Fatalf("a tombstone for an erased org must still answer 204, got %d", rec.Code)
	}
	if n := countTombstones(t, db, "org_a"); n != 0 {
		t.Fatalf("an erased org has %d tombstones again", n)
	}
}
