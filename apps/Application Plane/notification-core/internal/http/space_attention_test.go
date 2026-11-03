package http

import (
	"bytes"
	"encoding/json"
	"io"
	stdhttp "net/http"
	"net/http/httptest"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// WS11-S9: the "space-attention" service principal (Convex's out-of-app courier).

const testSpaceAttentionSecret = "space-attention-test-secret-at-least-32-bytes"
const testSupportWorkerSecret = "support-worker-test-secret-at-least-32-bytes"

func newSpaceAttentionVerifier(t *testing.T) *delegation.Verifier {
	t.Helper()
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "notification-core",
		Keys: map[string]string{
			"verevon-gateway": testDelegationSecret,
			"capability-core": testCapabilityCoreDelegationSecret,
			"support-worker":  testSupportWorkerSecret,
			"space-attention": testSpaceAttentionSecret,
		},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	return verifier
}

func spaceAttentionBody(t *testing.T, overrides map[string]any) []byte {
	t.Helper()
	body := map[string]any{
		"organization_id": "org_123",
		"idempotency_key": "evt_1:user_456",
		"recipient":       map[string]any{"kind": "user", "id": "user_456"},
		"type":            "space.mention",
		"retention_mode":  "standard",
		"payload": map[string]any{
			"space_ref":      "space_1",
			"actor_kind":     "user",
			"deep_link_path": "/spaces/space_1?thread_id=thread_1#chat",
		},
	}
	for key, value := range overrides {
		if value == nil {
			delete(body, key)
		} else {
			body[key] = value
		}
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("json.Marshal error = %v", err)
	}
	return encoded
}

// postAs sends the body signed as serviceID and returns the status and body.
func postAs(t *testing.T, serviceID, secret string, body []byte) (int, string) {
	t.Helper()
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Notifications: newTestNotificationService(nil)})
	server := httptest.NewServer(newRouter(handler, newSpaceAttentionVerifier(t)))
	defer server.Close()

	request, err := stdhttp.NewRequest(stdhttp.MethodPost, server.URL+"/api/v1/notification-requests", bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequestAs(t, request, body, serviceID, secret, "", "org_123", "")
	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("POST error = %v", err)
	}
	defer response.Body.Close()
	text, _ := io.ReadAll(response.Body)
	return response.StatusCode, string(text)
}

func TestSpaceAttentionMayNotifyAnOrgUserWithASpaceType(t *testing.T) {
	for _, notificationType := range []string{
		"space.mention", "space.reply", "space.invite", "space.agent_done",
		"space.agent_failed", "space.approval_needed", "space.agent_awaiting", "space.reminder",
	} {
		status, body := postAs(t, "space-attention", testSpaceAttentionSecret, spaceAttentionBody(t, map[string]any{"type": notificationType}))
		if status != stdhttp.StatusAccepted {
			t.Fatalf("%s status = %d, want %d, body = %s", notificationType, status, stdhttp.StatusAccepted, body)
		}
		var payload struct {
			RequestID string `json:"request_id"`
			Status    string `json:"status"`
		}
		if err := json.Unmarshal([]byte(body), &payload); err != nil {
			t.Fatalf("%s: decode %q: %v", notificationType, body, err)
		}
		if payload.RequestID == "" || payload.Status != notification.StatusSubmitted {
			t.Fatalf("%s: response = %+v", notificationType, payload)
		}
	}
}

func TestSpaceAttentionRecipientOtherThanUserIsRefused(t *testing.T) {
	for _, kind := range []string{"topic", "subscriber", "email", ""} {
		body := spaceAttentionBody(t, map[string]any{"recipient": map[string]any{"kind": kind, "id": "user_456"}})
		status, text := postAs(t, "space-attention", testSpaceAttentionSecret, body)
		if status != stdhttp.StatusForbidden {
			t.Fatalf("recipient kind %q status = %d, want 403, body = %s", kind, status, text)
		}
	}
}

func TestSpaceAttentionRequiresStandardRetentionMode(t *testing.T) {
	for _, mode := range []any{"zdr", "ZDR", nil, "", "   "} {
		body := spaceAttentionBody(t, map[string]any{"retention_mode": mode})
		status, text := postAs(t, "space-attention", testSpaceAttentionSecret, body)
		if status != stdhttp.StatusForbidden {
			t.Fatalf("retention_mode %v status = %d, want 403, body = %s", mode, status, text)
		}
	}
	// "standard" in any case is accepted.
	status, text := postAs(t, "space-attention", testSpaceAttentionSecret, spaceAttentionBody(t, map[string]any{"retention_mode": " Standard "}))
	if status != stdhttp.StatusAccepted {
		t.Fatalf("retention_mode Standard status = %d, want 202, body = %s", status, text)
	}
}

func TestSpaceAttentionIsRefusedForNonSpaceTypes(t *testing.T) {
	for _, notificationType := range []string{
		"notification.created", "ticket.assigned", "modelplane.run_completed", "space.unknown", "space.", "mention",
	} {
		body := spaceAttentionBody(t, map[string]any{"type": notificationType})
		status, text := postAs(t, "space-attention", testSpaceAttentionSecret, body)
		if status != stdhttp.StatusForbidden {
			t.Fatalf("type %q status = %d, want 403, body = %s", notificationType, status, text)
		}
	}
}

func TestSpaceAttentionRequestMustMatchTheDelegatedOrganization(t *testing.T) {
	body := spaceAttentionBody(t, map[string]any{"organization_id": "org_other"})
	status, text := postAs(t, "space-attention", testSpaceAttentionSecret, body)
	if status != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want 403, body = %s", status, text)
	}
}

func TestOtherPrincipalsAreRefusedForSpaceTypes(t *testing.T) {
	cases := []struct {
		serviceID, secret string
	}{
		{"verevon-gateway", testDelegationSecret},
		{"capability-core", testCapabilityCoreDelegationSecret},
		{"support-worker", testSupportWorkerSecret},
	}
	for _, tc := range cases {
		status, text := postAs(t, tc.serviceID, tc.secret, spaceAttentionBody(t, nil))
		if status != stdhttp.StatusForbidden {
			t.Fatalf("%s status = %d, want 403, body = %s", tc.serviceID, status, text)
		}
	}
}

func TestSpaceTypeAllowlistIsOnlyForSpaceAttention(t *testing.T) {
	for _, notificationType := range []string{
		"space.mention", "space.reply", "space.invite", "space.agent_done",
		"space.agent_failed", "space.approval_needed", "space.agent_awaiting", "space.reminder",
	} {
		if !isNotificationTypeAuthorized("space-attention", notificationType) {
			t.Fatalf("space-attention type %q rejected, want authorized", notificationType)
		}
		for _, other := range []string{"verevon-gateway", "capability-core", "support-worker", "insight-core", "", "unknown"} {
			if isNotificationTypeAuthorized(other, notificationType) {
				t.Fatalf("%q may request %q", other, notificationType)
			}
		}
	}
	for _, notificationType := range []string{"notification.created", "ticket.assigned", "modelplane.run_failed", "sla.breach"} {
		if isNotificationTypeAuthorized("space-attention", notificationType) {
			t.Fatalf("space-attention may request the unrelated type %q", notificationType)
		}
	}
}
