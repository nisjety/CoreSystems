package notify

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

// The same fixed call is pinned in notification-core's delegation tests
// (TestVerifierAcceptsQuarryControlCrossLanguageFixture). A change to the body, the
// canonical string or the signing on either side fails a test on both.
const (
	goldenSecret    = "0123456789abcdef0123456789abcdef"
	goldenBody      = `{"idempotency_key":"evt-1","organization_id":"org-1","payload":{"baseline_id":"base-1","body":"Siden har endret seg. Åpne Innhenting for å se endringen.","diff_id":"diff-1","status":"changed","title":"Endring på www.aquatiq.com","url":"https://www.aquatiq.com/no"},"recipient":{"id":"user-1","kind":"user"},"retention_mode":"standard","source":"quarry","type":"change_detected"}`
	goldenBodySHA   = "IdkszbsrD07ktfTWejg4Q1wIR6jgfa1BE4Oki0Suixg"
	goldenSignature = "vDidBYULyZLo-cCWW-0BLxc0bZicUCzy4g2r90-o5ns"
	goldenTimestamp = "2026-11-03T10:00:00Z"
	goldenNonce     = "fixed-nonce-quarry-0001"
)

func TestNotifyChangeMatchesTheNotificationCoreFixture(t *testing.T) {
	var gotBody []byte
	var gotHeader http.Header
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotBody, _ = io.ReadAll(r.Body)
		gotHeader = r.Header.Clone()
		w.WriteHeader(http.StatusCreated)
	}))
	defer srv.Close()

	s := NewHTTPSink(srv.URL, goldenSecret)
	s.now = func() time.Time { return time.Date(2026, 11, 3, 10, 0, 0, 0, time.UTC) }
	s.nonce = func() (string, error) { return goldenNonce, nil }
	err := s.NotifyChange(context.Background(), "org-1", "user-1", "evt-1", map[string]any{
		"url":         "https://www.aquatiq.com/no",
		"status":      "changed",
		"diff_id":     "diff-1",
		"baseline_id": "base-1",
	})
	if err != nil {
		t.Fatalf("NotifyChange: %v", err)
	}
	if string(gotBody) != goldenBody {
		t.Errorf("body\n got=%s\nwant=%s", gotBody, goldenBody)
	}
	if got := gotHeader.Get("x-delegation-body-sha256"); got != goldenBodySHA {
		t.Errorf("body sha=%q want=%q", got, goldenBodySHA)
	}
	if got := gotHeader.Get("x-delegation-signature"); got != goldenSignature {
		t.Errorf("signature=%q want=%q", got, goldenSignature)
	}
	if got := gotHeader.Get("x-delegation-timestamp"); got != goldenTimestamp {
		t.Errorf("timestamp=%q want=%q", got, goldenTimestamp)
	}
	if got := gotHeader.Get("x-delegation-nonce"); got != goldenNonce {
		t.Errorf("nonce=%q want=%q", got, goldenNonce)
	}
}
