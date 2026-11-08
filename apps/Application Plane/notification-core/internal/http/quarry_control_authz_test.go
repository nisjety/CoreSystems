package http

import "testing"

// Quarry sends three notification types and no others: the change monitor's change
// notice and the schedule runner's failed and skipped notices. Each reaches only the
// delegated user (see quarryNoticeRefusal). The delegation key is provisioned
// separately (NOTIFICATION_QUARRY_CONTROL_SERVICE_TOKEN).
func TestQuarryControlMayNotifyOnlyItsRegisteredTypes(t *testing.T) {
	for _, notificationType := range []string{
		"change_detected",
		"quarry.schedule_run_failed",
		"quarry.schedule_run_skipped",
	} {
		if !isNotificationTypeAuthorized("quarry-control", notificationType) {
			t.Fatalf("quarry-control must be authorized for %q", notificationType)
		}
	}
	for _, notificationType := range []string{
		"notification.created",
		"ticket.assigned",
		"modelplane.run_completed",
		"space.mention",
		"change_unchanged",
		"quarry.schedule_paused",
	} {
		if isNotificationTypeAuthorized("quarry-control", notificationType) {
			t.Fatalf("quarry-control may not send %q", notificationType)
		}
	}
	if isNotificationTypeAuthorized("verevon-gateway", "change_detected") {
		t.Fatal("the gateway may not send change_detected")
	}
	if isNotificationTypeAuthorized("verevon-gateway", "quarry.schedule_run_failed") {
		t.Fatal("the gateway may not send quarry schedule notices")
	}
}
