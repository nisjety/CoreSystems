package dbtest

import "testing"

// H5: an empty ANBUD_TEST_DSN fails the database test when ANBUD_REQUIRE_DB_TESTS
// is 1, and skips it otherwise. This closes the silent-skip gap: a run that was
// meant to exercise the database cannot pass by skipping it. The decision is pure.
func TestUnsetDSNFailsOnlyWhenRequired(t *testing.T) {
	cases := []struct {
		name    string
		require string
		want    bool
	}{
		{"required: the test fails", "1", true},
		{"unset: the test skips, as before", "", false},
		{"explicitly off: the test skips", "0", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := unsetDSNFails(c.require); got != c.want {
				t.Errorf("unsetDSNFails(%q) = %v, want %v", c.require, got, c.want)
			}
		})
	}
}
