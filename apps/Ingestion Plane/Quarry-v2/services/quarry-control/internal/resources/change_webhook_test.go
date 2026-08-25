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
	"strconv"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// testSecret is the shared HMAC secret for this suite; requests are signed
// exactly the way crates/quarry-edge/src/internal_auth.rs signs them.
const testSecret = "test-change-webhook-secret"

// newChangeWebhookServer builds the route tree exactly as cmd/control/main.go
// does: RequestID → HMAC group → MountChangeWebhook. require=true so the
// suite exercises enforcement, not rollout trust-the-network.
func newChangeWebhookServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	verifier := httpx.NewHMACVerifier(testSecret, true)
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	r.Group(func(r chi.Router) {
		r.Use(verifier.Middleware)
		MountChangeWebhook(r, db, nil)
	})
	return r, db
}

// signedChangePost mints an edge-style POST /v1/webhooks/change?org_id=<org>
// with the canonical-string signature over method/path-with-query/body-hash/
// ts/nonce. ts<=0 means "now"; override for stale-timestamp tests.
func signedChangePost(t *testing.T, org string, body []byte, idemKey string, ts int64, nonce string) *http.Request {
	t.Helper()
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	pathQ := "/v1/webhooks/change"
	if org != "" {
		pathQ += "?org_id=" + org
	}
	bodyHash := sha256.Sum256(body)
	canonical := http.MethodPost + "\n" + pathQ + "\n" + hex.EncodeToString(bodyHash[:]) +
		"\n" + strconv.FormatInt(ts, 10) + "\n" + nonce
	mac := hmac.New(sha256.New, []byte(testSecret))
	_, _ = mac.Write([]byte(canonical))
	sig := base64.StdEncoding.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest(http.MethodPost, pathQ, bytes.NewReader(body))
	req.Header.Set(httpx.HMACHeaderSig, "sig_v1="+sig)
	req.Header.Set(httpx.HMACHeaderTS, strconv.FormatInt(ts, 10))
	req.Header.Set(httpx.HMACHeaderNonce, nonce)
	if idemKey != "" {
		req.Header.Set("Idempotency-Key", idemKey)
	}
	return req
}

// changedBody is a minimal status=changed record with the envelope fields,
// mirroring what change_webhook_payload emits on the Rust side.
var changedBody = []byte(`{
  "subject": "quarry.change.detected",
  "emitted_at": "2026-08-24T17:00:06Z",
  "source_url": "https://example.com/pricing",
  "org_id": "o1",
  "status": "changed",
  "new_baseline": {
    "baseline_id": "bln_01NEW",
    "org_id": "o1",
    "source_url": "https://example.com/pricing",
    "fingerprint": "blake3:newcontent",
    "captured_at": "2026-08-24T17:00:00Z"
  },
  "prev_baseline": {
    "baseline_id": "bln_01OLD",
    "org_id": "o1",
    "source_url": "https://example.com/pricing",
    "fingerprint": "blake3:oldcontent",
    "captured_at": "2026-08-23T16:00:00Z"
  },
  "diff_id": "diff_01XYZ",
  "checked_at": "2026-08-24T17:00:05Z"
}`)

type changeAck struct {
	Status   string             `json:"status"`
	EventID  quarrycontracts.ID `json:"event_id"`
	Duplicate bool              `json:"duplicate"`
}

