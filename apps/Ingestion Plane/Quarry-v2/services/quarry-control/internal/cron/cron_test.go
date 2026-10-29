package cron

import "testing"

func TestValidate_Valid(t *testing.T) {
	cases := []string{
		"* * * * *",
		"*/5 * * * *",
		"0 9-17 * * 1-5",
		"0,30 * * * *",
		"15 0 1 1 0",
		"0-59 0-23 1-31 1-12 0-6",
		"*/15 */2 * * *",
		"0 0-12/3 * * *",
		"59 23 31 12 6",
	}
	for _, c := range cases {
		if err := Validate(c); err != nil {
			t.Errorf("Validate(%q) = %v, want nil", c, err)
		}
	}
}

func TestValidate_Invalid(t *testing.T) {
	cases := []struct {
		expr string
	}{
		{""},
		{"* * * *"},      // 4 fields
		{"* * * * * *"},  // 6 fields
		{"@hourly"},      // alias
		{"60 * * * *"},   // minute OOR
		{"* 24 * * *"},   // hour OOR
		{"* * 0 * *"},    // dom OOR low
		{"* * 32 * *"},   // dom OOR high
		{"* * * 13 *"},   // month OOR
		{"* * * * 7"},    // dow OOR
		{"* * * * L"},    // unsupported
		{"? * * * *"},    // unsupported
		{"*/0 * * * *"},  // step zero
		{"*/-1 * * * *"}, // negative step
		{"5-2 * * * *"},  // inverted range
		{"abc * * * *"},  // not a number
		{"1,,2 * * * *"}, // empty list elem
		{"5/2 * * * *"},  // step without wildcard/range
	}
	for _, c := range cases {
		if err := Validate(c.expr); err == nil {
			t.Errorf("Validate(%q) = nil, want error", c.expr)
		}
	}
}
