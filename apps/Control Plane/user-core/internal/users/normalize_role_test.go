package users

import "testing"

func TestNormalizeRole(t *testing.T) {
	cases := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{"view", "view", false},
		{"read", "view", false},
		{"", "view", false}, // empty → least privilege (back-compat)
		{"VIEW", "view", false},
		{" edit ", "edit", false},
		{"write", "edit", false},
		{"admin", "edit", false},
		{"owner", "edit", false},
		{"garbage", "", true},
		{"rdonly", "", true},
	}
	for _, tc := range cases {
		got, err := normalizeRole(tc.in)
		if tc.wantErr {
			if err == nil {
				t.Errorf("normalizeRole(%q): expected error, got %q", tc.in, got)
			}
			continue
		}
		if err != nil {
			t.Errorf("normalizeRole(%q): unexpected error: %v", tc.in, err)
			continue
		}
		if got != tc.want {
			t.Errorf("normalizeRole(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
