package store

import "testing"

func TestNormalizeKind(t *testing.T) {
	t.Parallel()

	cases := []struct {
		in   string
		want string
	}{
		{in: "", want: SourceKindDrive},
		{in: "   ", want: SourceKindDrive},
		{in: "drive", want: SourceKindDrive},
		{in: "site_pages", want: SourceKindSitePages},
		{in: " site_pages ", want: SourceKindSitePages},
		{in: "bogus", want: "bogus"}, // validation is the API layer's job
	}
	for _, tc := range cases {
		if got := NormalizeKind(tc.in); got != tc.want {
			t.Errorf("NormalizeKind(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestNormalizeFolderPath(t *testing.T) {
	t.Parallel()

	cases := []struct {
		in   string
		want string
	}{
		{in: "", want: ""},
		{in: "   ", want: ""},
		{in: "/", want: ""},
		{in: ".", want: ""},
		{in: "Contracts", want: "/Contracts"},
		{in: "/Contracts", want: "/Contracts"},
		{in: "/Contracts/", want: "/Contracts"},
		{in: "Contracts/2026/", want: "/Contracts/2026"},
		{in: "//Contracts//2026", want: "/Contracts/2026"},
		{in: "/Contracts/../Legal", want: "/Legal"},
	}
	for _, tc := range cases {
		if got := NormalizeFolderPath(tc.in); got != tc.want {
			t.Errorf("NormalizeFolderPath(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
