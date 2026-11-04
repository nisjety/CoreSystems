package delegation

import (
	"bytes"
	"net/http"
	"testing"
	"time"
)

// The fixed call quarry-control's sink signs (services/quarry-control/internal/notify,
// TestNotifyChangeMatchesTheNotificationCoreFixture). The literals must match there.
func TestVerifierAcceptsQuarryControlCrossLanguageFixture(t *testing.T) {
	now := time.Date(2026, time.November, 3, 10, 0, 0, 0, time.UTC)
	verifier, err := NewVerifier(Config{
		Audience: "notification-core",
		Keys:     map[string]string{"quarry-control": "0123456789abcdef0123456789abcdef"},
		Now:      func() time.Time { return now },
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	body := []byte(`{"idempotency_key":"evt-1","organization_id":"org-1","payload":{"baseline_id":"base-1","body":"Siden har endret seg. Åpne Innhenting for å se endringen.","diff_id":"diff-1","status":"changed","title":"Endring på www.aquatiq.com","url":"https://www.aquatiq.com/no"},"recipient":{"id":"user-1","kind":"user"},"retention_mode":"standard","source":"quarry","type":"change_detected"}`)
	request, err := http.NewRequest(http.MethodPost, "http://notification-core:3140/api/v1/notification-requests", bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest() error = %v", err)
	}
	request.Header.Set(HeaderServiceID, "quarry-control")
	request.Header.Set(HeaderUserID, "user-1")
	request.Header.Set(HeaderOrganizationID, "org-1")
	request.Header.Set(HeaderTimestamp, "2026-11-03T10:00:00Z")
	request.Header.Set(HeaderNonce, "fixed-nonce-quarry-0001")
	request.Header.Set(HeaderBodySHA256, "IdkszbsrD07ktfTWejg4Q1wIR6jgfa1BE4Oki0Suixg")
	request.Header.Set(HeaderSignature, "vDidBYULyZLo-cCWW-0BLxc0bZicUCzy4g2r90-o5ns")

	principal, err := verifier.Verify(request, body)
	if err != nil {
		t.Fatalf("Verify() error = %v", err)
	}
	if principal.ServiceID != "quarry-control" || principal.UserID != "user-1" || principal.OrganizationID != "org-1" || principal.Role != "" {
		t.Fatalf("principal = %#v", principal)
	}
}
