package dispatcher

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const testSecret = "supersecret"

// testOrg owns every subscription and delivery these tests create: the dispatcher refuses a
// subscription with no org and a delivery whose org differs from its subscription's.
const testOrg = "org_dispatch_test"

func verifySignature(t *testing.T, r *http.Request, body, secret string) {
	t.Helper()
	sig := r.Header.Get("X-Quarry-Signature")
	if sig == "" {
		t.Fatalf("missing X-Quarry-Signature header")
	}
	parts := strings.Split(sig, ",")
	if len(parts) != 2 {
		t.Fatalf("unexpected signature format: %q", sig)
	}
	tsPart := strings.TrimPrefix(parts[0], "t=")
	v1Part := strings.TrimPrefix(parts[1], "v1=")
	if tsPart == parts[0] || v1Part == parts[1] {
		t.Fatalf("malformed signature: %q", sig)
	}
	ts, err := strconv.ParseInt(tsPart, 10, 64)
	if err != nil {
		t.Fatalf("invalid timestamp in signature: %v", err)
	}
	mac := hmac.New(sha256.New, []byte(secret))
	fmt.Fprintf(mac, "%d.%s", ts, body)
	want := hex.EncodeToString(mac.Sum(nil))
	if !hmac.Equal([]byte(want), []byte(v1Part)) {
		t.Fatalf("signature mismatch: got %s want %s", v1Part, want)
	}
}

func newWebhook(t *testing.T, db store.DB, url string, active bool) store.Webhook {
	t.Helper()
	wh := store.Webhook{
		ID:        quarrycontracts.ID("wh_" + t.Name()),
		OrgID:     testOrg,
		URL:       url,
		Secret:    testSecret,
		Events:    []string{"job.completed"},
		Active:    active,
		CreatedAt: time.Now().Unix(),
	}
	if err := db.Webhooks().Create(wh); err != nil {
		t.Fatalf("create webhook: %v", err)
	}
	return wh
}

func newDelivery(t *testing.T, db store.DB, whID quarrycontracts.ID, payload string) store.WebhookDelivery {
	t.Helper()
	d := store.WebhookDelivery{
		ID:            quarrycontracts.ID("wd_" + t.Name()),
		WebhookID:     whID,
		OrgID:         testOrg,
		EventID:       quarrycontracts.ID("ev_" + t.Name()),
		Payload:       payload,
		Attempt:       0,
		Status:        "pending",
		NextAttemptAt: time.Now().Unix(),
		CreatedAt:     time.Now().Unix(),
	}
	if err := db.WebhookDeliveries().Create(d); err != nil {
		t.Fatalf("create delivery: %v", err)
	}
	return d
}

func TestDispatch_Success(t *testing.T) {
	const payload = `{"hello":"world"}`
	var gotBody string
	var gotCT string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(r.Body)
		gotBody = string(b)
		gotCT = r.Header.Get("Content-Type")
		verifySignature(t, r, gotBody, testSecret)
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, payload)

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, ok := db.WebhookDeliveries().Get(d.ID)
	if !ok {
		t.Fatalf("delivery not found")
	}
	if got.Status != "success" {
		t.Errorf("status = %q, want success", got.Status)
	}
	if got.LastError != "" {
		t.Errorf("LastError = %q, want empty", got.LastError)
	}
	if gotBody != payload {
		t.Errorf("body = %q, want %q", gotBody, payload)
	}
	if gotCT != "application/json" {
		t.Errorf("Content-Type = %q, want application/json", gotCT)
	}
}

func TestDispatch_RetryOn5xx(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)

	before := time.Now().Unix()
	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, ok := db.WebhookDeliveries().Get(d.ID)
	if !ok {
		t.Fatalf("delivery not found")
	}
	if got.Status != "pending" {
		t.Errorf("status = %q, want pending", got.Status)
	}
	if got.Attempt != 1 {
		t.Errorf("Attempt = %d, want 1", got.Attempt)
	}
	if got.LastError != "http 500" {
		t.Errorf("LastError = %q, want %q", got.LastError, "http 500")
	}
	// First retry uses backoff[1] = 5s.
	wantNext := before + 5
	if got.NextAttemptAt < wantNext-2 || got.NextAttemptAt > wantNext+2 {
		t.Errorf("NextAttemptAt = %d, want ~%d", got.NextAttemptAt, wantNext)
	}
}

