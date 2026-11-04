package resources

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// capturedNotification is what a faithful stand-in for notification-core's intake
// (POST /api/v1/notification-requests) records: the body as sent, the headers, and
// whether the delegation verifies against the shared secret.
type capturedNotification struct {
	Body      map[string]any
	Headers   http.Header
	VerifyErr error
}

// verifyDelegation recomputes notification-core's HMAC v2 check on its own, so the test
// does not reuse the sink's code to check the sink.
func verifyDelegation(secret string, r *http.Request, body []byte) error {
	sum := sha256.Sum256(body)
	wantDigest := base64.RawURLEncoding.EncodeToString(sum[:])
	if r.Header.Get("x-delegation-body-sha256") != wantDigest {
		return errors.New("body digest mismatch")
	}
	timestamp := r.Header.Get("x-delegation-timestamp")
	sent, err := time.Parse(time.RFC3339, timestamp)
	if err != nil {
		return errors.New("timestamp is not RFC 3339")
	}
	if d := time.Since(sent); d > 2*time.Minute || d < -2*time.Minute {
		return errors.New("timestamp is outside the allowed skew")
	}
	canonical := strings.Join([]string{
		"v2",
		r.Header.Get("x-service-id"),
		"notification-core",
		timestamp,
		r.Header.Get("x-delegation-nonce"),
		r.Method,
		r.URL.RequestURI(),
		r.Header.Get("x-user-id"),
		r.Header.Get("x-org-id"),
		r.Header.Get("x-user-role"),
		wantDigest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	got, err := base64.RawURLEncoding.DecodeString(r.Header.Get("x-delegation-signature"))
	if err != nil || !hmac.Equal(got, mac.Sum(nil)) {
		return errors.New("signature mismatch")
	}
	return nil
}

// TestChangeToInProductNotification_E2E exercises the full W2 monitor→notify
// leg through real wiring:
//
//	POST /v1/runs/{id}/events  (a `change_detected` event with a creator and org)
//	  → notifyOnChange → notify.HTTPSink.NotifyChange
//	  → POST notification-core /api/v1/notification-requests (signed as quarry-control)
//
// By default it runs HERMETICALLY against an httptest stand-in for
// notification-core, so it is a real end-to-end of the producing side (event
// intake → sink → HTTP POST) without external infra and is NOT skipped in CI.
//
// Set QUARRY_E2E_NOTIFY_URL to a live notification-core base URL (and
// QUARRY_E2E_NOTIFY_SECRET to the quarry-control delegation secret) to drive the
// real stack instead.
func TestChangeToInProductNotification_E2E(t *testing.T) {
	const recipient = "user_creator_1"
	const orgID = "org_a"
	const pageURL = "https://a.example/pricing"
	const secret = "test-quarry-control-secret"

	var notifyBaseURL, notifySecret string
	captured := make(chan capturedNotification, 4)
	live := strings.TrimSpace(os.Getenv("QUARRY_E2E_NOTIFY_URL")) != ""

	if live {
		// Live-stack mode: post against the real notification-core intake.
		notifyBaseURL = strings.TrimSpace(os.Getenv("QUARRY_E2E_NOTIFY_URL"))
		notifySecret = os.Getenv("QUARRY_E2E_NOTIFY_SECRET")
		t.Logf("change→notify e2e: LIVE mode against %s", notifyBaseURL)
	} else {
		// Hermetic mode: a stand-in that records the POST notification-core would
		// have received, checks its delegation, then 201s like the real intake.
		stub := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path != "/api/v1/notification-requests" || r.Method != http.MethodPost {
				http.Error(w, "unexpected route", http.StatusNotFound)
				return
			}
			body, _ := io.ReadAll(r.Body)
			var decoded map[string]any
			_ = json.Unmarshal(body, &decoded)
			captured <- capturedNotification{
				Body:      decoded,
				Headers:   r.Header.Clone(),
				VerifyErr: verifyDelegation(secret, r, body),
			}
			w.WriteHeader(http.StatusCreated)
		}))
		defer stub.Close()
		notifyBaseURL = stub.URL
		notifySecret = secret
		t.Logf("change→notify e2e: HERMETIC mode (httptest notification-core stand-in)")
	}

	// Stand up control's event intake wired to the REAL HTTPSink → notify base.
	sink := notify.NewHTTPSink(notifyBaseURL, notifySecret)
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", sink) // empty apiKey → bearer auth disabled for the test
	srv := httptest.NewServer(r)
	defer srv.Close()

	// Post a change_detected event with the keys the orchestrator really sends
	// (url, org_id, created_by, status, diff_id, baseline_id) and a fingerprint, which
	// must not reach the feed.
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	batch := []map[string]any{{
		"type": string(quarrycontracts.EvtChangeDetected),
		"payload": map[string]any{
			"url":         pageURL,
			"org_id":      orgID,
			"created_by":  recipient,
			"status":      "changed",
			"diff_id":     "diff_1",
			"baseline_id": "base_1",
			"fingerprint": "blake3:0000",
		},
	}}
	body, _ := json.Marshal(batch)
	resp, err := http.Post(srv.URL+"/v1/runs/"+string(runID)+"/events", "application/json", strings.NewReader(string(body)))
	if err != nil {
		t.Fatalf("post events: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusCreated {
		rb, _ := io.ReadAll(resp.Body)
		t.Fatalf("event intake status=%d body=%s", resp.StatusCode, string(rb))
	}

	if live {
		// Live mode: only the intake's acceptance is asserted here. Sink delivery is
		// best-effort, and a non-2xx is logged by notifyOnChange.
		t.Log("change→notify e2e (live): event accepted; delivery is best-effort and checked in control logs")
		return
	}

	select {
	case n := <-captured:
		if n.VerifyErr != nil {
			t.Fatalf("delegation did not verify: %v", n.VerifyErr)
		}
		if got := n.Headers.Get("x-service-id"); got != notify.ServiceID {
			t.Errorf("x-service-id=%q want=%q", got, notify.ServiceID)
		}
		if got := n.Headers.Get("x-user-id"); got != recipient {
			t.Errorf("x-user-id=%q want=%q", got, recipient)
		}
		if got := n.Headers.Get("x-org-id"); got != orgID {
			t.Errorf("x-org-id=%q want=%q", got, orgID)
		}
		if got, _ := n.Body["type"].(string); got != "change_detected" {
			t.Errorf("type=%q want=change_detected", got)
		}
		if got, _ := n.Body["organization_id"].(string); got != orgID {
			t.Errorf("organization_id=%q want=%q", got, orgID)
		}
		recip, _ := n.Body["recipient"].(map[string]any)
		if recip["kind"] != "user" || recip["id"] != recipient {
			t.Errorf("recipient=%v want kind=user id=%s", recip, recipient)
		}
		if got, _ := n.Body["source"].(string); got != "quarry" {
			t.Errorf("source=%q want=quarry", got)
		}
		if got, _ := n.Body["retention_mode"].(string); got != "standard" {
			t.Errorf("retention_mode=%q want=standard", got)
		}
		if got, _ := n.Body["idempotency_key"].(string); got == "" {
			t.Error("expected idempotency_key (the event id) to be set")
		}
		payload, _ := n.Body["payload"].(map[string]any)
		if title, _ := payload["title"].(string); !strings.Contains(title, "a.example") {
			t.Errorf("payload.title=%q should name the page host", title)
		}
		if got, _ := payload["url"].(string); got != pageURL {
			t.Errorf("payload.url=%q want=%q", got, pageURL)
		}
		if _, leaked := payload["fingerprint"]; leaked {
			t.Error("the fingerprint must not reach the notification payload")
		}
	default:
		t.Fatal("notification-core stand-in received no change notification")
	}
}

