package org

import "testing"

func TestPlanAllowsZeroDataRetention(t *testing.T) {
	cases := []struct {
		plan string
		want bool
	}{
		{"pro", true},
		{"enterprise", true},
		{"Enterprise", true},   // case-insensitive
		{"  pro  ", true},      // trimmed
		{"free", false},
		{"trial", false},
		{"hobby", false},
		{"standard", false},
		{"", false},
		{"unknown", false},
	}
	for _, c := range cases {
		if got := PlanAllowsZeroDataRetention(c.plan); got != c.want {
			t.Errorf("PlanAllowsZeroDataRetention(%q) = %v, want %v", c.plan, got, c.want)
		}
	}
}
