package pulse

import "testing"

func TestGSCMatchesOnlyAccessibleExactWWWPair(t *testing.T) {
	for _, pair := range [][2]string{{"https://www.triodelab.no/", "sc-domain:triodelab.no"}, {"https://triodelab.no/", "https://www.triodelab.no/"}, {"https://www.triodelab.no/", "https://triodelab.no/"}} {
		if !GSCPropertyForSiteAllowed(pair[1], pair[0]) {
			t.Fatal("matching property rejected", pair)
		}
	}
	for _, property := range []string{"sc-domain:other.no", "https://blog.triodelab.no/", "https://triodelab.no:443/", "https://triodelab.no/path"} {
		if GSCPropertyForSiteAllowed(property, "https://triodelab.no/") {
			t.Fatal("unrelated property accepted", property)
		}
	}
}
