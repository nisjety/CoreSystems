package pagespeed

import (
	"strings"
	"testing"
)

func TestPageSpeedCarriesOnlyValidatedRedirectOrigin(t *testing.T) {
	got, reason := decodeReport([]byte(fixture), "https://aquatiq.com/", "mobile")
	if reason != "" || got == nil || got.ReportingOrigin != "https://www.aquatiq.com" {
		t.Fatal("actual Lighthouse origin missing", reason)
	}
	for _, target := range []string{"https://other.no/", "https://blog.aquatiq.com/", "https://user:private@www.aquatiq.com/", "http://www.aquatiq.com/"} {
		raw := strings.Replace(fixture, "https://www.aquatiq.com/no", target, 1)
		if _, reason := decodeReport([]byte(raw), "https://aquatiq.com/", "mobile"); reason != "audit_failed" {
			t.Fatal("unrelated final origin accepted", target)
		}
	}
}
