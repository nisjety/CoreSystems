package store

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

// TestJob_MarshalJSON_CreatedAtIsRFC3339 is a regression test for the
// Ingestions-page crash: quarry-edge's `forward_list::<JobSummary>`
// (crates/quarry-edge/src/resource_routes.rs) decodes every
// `GET /v1/{kind}/jobs` (and `GET /v1/jobs`) response into
// `quarry_core::resources::JobSummary`, whose `created_at` field is a
// `chrono::DateTime<Utc>` — serde only accepts that as an RFC3339
// string. Job.CreatedAt is stored internally as a Unix-millis int64;
// without a custom MarshalJSON, encoding/json emitted that int64 as a
// bare JSON number, which quarry-edge failed to parse with "invalid
// type: integer `1784539125221`, expected an RFC 3339 formatted date
// and time string". This test pins the wire shape so a regression is
// caught here instead of at the SPA.
func TestJob_MarshalJSON_CreatedAtIsRFC3339(t *testing.T) {
	t.Parallel()

	millis := int64(1784539125221) // the exact value from the reported crash
	j := Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		Kind:      "crawl",
		Status:    "accepted",
		CreatedAt: millis,
	}

	raw, err := json.Marshal(j)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var asMap map[string]any
	if err := json.Unmarshal(raw, &asMap); err != nil {
		t.Fatalf("unmarshal into map: %v", err)
	}

	createdAt, ok := asMap["created_at"].(string)
	if !ok {
		t.Fatalf("created_at is not a JSON string (got %T = %v) — quarry-edge's "+
			"JobSummary.created_at (DateTime<Utc>) will fail to deserialize it",
			asMap["created_at"], asMap["created_at"])
	}

	parsed, err := time.Parse(time.RFC3339Nano, createdAt)
	if err != nil {
		t.Fatalf("created_at %q is not RFC3339: %v", createdAt, err)
	}
	if got := parsed.UnixMilli(); got != millis {
		t.Fatalf("created_at round-trips to %d millis, want %d", got, millis)
	}
}

// TestJob_JSON_RoundTrip verifies UnmarshalJSON (MarshalJSON's inverse)
// recovers the exact CreatedAt millis after a Marshal/Unmarshal cycle,
// so any current or future code that decodes a Job from JSON keeps
// working now that the wire format is a string.
func TestJob_JSON_RoundTrip(t *testing.T) {
	t.Parallel()

	want := Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		Kind:      "crawl",
		Status:    "running",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: time.Now().UnixMilli(),
	}

	raw, err := json.Marshal(want)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var got Job
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if got.ID != want.ID || got.Kind != want.Kind || got.Status != want.Status {
		t.Fatalf("round-trip mismatch: got %+v, want %+v", got, want)
	}
	if got.CreatedAt != want.CreatedAt {
		t.Fatalf("created_at round-trip = %d, want %d", got.CreatedAt, want.CreatedAt)
	}
}
