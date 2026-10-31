package org

import "testing"

func TestNormalizeWebsiteKeepsAWebAddressAndLowerCasesItsSchemeAndHost(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"https://Firma.NO/Om-oss":             "https://firma.no/Om-oss",
		"  http://firma.no  ":                 "http://firma.no",
		"https://firma.no:8443/x":             "https://firma.no:8443/x",
		"https://www.firma-as.no/kontakt?x=1": "https://www.firma-as.no/kontakt?x=1",
	}
	for raw, want := range cases {
		got, err := NormalizeWebsite(raw)
		if err != nil || got != want {
			t.Errorf("NormalizeWebsite(%q) = %q, %v; want %q", raw, got, err, want)
		}
	}
}

func TestNormalizeWebsiteRefusesAnythingThatIsNotAWebAddress(t *testing.T) {
	t.Parallel()

	refused := []string{
		"",
		"firma.no",
		"ftp://firma.no",
		"javascript:alert(1)",
		"https://user:secret@firma.no",
		"https://firma.no/#fragment",
		"https://localhost",
		"https://192.168.1.10",
		"https://[::1]/",
		"https://.firma.no",
		"https://firma.no/with space",
		"https://firma.no/\nX",
		"https://" + longHost(),
	}
	for _, raw := range refused {
		if got, err := NormalizeWebsite(raw); err != ErrInvalidWebsite {
			t.Errorf("NormalizeWebsite(%q) = %q, %v; want ErrInvalidWebsite", raw, got, err)
		}
	}
}

func longHost() string {
	host := ""
	for len(host) < maxWebsiteLength {
		host += "aaaaaaaaaa."
	}
	return host + "no"
}
