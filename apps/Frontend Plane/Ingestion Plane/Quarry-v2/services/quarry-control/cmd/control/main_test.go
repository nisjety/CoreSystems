package main

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func TestEnvOr(t *testing.T) {
	t.Setenv("QUARRY_TEST_ENVOR", "x")
	if got := envOr("QUARRY_TEST_ENVOR", "d"); got != "x" {
		t.Fatalf("envOr returned %q want %q", got, "x")
	}

	t.Setenv("QUARRY_TEST_ENVOR", "")
	if got := envOr("QUARRY_TEST_ENVOR", "d"); got != "d" {
		t.Fatalf("envOr fallback %q want %q", got, "d")
	}
}

// The tombstone receiver and the erasure marker are OFF by default and come on
// together, only with the flag AND an HMAC secret.
func TestZdrTombstoneReceiverPosture(t *testing.T) {
	flag := func(v string) func(string) string {
		return func(k string) string {
			if k == "QUARRY_ZDR_TOMBSTONE_RECEIVER" {
				return v
			}
			return ""
		}
	}
	markerWritten := func(db store.DB) bool {
		now := time.Now().UTC()
		tomb := store.ZdrTombstone{RunID: "run_1", OrgID: "org_a", Kind: "scrape",
			RetentionClass: store.ZdrTombstoneRetentionClass, Outcome: "ok", EndedAt: now}
		if _, err := db.PurgeOrg("org_a"); err != nil {
			t.Fatal(err)
		}
		inserted, err := db.ZdrTombstones().Put(tomb)
		if err != nil {
			t.Fatal(err)
		}
		return !inserted // a refused Put means a marker exists
	}
	cases := []struct {
		name      string
		getenv    func(string) string
		hasSecret bool
		mounted   bool
	}{
		{"default: flag unset", flag(""), true, false},
		{"flag false", flag("false"), true, false},
		{"flag on but no HMAC secret", flag("true"), false, false},
		{"flag on with a secret", flag("true"), true, true},
	}
	for _, c := range cases {
		db := store.NewMemory()
		if got := zdrTombstoneReceiverPosture(c.getenv, c.hasSecret, db); got != c.mounted {
			t.Fatalf("%s: mounted=%v want %v", c.name, got, c.mounted)
		}
		if got := markerWritten(db); got != c.mounted {
			t.Fatalf("%s: erasure marker written=%v want %v", c.name, got, c.mounted)
		}
	}
}
