package subscriber

import "testing"

func TestConsumerDeliveryLimitMatchesDeadLetterThreshold(t *testing.T) {
	t.Parallel()

	if maxConsumerDeliveries != maxDeliveries {
		t.Fatalf(
			"consumer MaxDeliver (%d) must match the handler dead-letter threshold (%d)",
			maxConsumerDeliveries,
			maxDeliveries,
		)
	}
}
