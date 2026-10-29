package noticeid

import "testing"

func TestBOAMPIdentifierShape(t *testing.T) {
	t.Parallel()
	for _, id := range []string{"26-104530", "25-1234"} {
		if !IsBOAMPID(id) {
			t.Errorf("IsBOAMPID(%q) = false, want true", id)
		}
	}
	for _, id := range []string{"2026-116564", "741144-2026", "26-123", "26-104530x", "26_104530", ""} {
		if IsBOAMPID(id) {
			t.Errorf("IsBOAMPID(%q) = true, want false", id)
		}
	}
}

// TestPrefixKeepsTheOverlappingShapesApart: 26-1234 is a TED publication number and a BOAMP identifier
// both. The public id tells them apart, so each source's copy has its own id in the app.
func TestPrefixKeepsTheOverlappingShapesApart(t *testing.T) {
	t.Parallel()
	ted, boamp := Public(SourceTED, "26-1234"), Public(SourceBOAMP, "26-1234")
	if ted != "TED-26-1234" || boamp != "BOAMP-26-1234" {
		t.Fatalf("public ids %q, %q", ted, boamp)
	}
	for public, want := range map[string]string{ted: SourceTED, boamp: SourceBOAMP} {
		source, id, ok := Parse(public)
		if !ok || source != want || id != "26-1234" {
			t.Errorf("Parse(%q) = %q, %q, %v; want %q, 26-1234, true", public, source, id, ok, want)
		}
	}
}

func TestBOAMPValidForAndParse(t *testing.T) {
	t.Parallel()
	if !ValidFor(SourceBOAMP, "26-104530") || ValidFor(SourceBOAMP, "741144-2026") || ValidFor(SourceBOAMP, "2026-116564") {
		t.Error("a BOAMP sync must take BOAMP identifiers and refuse TED and Doffin ids")
	}
	if got := Public(SourceBOAMP, "26-104530"); got != "BOAMP-26-104530" {
		t.Errorf("Public = %q", got)
	}
	source, id, ok := Parse("BOAMP-26-104530")
	if !ok || source != SourceBOAMP || id != "26-104530" {
		t.Errorf("Parse(BOAMP-26-104530) = %q, %q, %v", source, id, ok)
	}
	if _, _, ok := Parse("BOAMP-2026-116564"); ok {
		t.Error("a BOAMP id with a Doffin shape read as well formed")
	}
	if !Valid("BOAMP-26-104530") || !Valid("TED-741144-2026") || !Valid("2026-116564") {
		t.Error("a well-formed id of some source was refused")
	}
}
