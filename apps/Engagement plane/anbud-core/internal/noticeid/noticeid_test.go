package noticeid

import "testing"

func TestPublicIDsRoundTripAndSayTheirSource(t *testing.T) {
	for _, tc := range []struct{ source, id, public string }{
		{SourceDoffin, "2026-116564", "2026-116564"},
		{SourceTED, "741144-2026", "TED-741144-2026"},
		// The overlap the prefix exists for: the same digits, two sources, two public ids.
		{SourceDoffin, "1234-2026", "1234-2026"},
		{SourceTED, "1234-2026", "TED-1234-2026"},
	} {
		if got := Public(tc.source, tc.id); got != tc.public {
			t.Errorf("Public(%s, %s) = %q, want %q", tc.source, tc.id, got, tc.public)
		}
		source, id, ok := Parse(tc.public)
		if !ok || source != tc.source || id != tc.id {
			t.Errorf("Parse(%q) = %s, %s, %v; want %s, %s, true", tc.public, source, id, ok, tc.source, tc.id)
		}
		if !ValidFor(tc.source, tc.id) {
			t.Errorf("ValidFor(%s, %s) = false", tc.source, tc.id)
		}
	}
}

func TestMalformedIDsAreRefused(t *testing.T) {
	for _, public := range []string{"", "TED-", "TED-2026-741144x", "TED-123456789-2026", "ted-741144-2026", "741144-26", "2026-1", "TED-741144-2026/../x"} {
		if Valid(public) {
			t.Errorf("Valid(%q) = true", public)
		}
	}
	if ValidFor("other", "2026-116564") || ValidFor(SourceTED, "2026-116564x") {
		t.Error("an unknown source or a malformed TED number was accepted")
	}
}
