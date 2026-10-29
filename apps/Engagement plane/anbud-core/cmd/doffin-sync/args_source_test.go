package main

import (
	"strings"
	"testing"
	"time"
)

var sourceArgsNow = time.Date(2026, 10, 28, 12, 0, 0, 0, time.UTC)

// -source picks the source and its id shape; -countries belongs to TED only.
func TestTheSourceFlags(t *testing.T) {
	window := []string{"-from", "2026-10-01", "-to", "2026-10-01"}
	for _, tc := range []struct {
		args []string
		want string
	}{
		{append(window, "-source", "tedx"), "-source must be doffin, ted, boamp, fts or bzp"},
		{append(window, "-countries", "SWE"), "-countries applies to -source ted only"},
		{append(window, "-source", "boamp", "-countries", "FRA"), "-countries applies to -source ted only"},
	} {
		if _, err := parseArgs(tc.args, sourceArgsNow); err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("%q: err %v, want %q", tc.args, err, tc.want)
		}
	}

	r, err := parseArgs(append(window, "-source", "ted", "-countries", "SWE,DNK"), sourceArgsNow)
	if err != nil || r.source != "ted" || strings.Join(r.countries, " ") != "SWE DNK" {
		t.Fatalf("ted request %+v, %v", r, err)
	}
	valid := engineOptions(r, nil).ValidID
	if !valid("741144-2026") || valid("2026-116564") {
		t.Error("a TED sync must take TED publication numbers and refuse Doffin ids")
	}

	r, err = parseArgs(append(window, "-source", "boamp"), sourceArgsNow)
	if err != nil || r.source != "boamp" || r.countries != nil {
		t.Fatalf("boamp request %+v, %v", r, err)
	}
	valid = engineOptions(r, nil).ValidID
	if !valid("26-104530") || valid("741144-2026") || valid("2026-116564") {
		t.Error("a BOAMP sync must take BOAMP identifiers and refuse TED and Doffin ids")
	}

	r, err = parseArgs(append(window, "-source", "fts"), sourceArgsNow)
	if err != nil || r.source != "fts" || r.countries != nil {
		t.Fatalf("fts request %+v, %v", r, err)
	}
	valid = engineOptions(r, nil).ValidID
	if !valid("101800-2026") || valid("2026-116564") || valid("ocds-h6vhtk-0791ce") {
		t.Error("a Find a Tender sync must take notice numbers and refuse Doffin ids and OCIDs")
	}

	r, err = parseArgs(append(window, "-source", "bzp"), sourceArgsNow)
	if err != nil || r.source != "bzp" || r.countries != nil {
		t.Fatalf("bzp request %+v, %v", r, err)
	}
	valid = engineOptions(r, nil).ValidID
	if !valid("2026-00505976-01") || !valid("2025-00610043-06-P") || valid("2026-116564") || valid("2026/BZP 00505976/01") {
		t.Error("a BZP sync must take BZP ids and refuse Doffin ids and BZP's own notice-number form")
	}

	r, err = parseArgs(window, sourceArgsNow)
	if err != nil || r.source != "doffin" || r.countries != nil {
		t.Fatalf("default request %+v, %v", r, err)
	}
	valid = engineOptions(r, nil).ValidID
	if !valid("2026-116564") || valid("741144-2026") {
		t.Error("a Doffin sync must take Doffin ids and refuse TED publication numbers")
	}
}
