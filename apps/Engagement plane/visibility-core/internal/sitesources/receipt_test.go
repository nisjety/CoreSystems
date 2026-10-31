package sitesources

import (
	"encoding/json"
	"strings"
	"testing"
)

func cookieReport(t *testing.T) Report {
	t.Helper()
	_, _, c, _ := fixture(t)
	r := c.report
	r.Provider = "cookiebot"
	r.Algolia = nil
	r.Cookiebot = &Cookiebot{ReportingDomain: "www.triodelab.no", Consents: Consents{Status: "connected", ReportedDays: 7, Totals: &ConsentTotals{OptIn: 10, OptOut: 2, Preferences: 5, Statistics: 4, Marketing: 3}}, Inventory: Inventory{Status: "connected", ScannedAt: "2026-10-22T10:00:00Z", Total: integer(4), Categories: &Categories{Necessary: 1, Preferences: 1, Statistics: 1, Marketing: 1}, ThirdParty: integer(2), Secure: integer(4), HTTPOnly: integer(0)}}
	return r
}
func receipt(r Report) []byte {
	raw, _ := json.Marshal(map[string]any{"success": true, "data": r})
	return raw
}
func TestCookiebotComponentSemanticsAndRequiredScanDate(t *testing.T) {
	r := cookieReport(t)
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	for _, change := range []func(*Report){
		func(r *Report) { r.Cookiebot.Inventory.ScannedAt = "" }, func(r *Report) { r.Cookiebot.Inventory.ScannedAt = "2026-10-24T00:00:00Z" },
		func(r *Report) { r.Cookiebot.Inventory.Categories.Necessary = 10 }, func(r *Report) { r.Cookiebot.Inventory.ThirdParty = integer(5) },
		func(r *Report) { r.Cookiebot.ReportingDomain = "evil.no" }, func(r *Report) { r.Cookiebot.Consents.ReportedDays = 0 },
		func(r *Report) { r.Cookiebot.Consents.Totals.OptIn = 9007199254740992 }, func(r *Report) { r.Cookiebot.Inventory.Total = nil },
	} {
		bad := clone(r)
		change(&bad)
		if _, err := DecodeReceipt(receipt(bad)); err == nil {
			t.Fatal("invalid Cookiebot receipt accepted", bad)
		}
	}
	r.Cookiebot.Consents = Consents{Status: "plan_required", ReportedDays: 0}
	r.Status = "partial"
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	r.Cookiebot.Inventory = Inventory{Status: "access_denied"}
	r.Status = "unavailable"
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	r.Cookiebot.Consents = Consents{Status: "no_data", ReportedDays: 0}
	r.Status = "partial"
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	r.Cookiebot.Inventory = Inventory{Status: "no_data"}
	r.Status = "connected"
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
}
func TestAlgoliaExplicitZeroAndNoDataAreDistinct(t *testing.T) {
	_, _, c, _ := fixture(t)
	r := c.report
	r.Algolia.Analytics = Analytics{Status: "connected", ReportedDays: 0, Searches: integer(0), NoResultSearches: integer(0)}
	raw := receipt(r)
	if !strings.Contains(string(raw), `"noResultRate":null`) {
		t.Fatal(string(raw))
	}
	if _, err := DecodeReceipt(raw); err != nil {
		t.Fatal(err)
	}
	r.Algolia.Analytics = Analytics{Status: "no_data", ReportedDays: 0}
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	r.Algolia.Analytics = Analytics{Status: "unavailable", ReportedDays: 0}
	r.Status = "unavailable"
	if _, err := DecodeReceipt(receipt(r)); err != nil {
		t.Fatal(err)
	}
	r.Algolia.Analytics.Searches = integer(0)
	if _, err := DecodeReceipt(receipt(r)); err == nil {
		t.Fatal("failed component contained fake zero")
	}
}
func TestReceiptRejectsAmbiguousUnexpectedMissingAndNullData(t *testing.T) {
	_, _, c, _ := fixture(t)
	raw := string(receipt(c.report))
	for _, bad := range []string{
		`{}`, `null`, raw + `{}`, strings.Repeat("x", MaxReceiptBytes+1),
		strings.Replace(raw, `"success":true`, `"success":true,"success":true`, 1), strings.Replace(raw, `"success":true`, `"success":false`, 1),
		strings.Replace(raw, `"provider":"algolia"`, `"provider":"algolia","token":"hidden"`, 1), strings.Replace(raw, `"searches":10,`, "", 1),
		strings.Replace(raw, `"searches":10`, `"searches":null`, 1), strings.Replace(raw, `"noResultRate":0.2`, `"noResultRate":0.2,"ip":"hidden"`, 1),
		strings.Replace(raw, `"provider":"algolia"`, `"Provider":"algolia"`, 1), strings.Replace(raw, `"region":"de"`, `"region":"fr"`, 1),
		strings.Replace(raw, `"indexName":"triodelab"`, `"indexName":"unsafe index"`, 1),
	} {
		if _, err := DecodeReceipt([]byte(bad)); err == nil {
			t.Fatal("bad receipt accepted", bad)
		}
	}
}
