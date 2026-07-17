package billing

import "time"

// PastDueGracePeriod is how long a past_due subscription keeps feature access
// before it is suspended (access denied). The clock starts when the account
// first enters past_due, tracked in Metadata[pastDueSinceMetaKey].
const PastDueGracePeriod = 60 * 24 * time.Hour

const pastDueSinceMetaKey = "past_due_since"

// applyPastDueClock keeps Metadata[past_due_since] consistent with the
// subscription state: it stamps the RFC3339 entry time when the account is
// past_due and the marker is not yet set, and clears the marker once the
// account leaves past_due. Idempotent (never resets a running clock) and
// non-destructive to other metadata keys. Call it on every durable save so the
// grace clock tracks the state regardless of which caller sets past_due.
func applyPastDueClock(account Account, now time.Time) Account {
	_, marked := account.Metadata[pastDueSinceMetaKey]
	if account.SubscriptionState == SubscriptionStatePastDue {
		if marked {
			return account // clock already running; do not reset
		}
		md := cloneMetadata(account.Metadata)
		md[pastDueSinceMetaKey] = now.UTC().Format(time.RFC3339)
		account.Metadata = md
		return account
	}
	if marked {
		md := cloneMetadata(account.Metadata)
		delete(md, pastDueSinceMetaKey)
		account.Metadata = md
	}
	return account
}

// pastDueGraceExpired reports whether a past_due account has exceeded the grace
// period and must be suspended (feature access denied). A missing or
// unparseable marker fails OPEN (treated as still within grace) so a clock that
// never started can never wrongly suspend an org.
func pastDueGraceExpired(account Account, now time.Time) bool {
	if account.SubscriptionState != SubscriptionStatePastDue {
		return false
	}
	raw, ok := account.Metadata[pastDueSinceMetaKey].(string)
	if !ok || raw == "" {
		return false
	}
	since, err := time.Parse(time.RFC3339, raw)
	if err != nil {
		return false
	}
	return now.UTC().Sub(since.UTC()) > PastDueGracePeriod
}

func cloneMetadata(m map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{}, len(m)+1)
	for k, v := range m {
		out[k] = v
	}
	return out
}
