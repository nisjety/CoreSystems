package notification

import (
	"testing"
	"time"
)

func TestDeliveryAttemptTransitions(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		from DeliveryAttemptStatus
		to   DeliveryAttemptStatus
		want bool
	}{
		{name: "pending can be claimed", from: DeliveryAttemptPending, to: DeliveryAttemptClaimed, want: true},
		{name: "claim can be submitted", from: DeliveryAttemptClaimed, to: DeliveryAttemptSentUnconfirmed, want: true},
		{name: "submission can be acknowledged", from: DeliveryAttemptSentUnconfirmed, to: DeliveryAttemptAcknowledged, want: true},
		{name: "submission can become unknown", from: DeliveryAttemptSentUnconfirmed, to: DeliveryAttemptUnknown, want: true},
		{name: "unknown can be acknowledged by reconciliation", from: DeliveryAttemptUnknown, to: DeliveryAttemptAcknowledged, want: true},
		{name: "pending can fail", from: DeliveryAttemptPending, to: DeliveryAttemptFailed, want: true},
		{name: "acknowledged is terminal", from: DeliveryAttemptAcknowledged, to: DeliveryAttemptPending, want: false},
		{name: "failed is terminal", from: DeliveryAttemptFailed, to: DeliveryAttemptClaimed, want: false},
		{name: "claimed cannot be acknowledged", from: DeliveryAttemptClaimed, to: DeliveryAttemptAcknowledged, want: false},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			if got := CanTransitionDeliveryAttempt(test.from, test.to); got != test.want {
				t.Fatalf("CanTransitionDeliveryAttempt(%q, %q) = %v, want %v", test.from, test.to, got, test.want)
			}
		})
	}
}

func TestDeliveryAttemptLeaseExpiryIsDeterministic(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC)
	expired := now.Add(-time.Second)
	attempt := DeliveryAttempt{Status: DeliveryAttemptClaimed, LeaseExpiresAt: &expired}
	if !attempt.LeaseExpired(now) {
		t.Fatal("expired claim was considered live")
	}
	live := now.Add(time.Second)
	attempt.LeaseExpiresAt = &live
	if attempt.LeaseExpired(now) {
		t.Fatal("live claim was considered expired")
	}
}

func TestDeliveryAttemptCannotRetryAmbiguousSubmissionWithoutReconciliation(t *testing.T) {
	t.Parallel()

	if CanTransitionDeliveryAttempt(DeliveryAttemptUnknown, DeliveryAttemptClaimed) {
		t.Fatal("unknown provider outcome may not be blindly retried")
	}
	if !CanTransitionDeliveryAttempt(DeliveryAttemptUnknown, DeliveryAttemptAcknowledged) {
		t.Fatal("unknown provider outcome must be settleable by reconciliation")
	}
}
