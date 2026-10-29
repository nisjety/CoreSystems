package billing

import (
	"testing"
	"time"
)

func TestPastDueGraceExpired(t *testing.T) {
	now := time.Date(2026, 7, 17, 12, 0, 0, 0, time.UTC)
	mk := func(state SubscriptionState, since *time.Time) Account {
		md := map[string]interface{}{}
		if since != nil {
			md[pastDueSinceMetaKey] = since.UTC().Format(time.RFC3339)
		}
		return Account{SubscriptionState: state, Metadata: md}
	}
	d61 := now.Add(-61 * 24 * time.Hour)
	d59 := now.Add(-59 * 24 * time.Hour)

	cases := []struct {
		name string
		acct Account
		want bool
	}{
		{"active never expires", mk(SubscriptionStateActive, &d61), false},
		{"past_due within grace (59d)", mk(SubscriptionStatePastDue, &d59), false},
		{"past_due beyond grace (61d)", mk(SubscriptionStatePastDue, &d61), true},
		{"past_due no marker fails open", mk(SubscriptionStatePastDue, nil), false},
		{"past_due unparseable marker fails open", Account{
			SubscriptionState: SubscriptionStatePastDue,
			Metadata:          map[string]interface{}{pastDueSinceMetaKey: "not-a-time"},
		}, false},
	}
	for _, c := range cases {
		if got := pastDueGraceExpired(c.acct, now); got != c.want {
			t.Errorf("%s: pastDueGraceExpired = %v, want %v", c.name, got, c.want)
		}
	}
}

func TestApplyPastDueClock(t *testing.T) {
	now := time.Date(2026, 7, 17, 12, 0, 0, 0, time.UTC)

	// entering past_due stamps the marker
	a := applyPastDueClock(Account{SubscriptionState: SubscriptionStatePastDue}, now)
	if got, _ := a.Metadata[pastDueSinceMetaKey].(string); got != now.Format(time.RFC3339) {
		t.Fatalf("expected stamped marker %q, got %q", now.Format(time.RFC3339), got)
	}

	// a running clock is not reset on a later save
	earlier := now.Add(-10 * 24 * time.Hour).Format(time.RFC3339)
	a2 := applyPastDueClock(Account{
		SubscriptionState: SubscriptionStatePastDue,
		Metadata:          map[string]interface{}{pastDueSinceMetaKey: earlier},
	}, now)
	if got, _ := a2.Metadata[pastDueSinceMetaKey].(string); got != earlier {
		t.Fatalf("running clock reset: got %q want %q", got, earlier)
	}

	// leaving past_due clears the marker but preserves other metadata
	a3 := applyPastDueClock(Account{
		SubscriptionState: SubscriptionStateActive,
		Metadata:          map[string]interface{}{pastDueSinceMetaKey: earlier, "keep": "me"},
	}, now)
	if _, ok := a3.Metadata[pastDueSinceMetaKey]; ok {
		t.Fatal("marker not cleared on leaving past_due")
	}
	if a3.Metadata["keep"] != "me" {
		t.Fatal("unrelated metadata was dropped")
	}
}
