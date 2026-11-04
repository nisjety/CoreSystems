package http

import (
	"bytes"
	"encoding/json"
	stdhttp "net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const testQuarryControlDelegationSecret = "quarry-control-test-secret-at-least-32-bytes"

// newTestVerifierWithQuarryControl is newTestVerifier plus the "quarry-control"
// principal, for tests that exercise Quarry's notice caller.
func newTestVerifierWithQuarryControl(t *testing.T) *delegation.Verifier {
	t.Helper()
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "notification-core",
		Keys: map[string]string{
			"verevon-gateway": testDelegationSecret,
			"quarry-control":  testQuarryControlDelegationSecret,
		},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	return verifier
}

// quarryNoticeBody builds a Quarry notice for recipientID. The idempotency key is
// fixed, so a test that sends the same notice twice exercises a replay.
func quarryNoticeBody(t *testing.T, notificationType, retentionMode, recipientID string, payload map[string]any) []byte {
	t.Helper()
	body, err := json.Marshal(map[string]any{
		"organization_id": "org_123",
		"idempotency_key": "quarry-event-1",
		"recipient":       map[string]any{"kind": "user", "id": recipientID},
		"type":            notificationType,
		"payload":         payload,
		"retention_mode":  retentionMode,
	})
	if err != nil {
		t.Fatalf("json.Marshal error = %v", err)
	}
	return body
}

// quarryNotice is the same notice as quarryNoticeBody, as a struct for the rule table.
func quarryNotice(notificationType, retentionMode, recipientID, idempotencyKey string, payload map[string]any) notification.Request {
	return notification.Request{
		OrganizationID: "org_123",
		IdempotencyKey: idempotencyKey,
		Recipient:      notification.Recipient{Kind: "user", ID: recipientID},
		Type:           notificationType,
		Payload:        payload,
		RetentionMode:  retentionMode,
	}
}

// postQuarryNotice sends body as the quarry-control principal acting for
// delegatedUserID. It returns the response status and the repository the service
// wrote to, so a test can check what was stored.
func postQuarryNotice(t *testing.T, body []byte, delegatedUserID string) (int, *testRepository) {
	t.Helper()
	repository := newTestRepository()
	service := notification.NewService(
		repository,
		&testRuntimeClient{providerRequestID: "novu_req_quarry"},
		noopPublisher{},
		notification.WithIDGenerator(func() string { return "req_quarry" }),
		notification.WithRecipientResolver(testRecipientResolver{}),
	)
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: service})
	server := httptest.NewServer(newRouter(handler, newTestVerifierWithQuarryControl(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequestAs(t, request, body, "quarry-control", testQuarryControlDelegationSecret, delegatedUserID, "org_123", "")

	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST /api/v1/notification-requests error = %v", err)
	}
	defer response.Body.Close()
	return response.StatusCode, repository
}

// A schedule notice reaches the delegated user, the schedule's creator, as a
// content-free standard payload.
func TestQuarryScheduleNoticeToDelegatedUserIsAccepted(t *testing.T) {
	body := quarryNoticeBody(t, "quarry.schedule_run_skipped", "standard", "user_123", map[string]any{
		"title":         "Scheduled run skipped",
		"body":          "Open Innhenting to see why.",
		"schedule_id":   "sched_1",
		"schedule_kind": "crawl",
		"reason":        "skipped_zdr",
		"run_id":        "run_1",
	})
	status, repository := postQuarryNotice(t, body, "user_123")
	if status != stdhttp.StatusAccepted {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusAccepted)
	}
	stored := repository.stored["req_quarry"]
	if stored == nil {
		t.Fatal("accepted notice was not stored")
	}
	if stored.Source != "quarry-control" || stored.RecipientID != "user_123" || stored.RetentionMode != "standard" {
		t.Fatalf("stored notice source=%q recipient=%q retention=%q", stored.Source, stored.RecipientID, stored.RetentionMode)
	}
}

// A schedule notice addressed to another member is refused: the notice principal is
// the delegated user, the schedule's creator.
func TestQuarryScheduleNoticeToAnotherUserIsRefused(t *testing.T) {
	body := quarryNoticeBody(t, "quarry.schedule_run_failed", "standard", "user_123", map[string]any{"title": "Scheduled run failed"})
	status, repository := postQuarryNotice(t, body, "user_999")
	if status != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusForbidden)
	}
	if len(repository.stored) != 0 {
		t.Fatal("refused notice was stored")
	}
}

