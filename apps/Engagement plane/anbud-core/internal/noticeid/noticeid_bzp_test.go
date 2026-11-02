package noticeid

import "testing"

func TestBZPIdentifierShape(t *testing.T) {
	t.Parallel()
	for _, id := range []string{"2026-00505976-01", "2025-00610043-06-P"} {
		if !IsBZPID(id) {
			t.Errorf("IsBZPID(%q) = false, want true", id)
		}
	}
	for _, id := range []string{"2026/BZP 00505976/01", "2026-0050597-01", "2026-116564", "26-104530", "2026-00505976-1", "2026-00505976-01-X"} {
		if IsBZPID(id) {
			t.Errorf("IsBZPID(%q) = true, want false", id)
		}
	}
}

// TestBZPPublicIDRoundTrips: the public id carries the prefix, and parsing it gives the source back.
func TestBZPPublicIDRoundTrips(t *testing.T) {
	t.Parallel()
	public := Public(SourceBZP, "2026-00505976-01")
	if public != "BZP-2026-00505976-01" {
		t.Fatalf("Public = %q", public)
	}
	source, id, ok := Parse(public)
	if !ok || source != SourceBZP || id != "2026-00505976-01" {
		t.Errorf("Parse(%q) = %q, %q, %v", public, source, id, ok)
	}
	if !ValidFor(SourceBZP, "2026-00505976-01") || ValidFor(SourceBZP, "101800-2026") {
		t.Error("a BZP sync must take BZP ids and refuse the other sources' shapes")
	}
}
