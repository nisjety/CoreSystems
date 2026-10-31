package models

import "testing"

// runOriginVectors are the known answers shared with the Rust twin
// (mp-contracts/src/run_origin.rs `run_origin_table`): the two tables are kept
// identical by hand, so a token that means one thing to the writer and another
// to a reader fails one of the two suites.
var runOriginVectors = []struct {
	origin string
	token  string
}{
	{RunOriginChat, "chat"},
	{RunOriginSchedule, "schedule"},
	{RunOriginTrigger, "trigger"},
	{RunOriginAPI, "api"},
}

func TestRunOriginKnownAnswers(t *testing.T) {
	t.Parallel()

	for _, v := range runOriginVectors {
		got, ok := ParseRunOrigin(v.token)
		if !ok || got != v.origin {
			t.Errorf("ParseRunOrigin(%q) = %q, %v; want %q, true", v.token, got, ok, v.origin)
		}
	}
}

func TestParseRunOriginRefusesEverythingOutsideTheClosedSet(t *testing.T) {
	t.Parallel()

	for _, bad := range []string{"", " schedule", "schedule ", "Schedule", "scheduled", "task-runner", "mail_task", "space"} {
		if got, ok := ParseRunOrigin(bad); ok {
			t.Errorf("ParseRunOrigin(%q) = %q, true; want refused", bad, got)
		}
	}
}

func TestRunOriginFromMetadata(t *testing.T) {
	t.Parallel()

	if got, ok := RunOriginFromMetadata(map[string]any{}); !ok || got != RunOriginChat {
		t.Errorf("absent stamp = %q, %v; want chat, true", got, ok)
	}
	if got, ok := RunOriginFromMetadata(map[string]any{RunOriginMetadataKey: nil}); !ok || got != RunOriginChat {
		t.Errorf("null stamp = %q, %v; want chat, true", got, ok)
	}
	if got, ok := RunOriginFromMetadata(map[string]any{RunOriginMetadataKey: "schedule"}); !ok || got != RunOriginSchedule {
		t.Errorf("schedule stamp = %q, %v; want schedule, true", got, ok)
	}
	for _, bad := range []any{"sheduled", 1, true} {
		if got, ok := RunOriginFromMetadata(map[string]any{RunOriginMetadataKey: bad}); ok {
			t.Errorf("stamp %v = %q, true; want refused", bad, got)
		}
	}
}