// Under ZDR a schedule notice carries its title only, and the stored request holds no
// payload.
func TestQuarryZDRScheduleNoticeCarriesTitleOnly(t *testing.T) {
	body := quarryNoticeBody(t, "quarry.schedule_run_skipped", "zdr", "user_123", map[string]any{"title": "Scheduled run skipped"})
	status, repository := postQuarryNotice(t, body, "user_123")
	if status != stdhttp.StatusAccepted {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusAccepted)
	}
	stored := repository.stored["req_quarry"]
	if stored == nil {
		t.Fatal("accepted notice was not stored")
	}
	if stored.RetentionMode != "zdr" || stored.Payload != nil {
		t.Fatalf("stored retention=%q payload=%v, want zdr with no payload", stored.RetentionMode, stored.Payload)
	}
}

// A ZDR schedule notice with a body or a URL is refused before anything is stored.
func TestQuarryZDRScheduleNoticeWithContentIsRefused(t *testing.T) {
	cases := []struct {
		name    string
		payload map[string]any
	}{
		{name: "body", payload: map[string]any{"title": "Scheduled run failed", "body": "page text"}},
		{name: "url", payload: map[string]any{"title": "Scheduled run failed", "url": "https://example.com/private"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			body := quarryNoticeBody(t, "quarry.schedule_run_failed", "zdr", "user_123", tc.payload)
			status, repository := postQuarryNotice(t, body, "user_123")
			if status != stdhttp.StatusBadRequest {
				t.Fatalf("status = %d, want %d", status, stdhttp.StatusBadRequest)
			}
			if len(repository.stored) != 0 {
				t.Fatal("refused ZDR notice was stored")
			}
		})
	}
}

// A standard schedule notice never carries a URL: its payload is a closed key set.
func TestQuarryStandardScheduleNoticeCannotCarryURL(t *testing.T) {
	body := quarryNoticeBody(t, "quarry.schedule_run_failed", "standard", "user_123", map[string]any{
		"title": "Scheduled run failed",
		"url":   "https://example.com/private",
	})
	status, _ := postQuarryNotice(t, body, "user_123")
	if status != stdhttp.StatusBadRequest {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusBadRequest)
	}
}

// The change notice is accepted in standard retention. Before this caller was
// registered in the request switch, every quarry-control request was refused.
func TestQuarryChangeNoticeIsStandardRetentionOnly(t *testing.T) {
	standard := quarryNoticeBody(t, "change_detected", "standard", "user_123", map[string]any{
		"title":       "Change detected on www.aquatiq.com",
		"body":        "The page has changed.",
		"url":         "https://www.aquatiq.com/no",
		"status":      "changed",
		"diff_id":     "diff-1",
		"baseline_id": "base-1",
	})
	if status, _ := postQuarryNotice(t, standard, "user_123"); status != stdhttp.StatusAccepted {
		t.Fatalf("standard change notice status = %d, want %d", status, stdhttp.StatusAccepted)
	}
	zdr := quarryNoticeBody(t, "change_detected", "zdr", "user_123", map[string]any{"title": "Change detected"})
	if status, _ := postQuarryNotice(t, zdr, "user_123"); status != stdhttp.StatusForbidden {
		t.Fatalf("ZDR change notice status = %d, want %d", status, stdhttp.StatusForbidden)
	}
}

// A notice without an idempotency key is refused, so a replayed batch cannot add a
// second notice.
func TestQuarryNoticeRequiresIdempotencyKey(t *testing.T) {
	body := []byte(`{"organization_id":"org_123","recipient":{"kind":"user","id":"user_123"},"type":"quarry.schedule_run_failed","retention_mode":"standard","payload":{"title":"Scheduled run failed"}}`)
	status, _ := postQuarryNotice(t, body, "user_123")
	if status != stdhttp.StatusBadRequest {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusBadRequest)
	}
}

// A type outside the registered set is refused by the allowlist.
func TestQuarryControlRefusesUnregisteredNoticeType(t *testing.T) {
	body := quarryNoticeBody(t, "quarry.schedule_paused", "standard", "user_123", map[string]any{"title": "Schedule paused"})
	status, _ := postQuarryNotice(t, body, "user_123")
	if status != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want %d", status, stdhttp.StatusForbidden)
	}
}

