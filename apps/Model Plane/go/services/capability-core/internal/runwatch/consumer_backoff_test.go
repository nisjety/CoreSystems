package runwatch

import (
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

// The redelivery backoff of a transient attention or scheduled failure grows with each
// delivery: 5s, 15s, 45s, then 2m. A delivery past the fourth keeps the 2m step, so a larger
// MaxDeliver cannot make redelivery hot (the durable's MaxDeliver is 5, so its fifth delivery
// is the last JetStream makes), and a count JetStream did not give has no delay.
func TestAttentionBackoffGrowsByDeliveryAndHoldsTheLongestStep(t *testing.T) {
	cases := []struct {
		deliveries uint64
		want       time.Duration
	}{
		{0, 0},
		{1, 5 * time.Second},
		{2, 15 * time.Second},
		{3, 45 * time.Second},
		{4, 2 * time.Minute},
		{5, 2 * time.Minute},
		{9, 2 * time.Minute},
	}
	for _, tc := range cases {
		if got := attentionBackoff(tc.deliveries); got != tc.want {
			t.Fatalf("attentionBackoff(%d) = %v, want %v", tc.deliveries, got, tc.want)
		}
	}
}

// The terminal-run watchers keep the plain Nak they have always had, whatever the delivery.
// Only a transient attention or scheduled failure backs off, and a definitive outcome acks.
func TestSettleKeepsThePlainNakForWatchersAndBacksOffAttentionOnly(t *testing.T) {
	cases := []struct {
		name       string
		outcome    outcome
		deliveries uint64
		want       settlement
	}{
		{"a watcher retry is a plain Nak on the first delivery", outcomeRetry, 1, settlement{}},
		{"a watcher retry is a plain Nak on a later delivery", outcomeRetry, 4, settlement{}},
		{"an attention retry backs off 5s on the first delivery", outcomeRetryLater, 1, settlement{delay: 5 * time.Second}},
		{"an attention retry backs off 15s on the second", outcomeRetryLater, 2, settlement{delay: 15 * time.Second}},
		{"an attention retry backs off 45s on the third", outcomeRetryLater, 3, settlement{delay: 45 * time.Second}},
		{"an attention retry backs off 2m on the fourth", outcomeRetryLater, 4, settlement{delay: 2 * time.Minute}},
		{"the last delivery keeps the longest step", outcomeRetryLater, 5, settlement{delay: 2 * time.Minute}},
		{"an unknown delivery count is a plain Nak", outcomeRetryLater, 0, settlement{}},
		{"a definitive outcome is acked", outcomeAck, 1, settlement{ack: true}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := settle(tc.outcome, tc.deliveries); got != tc.want {
				t.Fatalf("settle(%v, %d) = %+v, want %+v", tc.outcome, tc.deliveries, got, tc.want)
			}
		})
	}
}

// A message without JetStream metadata reports no delivery count, so it is never delayed.
func TestDeliveryCountIsZeroWithoutJetStreamMetadata(t *testing.T) {
	if got := deliveryCount(&nats.Msg{Subject: RunEventSubject, Data: []byte("{}")}); got != 0 {
		t.Fatalf("deliveryCount(plain message) = %d, want 0", got)
	}
}
