package siem

import "time"

const (
	backoffBase = 5 * time.Second
	backoffMax  = 15 * time.Minute
)

// Backoff is how long to leave a destination alone after consecutive failed
// deliveries: 5 s, doubling, capped at 15 min. It is deterministic on purpose:
// destinations are retried one at a time from one loop, so there is no herd to
// spread with jitter.
func Backoff(consecutiveFailures int) time.Duration {
	if consecutiveFailures < 1 {
		return 0
	}
	delay := backoffBase
	for i := 1; i < consecutiveFailures; i++ {
		delay *= 2
		if delay >= backoffMax {
			return backoffMax
		}
	}
	return delay
}
