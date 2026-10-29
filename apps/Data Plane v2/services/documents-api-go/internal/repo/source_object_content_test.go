package repo

import "testing"

func TestSourceObjectContentChanged(t *testing.T) {
	cases := []struct {
		name     string
		inserted bool
		oldHash  string
		newHash  string
		want     bool
	}{
		{"insert is reported via Inserted, not ContentChanged", true, "", "sha1:abc", false},
		{"update with same hash is a no-op", false, "sha1:abc", "sha1:abc", false},
		{"update with changed hash is a content change", false, "sha1:abc", "sha1:def", true},
		{"update from empty to a hash is a content change", false, "", "sha1:abc", true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := sourceObjectContentChanged(tc.inserted, tc.oldHash, tc.newHash)
			if got != tc.want {
				t.Errorf("sourceObjectContentChanged(%v, %q, %q) = %v, want %v",
					tc.inserted, tc.oldHash, tc.newHash, got, tc.want)
			}
		})
	}
}