// The rules in quarryNoticeRefusal, checked without the HTTP layer.
func TestQuarryNoticeRefusalRules(t *testing.T) {
	standardSkip := map[string]any{"title": "Scheduled run skipped", "reason": "skipped_zdr", "run_id": "run_1"}
	titleOnly := map[string]any{"title": "Scheduled run skipped"}
	withURL := map[string]any{"title": "Scheduled run failed", "url": "https://example.com/private"}
	noTitle := map[string]any{"reason": "skipped_zdr"}
	nonString := map[string]any{"title": "Scheduled run failed", "run_id": 7}
	cases := []struct {
		name       string
		notice     notification.Request
		wantStatus int
	}{
		{"standard schedule notice with a closed payload", quarryNotice("quarry.schedule_run_skipped", "standard", "user_123", "evt-1", standardSkip), 0},
		{"schedule notice to another member", quarryNotice("quarry.schedule_run_failed", "standard", "user_456", "evt-1", titleOnly), stdhttp.StatusForbidden},
		{"schedule notice without an idempotency key", quarryNotice("quarry.schedule_run_failed", "standard", "user_123", "", titleOnly), stdhttp.StatusBadRequest},
		{"schedule notice without a retention mode", quarryNotice("quarry.schedule_run_failed", "", "user_123", "evt-1", titleOnly), stdhttp.StatusBadRequest},
		{"standard schedule notice with a URL", quarryNotice("quarry.schedule_run_failed", "standard", "user_123", "evt-1", withURL), stdhttp.StatusBadRequest},
		{"standard schedule notice without a title", quarryNotice("quarry.schedule_run_failed", "standard", "user_123", "evt-1", noTitle), stdhttp.StatusBadRequest},
		{"standard schedule notice with a non-string value", quarryNotice("quarry.schedule_run_failed", "standard", "user_123", "evt-1", nonString), stdhttp.StatusBadRequest},
		{"ZDR schedule notice, title only", quarryNotice("quarry.schedule_run_skipped", "zdr", "user_123", "evt-1", titleOnly), 0},
		{"ZDR schedule notice with a reason", quarryNotice("quarry.schedule_run_skipped", "zdr", "user_123", "evt-1", standardSkip), stdhttp.StatusBadRequest},
		{"change notice, standard", quarryNotice("change_detected", "standard", "user_123", "evt-1", withURL), 0},
		{"change notice, ZDR", quarryNotice("change_detected", "zdr", "user_123", "evt-1", titleOnly), stdhttp.StatusForbidden},
		{"unregistered type", quarryNotice("quarry.schedule_paused", "standard", "user_123", "evt-1", titleOnly), stdhttp.StatusForbidden},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			status, _ := quarryNoticeRefusal(tc.notice, "user_123")
			if status != tc.wantStatus {
				t.Fatalf("status = %d, want %d", status, tc.wantStatus)
			}
		})
	}
	if status, _ := quarryNoticeRefusal(quarryNotice("quarry.schedule_run_failed", "standard", "user_123", "evt-1", titleOnly), ""); status != stdhttp.StatusForbidden {
		t.Fatalf("blank delegated user status = %d, want %d", status, stdhttp.StatusForbidden)
	}
}

// The migration registers exactly the schedule notice types the allowlist admits, and
// its down migration removes them. The files are read from this module's migrations.
func TestQuarryScheduleNoticeMigrationRegistersAdmittedTypes(t *testing.T) {
	up, err := os.ReadFile("../../migrations/016_register_quarry_schedule_notifications.up.sql")
	if err != nil {
		t.Fatalf("read up migration: %v", err)
	}
	down, err := os.ReadFile("../../migrations/016_register_quarry_schedule_notifications.down.sql")
	if err != nil {
		t.Fatalf("read down migration: %v", err)
	}
	for _, notificationType := range []string{"quarry.schedule_run_failed", "quarry.schedule_run_skipped"} {
		if !isNotificationTypeAuthorized("quarry-control", notificationType) {
			t.Fatalf("%q is not admitted by the allowlist", notificationType)
		}
		quoted := "'" + notificationType + "'"
		if !strings.Contains(string(up), quoted) {
			t.Fatalf("up migration does not register %s", notificationType)
		}
		if !strings.Contains(string(down), quoted) {
			t.Fatalf("down migration does not remove %s", notificationType)
		}
	}
	if !strings.Contains(string(up), "ON CONFLICT (org_id, event_type, channel) DO NOTHING") {
		t.Fatal("up migration is not idempotent")
	}
}
