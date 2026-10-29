package main

import (
	"strings"
	"testing"
	"time"
)

var today = time.Date(2026, 10, 28, 15, 4, 5, 0, time.UTC)

func TestAPlainRunIsADryRunAsOfToday(t *testing.T) {
	r, err := parseArgs([]string{"-months", "12"}, today)
	if err != nil {
		t.Fatalf("parseArgs: %v", err)
	}
	if r.apply || r.months != 12 || r.limit != defaultLimit || !r.asOf.Equal(time.Date(2026, 10, 28, 0, 0, 0, 0, time.UTC)) {
		t.Errorf("%+v, want a dry run of 12 months as of 2026-10-28 with the default limit", r)
	}
	if got := requestRef(r); got != "retention:12m:2026-10-28" {
		t.Errorf("request ref %q", got)
	}
}

func TestTheArgumentsAreChecked(t *testing.T) {
	for _, tc := range []struct {
		args []string
		want string
	}{
		{[]string{}, "-months is required"},
		{[]string{"-months", "0"}, "-months is required"},
		{[]string{"-months", "121"}, "between 1 and 120"},
		{[]string{"-months", "-3"}, "between 1 and 120"},
		{[]string{"-months", "12", "-as-of", "28.10.2026"}, "YYYY-MM-DD"},
		{[]string{"-months", "12", "-as-of", "2026-10-29"}, "later than today"},
		{[]string{"-months", "12", "-limit", "0"}, "-limit must be"},
		{[]string{"-months", "12", "extra"}, "usage:"},
		{[]string{"-months", "12", "-unknown"}, "usage:"},
	} {
		_, err := parseArgs(tc.args, today)
		if err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("%q: err %v, want one that says %q", tc.args, err, tc.want)
		}
	}
}

// Nothing reaches the database without a DSN, and the message names the variable.
func TestTheDSNIsRequired(t *testing.T) {
	var out, errOut strings.Builder
	code := cli([]string{"-months", "12"}, func(string) string { return "" }, &out, &errOut)
	if code != exitUsage || !strings.Contains(errOut.String(), envDSN) || out.Len() != 0 {
		t.Errorf("code %d, stderr %q, stdout %q; want exit 1 naming %s and no output", code, errOut.String(), out.String(), envDSN)
	}
}
