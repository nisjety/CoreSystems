package discovery

import "testing"

func TestDiscoveryPreservesExactPublicWWWPairWithoutRewritingURLs(t *testing.T) {
	for _, site := range []string{"https://triodelab.no/", "https://www.triodelab.no/"} {
		for _, target := range []string{"https://triodelab.no/innsikt", "https://www.triodelab.no/innsikt"} {
			if !SafeURL(site, target) {
				t.Fatalf("paired public URL rejected: %s %s", site, target)
			}
			pages := sanitizePages(site, []Page{{URL: target, Title: "Observed label"}})
			if len(pages) != 1 || pages[0].URL != target {
				t.Fatal("captured URL was discarded or rewritten")
			}
		}
		for _, target := range []string{"https://news.triodelab.no/x", "https://www.www.triodelab.no/x", "https://triodelab.no.evil.example/x", "https://www.triodelab.no:444/x", "https://www.triodelab.no/admin", "https://www.triodelab.no/x?secret=y"} {
			if SafeURL(site, target) {
				t.Fatalf("out-of-scope URL accepted: %s", target)
			}
		}
	}
}