func TestDispatch_DLQOn4xx(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, ok := db.WebhookDeliveries().Get(d.ID)
	if !ok {
		t.Fatalf("delivery not found")
	}
	if got.Status != "dlq" {
		t.Errorf("status = %q, want dlq", got.Status)
	}
	if got.LastError != "http 400" {
		t.Errorf("LastError = %q, want %q", got.LastError, "http 400")
	}
	if got.Attempt != 0 {
		t.Errorf("Attempt = %d, want 0 (immediate dlq, no increment)", got.Attempt)
	}
}

func TestDispatch_RetryOn429(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusTooManyRequests)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, _ := db.WebhookDeliveries().Get(d.ID)
	if got.Status != "pending" {
		t.Errorf("status = %q, want pending (429 is retryable)", got.Status)
	}
	if got.Attempt != 1 {
		t.Errorf("Attempt = %d, want 1", got.Attempt)
	}
}

func TestDispatch_InactiveWebhook(t *testing.T) {
	db := store.NewMemory()
	wh := newWebhook(t, db, "http://unused.invalid", false)
	d := newDelivery(t, db, wh.ID, `{}`)

	dispatch(context.Background(), db, &http.Client{Timeout: 1 * time.Second}, d, zerolog.Nop())

	got, ok := db.WebhookDeliveries().Get(d.ID)
	if !ok {
		t.Fatalf("delivery not found")
	}
	if got.Status != "failed" {
		t.Errorf("status = %q, want failed", got.Status)
	}
	if got.LastError != "webhook missing or inactive" {
		t.Errorf("LastError = %q, want %q", got.LastError, "webhook missing or inactive")
	}
}

func TestDispatch_TransportError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	url := srv.URL
	srv.Close() // shut down before dispatch so connection is refused

	db := store.NewMemory()
	wh := newWebhook(t, db, url, true)
	d := newDelivery(t, db, wh.ID, `{}`)

	dispatch(context.Background(), db, &http.Client{Timeout: 1 * time.Second}, d, zerolog.Nop())

	got, ok := db.WebhookDeliveries().Get(d.ID)
	if !ok {
		t.Fatalf("delivery not found")
	}
	if got.Status != "pending" {
		t.Errorf("status = %q, want pending (transport error is retryable)", got.Status)
	}
	if got.Attempt != 1 {
		t.Errorf("Attempt = %d, want 1", got.Attempt)
	}
	if !strings.HasPrefix(got.LastError, "transport: ") {
		t.Errorf("LastError = %q, want prefix %q", got.LastError, "transport: ")
	}
}

func TestDispatch_DLQAfterMaxAttempts(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)
	d.Attempt = maxAttempts - 1
	if err := db.WebhookDeliveries().Update(d); err != nil {
		t.Fatalf("update delivery: %v", err)
	}

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, _ := db.WebhookDeliveries().Get(d.ID)
	if got.Status != "dlq" {
		t.Errorf("status = %q, want dlq after maxAttempts reached", got.Status)
	}
	if got.Attempt != maxAttempts {
		t.Errorf("Attempt = %d, want %d", got.Attempt, maxAttempts)
	}
}

// --- INNH-5: tenancy, SSRF, and the documented signature -----------------------------------

func TestDispatch_RefusesWebhookWithoutOrg(t *testing.T) {
	hit := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { hit = true }))
	defer srv.Close()

	db := store.NewMemory()
	// A legacy row: no org.
	legacy := store.Webhook{ID: "wh_legacy", URL: srv.URL, Secret: testSecret, Events: []string{"*"}, Active: true}
	if err := db.Webhooks().Create(legacy); err != nil {
		t.Fatal(err)
	}
	d := newDelivery(t, db, legacy.ID, `{"secret":"data"}`)

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	got, _ := db.WebhookDeliveries().Get(d.ID)
	if hit {
		t.Fatal("a delivery for a subscription with no org reached the target")
	}
	if got.Status != "failed" || got.LastError != "webhook org mismatch" {
		t.Fatalf("status=%q last_error=%q, want failed / webhook org mismatch", got.Status, got.LastError)
	}
}

func TestDispatch_RefusesDeliveryOfAnotherOrg(t *testing.T) {
	hit := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { hit = true }))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)
	d.OrgID = "org_somebody_else" // the row handed to dispatch disagrees with its subscription

	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	if hit {
		t.Fatal("a delivery owned by another org reached the subscription's target")
	}
	got, _ := db.WebhookDeliveries().Get(d.ID)
	if got.Status != "failed" {
		t.Fatalf("status = %q, want failed", got.Status)
	}
}