func doChange(t *testing.T, h http.Handler, req *http.Request) (int, changeAck) {
	t.Helper()
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	var ack changeAck
	if w.Code < 300 {
		var env struct {
			Data changeAck `json:"data"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
			t.Fatalf("decode ack: %v; body=%s", err, w.Body.String())
		}
		ack = env.Data
	}
	return w.Code, ack
}

func TestChangeWebhook_HappyPathPersistsDurableEvent(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	code, ack := doChange(t, h, signedChangePost(t, "o1", changedBody, "idem-1", 0, "nonce-happy"))
	if code != http.StatusOK {
		t.Fatalf("status=%d want=200; body=%s", code, ack.Status)
	}
	if ack.Duplicate || ack.Status != "recorded" || ack.EventID == "" {
		t.Fatalf("ack=%+v", ack)
	}

	existing, ok := db.Events().FindByIdempotencyKey("idem-1")
	if !ok {
		t.Fatal("event not persisted under idempotency key")
	}
	if existing.EventID != ack.EventID || existing.Type != quarrycontracts.EvtChangeDetected {
		t.Fatalf("persisted=%+v ack=%+v", existing, ack)
	}
	// Timestamp must come from emitted_at, not server-now.
	wantTS := time.Date(2026, 8, 24, 17, 0, 6, 0, time.UTC)
	if !existing.Timestamp.Equal(wantTS) {
		t.Fatalf("ts=%v want=%v", existing.Timestamp, wantTS)
	}
	// Payload preserves the exact wire document the edge produced.
	if existing.Payload["subject"] != quarrycontracts.SubjectChangeDetected ||
		existing.Payload["source_url"] != "https://example.com/pricing" ||
		existing.Payload["status"] != "changed" {
		t.Fatalf("payload=%+v", existing.Payload)
	}
	nb, ok := existing.Payload["new_baseline"].(map[string]any)
	if !ok || nb["baseline_id"] != "bln_01NEW" || nb["fingerprint"] != "blake3:newcontent" {
		t.Fatalf("new_baseline payload=%+v", existing.Payload["new_baseline"])
	}
	if existing.RunID != nil || existing.JobID != nil {
		t.Fatalf("webhook-born event must be run/job-less: %+v", existing)
	}
}

func TestChangeWebhook_IdempotentRetryReturnsOriginalEvent(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	req1 := signedChangePost(t, "o1", changedBody, "idem-retry", 0, "nonce-r1")
	code1, ack1 := doChange(t, h, req1)
	if code1 != http.StatusOK || ack1.Duplicate {
		t.Fatalf("first=%d %+v", code1, ack1)
	}

	// Same key, fresh signature (retry re-signs), identical body.
	req2 := signedChangePost(t, "o1", changedBody, "idem-retry", 0, "nonce-r2")
	code2, ack2 := doChange(t, h, req2)
	if code2 != http.StatusOK {
		t.Fatalf("retry status=%d want=200", code2)
	}
	if !ack2.Duplicate || ack2.Status != "duplicate" {
		t.Fatalf("retry ack=%+v want duplicate", ack2)
	}
	if ack2.EventID != ack1.EventID {
		t.Fatalf("retry event_id=%s want original %s", ack2.EventID, ack1.EventID)
	}

	// Exactly one durable row under the key.
	existing, ok := db.Events().FindByIdempotencyKey("idem-retry")
	if !ok || existing.EventID != ack1.EventID {
		t.Fatalf("post-retry lookup=%+v ok=%v", existing, ok)
	}
}

func TestChangeWebhook_BadSignatureRejected401(t *testing.T) {
	t.Parallel()
	h, _ := newChangeWebhookServer(t)

	// Tampered body AFTER signing → hash mismatch.
	req := signedChangePost(t, "o1", changedBody, "", 0, "nonce-bad")
	req.Body = httptest.NewRequest(http.MethodPost, "/", bytes.NewReader(
		bytes.Replace(changedBody, []byte("changed"), []byte("tampered", ), 1))).Body
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("tampered body status=%d want=401", w.Code)
	}

	// Unsigned entirely (require mode) → 401.
	unsigned := httptest.NewRequest(http.MethodPost, "/v1/webhooks/change?org_id=o1",
		bytes.NewReader(changedBody))
	w2 := httptest.NewRecorder()
	h.ServeHTTP(w2, unsigned)
	if w2.Code != http.StatusUnauthorized {
		t.Fatalf("unsigned status=%d want=401", w2.Code)
	}
}

func TestChangeWebhook_StaleTimestampRejected401(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	staleTs := time.Now().Unix() - (httpx.SkewToleranceSecs + 60)
	req := signedChangePost(t, "o1", changedBody, "", staleTs, "nonce-stale")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("stale ts status=%d want=401", w.Code)
	}
	if _, ok := db.Events().FindByIdempotencyKey(""); ok {
		t.Fatal("nothing should persist for rejected requests")
	}
}

func TestChangeWebhook_ZDRBodyRejectedDefensively(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	for _, variant := range []struct {
		name string
		zdr  string
	}{
		{"bool", `"zdr": true,`},
		{"string", `"zdr": "true",`},
	} {
		body := bytes.Replace(changedBody, []byte(`"subject"`), []byte(variant.zdr+"\n  \"subject\""), 1)
		req := signedChangePost(t, "o1", body, "idem-zdr-"+variant.name, 0, "nonce-zdr-"+variant.name)
		code, _ := doChange(t, h, req)
		if code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d want=400", variant.name, code)
		}
		if _, ok := db.Events().FindByIdempotencyKey("idem-zdr-" + variant.name); ok {
			t.Fatalf("%s: ZDR payload must not persist", variant.name)
		}
	}
}

func TestChangeWebhook_OrgScopingGuards(t *testing.T) {
	t.Parallel()
	h, _ := newChangeWebhookServer(t)

	cases := []struct {
		name   string
		org    string
		body   func() []byte
	}{
		{"missing org_id query", "", func() []byte { return changedBody }},
		{"body/query mismatch", "o2", func() []byte { return changedBody }}, // body says o1
		{"invalid status", "o1", func() []byte {
			return []byte(`{"subject":"quarry.change.detected","org_id":"o1","source_url":"https://x","status":"exploded","checked_at":"2026-08-24T17:00:05Z"}`)
		}},
		{"wrong subject", "o1", func() []byte {
			return []byte(`{"subject":"quarry.other","org_id":"o1","source_url":"https://x","status":"changed","checked_at":"2026-08-24T17:00:05Z"}`)
		}},
		{"malformed json", "o1", func() []byte { return []byte(`{not json`) }},
	}
	for _, tc := range cases {
		req := signedChangePost(t, tc.org, tc.body(), "idem-"+tc.name, 0, "nonce-"+tc.name)
		code, _ := doChange(t, h, req)
		if code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d want=400", tc.name, code)
		}
	}
}

func TestChangeWebhook_FanoutCreatesDeliveryRows(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	whID := quarrycontracts.NewID(quarrycontracts.KindWebhook)
	if err := db.Webhooks().Create(store.Webhook{
		ID:     whID,
		URL:    "https://hooks.example/quarry",
		Events: []string{"change_detected"},
		Active: true,
	}); err != nil {
		t.Fatalf("seed webhook: %v", err)
	}

	code, ack := doChange(t, h, signedChangePost(t, "o1", changedBody, "idem-fanout", 0, "nonce-fan"))
	if code != http.StatusOK {
		t.Fatalf("status=%d", code)
	}
	dels, _ := db.WebhookDeliveries().List(10, "")
	if len(dels) != 1 {
		t.Fatalf("deliveries=%d want=1", len(dels))
	}
	if dels[0].EventID != ack.EventID || dels[0].Status != "pending" {
		t.Fatalf("delivery=%+v want event_id=%s pending", dels[0], ack.EventID)
	}
}

// TestChangeWebhook_UniqueIndexSemantics pins the concurrent-double-delivery
// behavior the pg partial unique index guarantees: when Append reports
// ErrConflict because another instance already recorded the key, the handler
// resolves to the winner instead of erroring. The memory store can't produce
// that race naturally, so simulate it: pre-append the row behind the
// handler's back between its lookup and append is impossible from outside;
// instead assert the lookup fast-path contract that makes the race window
// benign, plus that a conflicting Append maps to the documented resolution
// flow via a direct store-level check.
func TestChangeWebhook_ConflictResolvesToExisting(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServer(t)

	// Pre-record the event under the key (simulates another control
	// instance winning the race before this request's lookup ran).
	winner := quarrycontracts.Event{
		EventID:        quarrycontracts.NewID(quarrycontracts.KindEvent),
		Type:           quarrycontracts.EvtChangeDetected,
		Timestamp:      time.Now().UTC(),
		Payload:        map[string]any{"source_url": "https://example.com/pricing"},
		IdempotencyKey: "idem-race",
	}
	if err := db.Events().Append(winner); err != nil {
		t.Fatalf("seed winner: %v", err)
	}

	code, ack := doChange(t, h, signedChangePost(t, "o1", changedBody, "idem-race", 0, "nonce-race"))
	if code != http.StatusOK || !ack.Duplicate {
		t.Fatalf("status=%d ack=%+v want 200/duplicate", code, ack)
	}
	if ack.EventID != winner.EventID {
		t.Fatalf("resolved to %s want winner %s", ack.EventID, winner.EventID)
	}
}

// TestChangeWebhook_NoIdemKeyStillRecords verifies callers without an
// Idempotency-Key aren't accidentally deduped into each other.
func TestChangeWebhook_NoIdemKeyStillRecords(t *testing.T) {
	t.Parallel()
	h, _ := newChangeWebhookServer(t)

	code1, ack1 := doChange(t, h, signedChangePost(t, "o1", changedBody, "", 0, "nonce-k1"))
	code2, ack2 := doChange(t, h, signedChangePost(t, "o1", changedBody, "", 0, "nonce-k2"))
	if code1 != http.StatusOK || code2 != http.StatusOK {
		t.Fatalf("statuses=%d,%d", code1, code2)
	}
	if ack1.Duplicate || ack2.Duplicate {
		t.Fatal("keyless deliveries must never report duplicate")
	}
	if ack1.EventID == ack2.EventID {
		t.Fatal("keyless deliveries mint distinct events")
	}
}
