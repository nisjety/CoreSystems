package http

import "testing"

// Quarry's change monitor may send exactly one notification type, and no other service
// may send it. The delegation keys are provisioned separately (NOTIFICATION_QUARRY_CONTROL_SERVICE_TOKEN).
func TestQuarryControlMayNotifyOnlyChangeDetected(t *testing.T) {
	if !isNotificationTypeAuthorized("quarry-control", "change_detected") {
		t.Fatal("quarry-control must be authorized for change_detected")
	}
	for _, notificationType := range []string{
		"notification.created",
		"ticket.assigned",
		"modelplane.run_completed",
		"space.mention",
		"change_unchanged",
	} {
		if isNotificationTypeAuthorized("quarry-control", notificationType) {
			t.Fatalf("quarry-control may not send %q", notificationType)
		}
	}
	if isNotificationTypeAuthorized("verevon-gateway", "change_detected") {
		t.Fatal("the gateway may not send change_detected")
	}
}
