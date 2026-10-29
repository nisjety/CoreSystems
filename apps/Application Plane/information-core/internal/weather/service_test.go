package weather

import "testing"

func TestMapCondition(t *testing.T) {
	if got := mapCondition("lightrain"); got != "Rain" {
		t.Fatalf("mapCondition() = %q", got)
	}
}

func TestFirstSegment(t *testing.T) {
	if got := firstSegment("Oslo, Norway"); got != "Oslo" {
		t.Fatalf("firstSegment() = %q", got)
	}
}