// TestDispatch_SignatureMatchesDocumentedRecipe recomputes the header from the documented
// recipe (HMAC-SHA256 over "<t>.<raw body>" keyed with the subscription secret, hex) with no
// dispatcher code, and checks the header grammar a customer would parse.
func TestDispatch_SignatureMatchesDocumentedRecipe(t *testing.T) {
	const payload = `{"event_id":"evt_1","type":"change_detected","ts":"2026-10-31T00:00:00Z","payload":{"subject":"quarry.change.detected"}}`
	var header, body string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(r.Body)
		body = string(b)
		header = r.Header.Get("X-Quarry-Signature")
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, payload)
	dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())

	if body != payload {
		t.Fatalf("body was altered: %q", body)
	}
	var tsStr, sig string
	for _, part := range strings.Split(header, ",") {
		switch {
		case strings.HasPrefix(part, "t="):
			tsStr = strings.TrimPrefix(part, "t=")
		case strings.HasPrefix(part, "v1="):
			sig = strings.TrimPrefix(part, "v1=")
		}
	}
	if tsStr == "" || sig == "" || len(strings.Split(header, ",")) != 2 {
		t.Fatalf("header %q is not t=<unix>,v1=<hex>", header)
	}
	mac := hmac.New(sha256.New, []byte(testSecret))
	mac.Write([]byte(tsStr + "." + payload))
	if want := hex.EncodeToString(mac.Sum(nil)); sig != want {
		t.Fatalf("v1 = %s, want %s", sig, want)
	}
	if ts, err := strconv.ParseInt(tsStr, 10, 64); err != nil || time.Since(time.Unix(ts, 0)) > time.Minute {
		t.Fatalf("t=%q is not a current unix timestamp", tsStr)
	}
}

// TestDispatch_BackoffThenDLQAtAttempt5 walks one delivery through five failing sends and
// checks the schedule 5s, 30s, 5m, 1h and then dlq on the fifth failure.
func TestDispatch_BackoffThenDLQAtAttempt5(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
	}))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)
	want := []time.Duration{5 * time.Second, 30 * time.Second, 5 * time.Minute, time.Hour}
	for i := 0; i < maxAttempts; i++ {
		before := time.Now().Unix()
		dispatch(context.Background(), db, &http.Client{Timeout: 5 * time.Second}, d, zerolog.Nop())
		d, _ = db.WebhookDeliveries().Get(d.ID)
		if i < maxAttempts-1 {
			if d.Status != "pending" || d.Attempt != i+1 {
				t.Fatalf("send %d: status=%q attempt=%d", i+1, d.Status, d.Attempt)
			}
			got := time.Duration(d.NextAttemptAt-before) * time.Second
			if got < want[i]-2*time.Second || got > want[i]+2*time.Second {
				t.Fatalf("send %d: next attempt in %v, want about %v", i+1, got, want[i])
			}
		}
	}
	if d.Status != "dlq" || d.Attempt != maxAttempts {
		t.Fatalf("after %d failures: status=%q attempt=%d, want dlq/%d", maxAttempts, d.Status, d.Attempt, maxAttempts)
	}
}

// TestDispatch_SSRFTargetIsRefused runs the dispatcher with the production client (egress
// SafeClient with a default-policy guard) against a loopback target. The connection must be
// refused before any byte is sent, and the delivery dead-lettered at once.
func TestDispatch_SSRFTargetIsRefused(t *testing.T) {
	hit := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { hit = true }))
	defer srv.Close()

	db := store.NewMemory()
	wh := newWebhook(t, db, srv.URL, true)
	d := newDelivery(t, db, wh.ID, `{}`)

	hc := egress.SafeClient(egress.ClientConfig{RequestTimeout: 5 * time.Second, Guard: egress.New()})
	dispatch(context.Background(), db, hc, d, zerolog.Nop())

	if hit {
		t.Fatal("the guard let a loopback target be dialled")
	}
	got, _ := db.WebhookDeliveries().Get(d.ID)
	if got.Status != "dlq" {
		t.Fatalf("status = %q (last_error %q), want dlq at once for a blocked target", got.Status, got.LastError)
	}
	if !strings.Contains(got.LastError, "destination blocked by policy") {
		t.Fatalf("last_error = %q, want the egress block", got.LastError)
	}
}
