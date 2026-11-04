package notify

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

const testSecret = "test-quarry-control-secret"

var fixedTime = time.Date(2026, 11, 3, 10, 0, 0, 0, time.UTC)

// newTestSink points a sink at a server with a fixed clock and nonce, so the signed
// headers can be checked exactly.
func newTestSink(baseURL string) *HTTPSink {
	s := NewHTTPSink(baseURL, testSecret)
	s.now = func() time.Time { return fixedTime }
	s.nonce = func() (string, error) { return "nonce_abcdefghijklmnop", nil }
	return s
}

func TestNotifyChangeSendsTheTypedBodyAndASignedDelegation(t *testing.T) {
	var gotBody []byte
	var gotHeader http.Header
	var gotPath, gotMethod string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotBody, _ = io.ReadAll(r.Body)
		gotHeader = r.Header.Clone()
		gotPath, gotMethod = r.URL.Path, r.Method
		w.WriteHeader(http.StatusCreated)
	}))
	defer srv.Close()

	payload := map[string]any{
		"url":         "https://www.aquatiq.com/no",
		"org_id":      "org_a",
		"created_by":  "user_creator_1",
		"status":      "changed",
		"diff_id":     "diff_1",
		"baseline_id": "base_1",
		"fingerprint": "blake3:secret-looking-value",
	}
	if err := newTestSink(srv.URL).NotifyChange(context.Background(), "org_a", "user_creator_1", "evt_1", payload); err != nil {
		t.Fatalf("NotifyChange: %v", err)
	}

	if gotMethod != http.MethodPost || gotPath != intakePath {
		t.Fatalf("sent %s %s, want POST %s", gotMethod, gotPath, intakePath)
	}

	var body map[string]any
	if err := json.Unmarshal(gotBody, &body); err != nil {
		t.Fatalf("body is not JSON: %v", err)
	}
	if body["organization_id"] != "org_a" || body["idempotency_key"] != "evt_1" || body["type"] != "change_detected" {
		t.Errorf("body identity fields: %v", body)
	}
	if body["source"] != "quarry" || body["retention_mode"] != "standard" {
		t.Errorf("body source/retention: %v", body)
	}
	recipient, _ := body["recipient"].(map[string]any)
	if recipient["kind"] != "user" || recipient["id"] != "user_creator_1" {
		t.Errorf("recipient=%v", recipient)
	}
	if _, present := body["recipient_id"]; present {
		t.Error("the legacy recipient_id field must not be sent")
	}

	feed, _ := body["payload"].(map[string]any)
	if feed["title"] != "Endring på www.aquatiq.com" {
		t.Errorf("payload.title=%v", feed["title"])
	}
	if feed["body"] == nil || feed["body"] == "" {
		t.Error("payload.body must be set for the feed")
	}
	if _, leaked := feed["fingerprint"]; leaked {
		t.Error("the fingerprint must not be sent in the feed payload")
	}

	// The delegation: the headers name this service, the recipient and the org, and the
	// signature matches the canonical string notification-core builds.
	if gotHeader.Get("x-service-id") != ServiceID || gotHeader.Get("x-user-id") != "user_creator_1" || gotHeader.Get("x-org-id") != "org_a" {
		t.Errorf("delegation identity headers: %v", gotHeader)
	}
	if gotHeader.Get("x-delegation-timestamp") != "2026-11-03T10:00:00Z" {
		t.Errorf("timestamp=%q", gotHeader.Get("x-delegation-timestamp"))
	}
	digest := sha256.Sum256(gotBody)
	wantDigest := base64.RawURLEncoding.EncodeToString(digest[:])
	if gotHeader.Get("x-delegation-body-sha256") != wantDigest {
		t.Errorf("body digest header does not match the body")
	}
	canonical := strings.Join([]string{
		"v2", ServiceID, Audience, "2026-11-03T10:00:00Z", "nonce_abcdefghijklmnop",
		http.MethodPost, intakePath, "user_creator_1", "org_a", "", wantDigest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(testSecret))
	_, _ = mac.Write([]byte(canonical))
	gotSig, err := base64.RawURLEncoding.DecodeString(gotHeader.Get("x-delegation-signature"))
	if err != nil || !hmac.Equal(gotSig, mac.Sum(nil)) {
		t.Error("the delegation signature does not match the canonical string")
	}
}

func TestNotifyChangeRefusesWithoutADelegationSecret(t *testing.T) {
	called := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		called = true
		w.WriteHeader(http.StatusCreated)
	}))
	defer srv.Close()

	err := NewHTTPSink(srv.URL, "").NotifyChange(context.Background(), "org_a", "user_a", "evt_1", nil)
	if err == nil {
		t.Fatal("a sink without a secret must refuse to send")
	}
	if called {
		t.Fatal("no request may leave without a delegation")
	}
}

func TestNotifyChangeNeedsAnOrganizationRecipientAndEvent(t *testing.T) {
	cases := []struct{ org, recipient, event string }{
		{"", "user_a", "evt_1"},
		{"org_a", "", "evt_1"},
		{"org_a", "user_a", ""},
	}
	for _, c := range cases {
		err := newTestSink("http://127.0.0.1:1").NotifyChange(context.Background(), c.org, c.recipient, c.event, nil)
		if err == nil {
			t.Errorf("org=%q recipient=%q event=%q: want an error", c.org, c.recipient, c.event)
		}
	}
}

func TestNotifyChangeReportsANon2xxAnswer(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "forbidden", http.StatusForbidden)
	}))
	defer srv.Close()

	err := newTestSink(srv.URL).NotifyChange(context.Background(), "org_a", "user_a", "evt_1", nil)
	if err == nil || !strings.Contains(err.Error(), "403") {
		t.Fatalf("want a 403 error, got %v", err)
	}
}

func TestChangePayloadKeepsTheReferencesAndNamesThePage(t *testing.T) {
	got := changePayload(map[string]any{
		"url":         "https://www.aquatiq.com/no/produkter",
		"status":      "changed",
		"diff_id":     "diff_1",
		"baseline_id": "base_1",
		"fingerprint": "blake3:0000",
		"created_by":  "user_a",
	})
	if got["title"] != "Endring på www.aquatiq.com" {
		t.Errorf("title=%v", got["title"])
	}
	for _, key := range []string{"url", "status", "diff_id", "baseline_id"} {
		if got[key] == nil || got[key] == "" {
			t.Errorf("%s should be kept", key)
		}
	}
	if _, leaked := got["fingerprint"]; leaked {
		t.Error("fingerprint must not be kept")
	}
	if _, leaked := got["created_by"]; leaked {
		t.Error("created_by is the recipient, not feed content")
	}
	if fallback := changePayload(map[string]any{"url": "not a url"})["title"]; fallback != "Endring på en overvåket side" {
		t.Errorf("fallback title=%v", fallback)
	}
}