// TestChangeNotify_SkipsWhenNoCreator locks the honesty rule: a change event
// with no creator (legacy / non-user schedule) produces NO notification rather
// than fabricating a recipient.
func TestChangeNotify_SkipsWhenNoCreator(t *testing.T) {
	t.Parallel()
	assertNoNotification(t, map[string]any{"org_id": "org_a", "status": "changed"})
}

// TestChangeNotify_SkipsWhenNoOrganization: without an organization the notification
// has no scope, so none is sent.
func TestChangeNotify_SkipsWhenNoOrganization(t *testing.T) {
	t.Parallel()
	assertNoNotification(t, map[string]any{"created_by": "user_creator_1", "status": "changed"})
}

func assertNoNotification(t *testing.T, payload map[string]any) {
	t.Helper()
	got := make(chan struct{}, 1)
	stub := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		got <- struct{}{}
		w.WriteHeader(http.StatusCreated)
	}))
	defer stub.Close()

	sink := notify.NewHTTPSink(stub.URL, "test-quarry-control-secret")
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", sink)
	srv := httptest.NewServer(r)
	defer srv.Close()

	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	batch := []map[string]any{{
		"type":    string(quarrycontracts.EvtChangeDetected),
		"payload": payload,
	}}
	body, _ := json.Marshal(batch)
	resp, err := http.Post(srv.URL+"/v1/runs/"+string(runID)+"/events", "application/json", strings.NewReader(string(body)))
	if err != nil {
		t.Fatalf("post: %v", err)
	}
	resp.Body.Close()

	select {
	case <-got:
		t.Fatal("a change event without a creator or an organization must NOT produce a notification")
	default:
	}
}
