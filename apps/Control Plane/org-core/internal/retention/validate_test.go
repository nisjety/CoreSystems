package retention

import (
	"errors"
	"strings"
	"testing"
)

func intPtr(v int) *int     { return &v }
func revPtr(v int64) *int64 { return &v }

func codesOf(err error) []string {
	var invalid *ValidationError
	if !errors.As(err, &invalid) {
		return nil
	}
	codes := make([]string, 0, len(invalid.Errors))
	for _, e := range invalid.Errors {
		codes = append(codes, e.Code)
	}
	return codes
}

func TestValidatePutBoundsTheWindowAndRequiresTheRevision(t *testing.T) {
	cases := []struct {
		name string
		req  PutRequest
		want []string
	}{
		{"keep", PutRequest{ExpectedRevision: revPtr(0)}, nil},
		{"lower bound", PutRequest{ExpectedRevision: revPtr(3), ThreadDays: intPtr(MinThreadDays)}, nil},
		{"upper bound", PutRequest{ExpectedRevision: revPtr(3), ThreadDays: intPtr(MaxThreadDays)}, nil},
		{"below", PutRequest{ExpectedRevision: revPtr(0), ThreadDays: intPtr(29)}, []string{CodeWindowOutOfRange}},
		{"above", PutRequest{ExpectedRevision: revPtr(0), ThreadDays: intPtr(3651)}, []string{CodeWindowOutOfRange}},
		{"zero", PutRequest{ExpectedRevision: revPtr(0), ThreadDays: intPtr(0)}, []string{CodeWindowOutOfRange}},
		{"negative", PutRequest{ExpectedRevision: revPtr(0), ThreadDays: intPtr(-30)}, []string{CodeWindowOutOfRange}},
		{"no revision", PutRequest{ThreadDays: intPtr(90)}, []string{CodeExpectedRevisionRequired}},
		{"negative revision", PutRequest{ExpectedRevision: revPtr(-1)}, []string{CodeExpectedRevisionRequired}},
		{"both", PutRequest{ThreadDays: intPtr(1)}, []string{CodeExpectedRevisionRequired, CodeWindowOutOfRange}},
	}
	for _, tc := range cases {
		got := codesOf(ValidatePut(tc.req))
		if strings.Join(got, ",") != strings.Join(tc.want, ",") {
			t.Errorf("%s: codes = %v, want %v", tc.name, got, tc.want)
		}
	}
}

func TestNormalizePlaceResolvesTheTargetFromTheScope(t *testing.T) {
	scope, target, ref, err := NormalizePlace("org-1", PlaceHoldRequest{Scope: "org", HoldRef: "  Matter 2026-17 "})
	if err != nil || scope != ScopeOrg || target != "org-1" || ref != "Matter 2026-17" {
		t.Fatalf("an org hold targets the organisation: scope=%q target=%q ref=%q err=%v", scope, target, ref, err)
	}
	scope, target, _, err = NormalizePlace("org-1", PlaceHoldRequest{Scope: "thread", ThreadID: " thr_01H ", HoldRef: "Matter"})
	if err != nil || scope != ScopeThread || target != "thr_01H" {
		t.Fatalf("a thread hold targets the thread: scope=%q target=%q err=%v", scope, target, err)
	}

	refusals := []struct {
		name string
		req  PlaceHoldRequest
		want string
	}{
		{"unknown scope", PlaceHoldRequest{Scope: "space", HoldRef: "m"}, CodeInvalidScope},
		{"org with thread", PlaceHoldRequest{Scope: "org", ThreadID: "t1", HoldRef: "m"}, CodeThreadIDNotAllowed},
		{"thread without id", PlaceHoldRequest{Scope: "thread", HoldRef: "m"}, CodeInvalidThreadID},
		{"thread id with slash", PlaceHoldRequest{Scope: "thread", ThreadID: "t/1", HoldRef: "m"}, CodeInvalidThreadID},
		{"thread id too long", PlaceHoldRequest{Scope: "thread", ThreadID: strings.Repeat("a", MaxThreadIDLength+1), HoldRef: "m"}, CodeInvalidThreadID},
		{"blank reference", PlaceHoldRequest{Scope: "org", HoldRef: "   "}, CodeInvalidHoldRef},
		{"long reference", PlaceHoldRequest{Scope: "org", HoldRef: strings.Repeat("r", MaxHoldRefLength+1)}, CodeInvalidHoldRef},
		{"multi-line reference", PlaceHoldRequest{Scope: "org", HoldRef: "a\nb"}, CodeInvalidHoldRef},
	}
	for _, tc := range refusals {
		_, _, _, err := NormalizePlace("org-1", tc.req)
		if codes := codesOf(err); len(codes) == 0 || codes[0] != tc.want {
			t.Errorf("%s: codes = %v, want %s first", tc.name, codes, tc.want)
		}
	}
}

func TestNormalizeCheckCapsRawEntriesAndDeduplicates(t *testing.T) {
	ids, err := NormalizeCheck(CheckRequest{ThreadIDs: []string{"t2", "t1", "t2"}})
	if err != nil || strings.Join(ids, ",") != "t1,t2" {
		t.Fatalf("ids must be deduplicated in bytewise order: %v err=%v", ids, err)
	}
	if ids, err := NormalizeCheck(CheckRequest{}); err != nil || len(ids) != 0 {
		t.Fatalf("an empty list asks for the window alone: %v err=%v", ids, err)
	}

	same := make([]string, MaxCheckThreadIDs+1)
	for i := range same {
		same[i] = "t1"
	}
	if codes := codesOf(func() error { _, err := NormalizeCheck(CheckRequest{ThreadIDs: same}); return err }()); len(codes) != 1 || codes[0] != CodeTooManyThreadIDs {
		t.Fatalf("the cap counts raw entries: %v", codes)
	}
	// An id is compared as sent: a padded id is refused, not trimmed into a match.
	if _, err := NormalizeCheck(CheckRequest{ThreadIDs: []string{" t1"}}); err == nil {
		t.Fatal("a padded thread id must be refused")
	}
}

func TestEnabledIsStrict(t *testing.T) {
	for value, want := range map[string]bool{"true": true, " true ": false, "": false, "1": false, "TRUE": false, "yes": false, "false": false} {
		got := Enabled(func(string) string { return value })
		if got != want {
			t.Errorf("Enabled(%q) = %v, want %v", value, got, want)
		}
	}
}
