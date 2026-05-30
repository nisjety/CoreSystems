package store

import "testing"

func TestSplitContentHash(t *testing.T) {
	t.Parallel()

	cases := []struct {
		in       string
		wantKind string
		wantVal  string
	}{
		{"sha1:abc", "sha1", "abc"},
		{"quickxor:zzz", "quickxor", "zzz"},
		{"justavalue", "", "justavalue"},
		{":empty", "", "empty"},
	}
	for _, tc := range cases {
		k, v := splitContentHash(tc.in)
		if k != tc.wantKind || v != tc.wantVal {
			t.Errorf("splitContentHash(%q) = (%q,%q), want (%q,%q)", tc.in, k, v, tc.wantKind, tc.wantVal)
		}
	}
}
