package demo

import (
	"testing"
	"time"
)

// A title written with an en dash is found by a hyphen, and the other way round; so is one with a
// no-break space or a doubled space. Found on 2026-10-28: "Applikasjonssikkerhetssystem - F5" found
// nothing while the title held "–".
func TestTheCatalogueSearchTreatsEveryDashAndSpaceAlike(t *testing.T) {
	n := Result{Title: "Applikasjonssikkerhetssystem – F5", Buyer: "Norsk Tipping"}
	for _, query := range []string{
		"Applikasjonssikkerhetssystem – F5",
		"Applikasjonssikkerhetssystem - F5",
		"applikasjonssikkerhetssystem — f5",
		"sikkerhetssystem − F5",
		"Applikasjonssikkerhetssystem\u00a0-\u00a0F5",
		"Applikasjonssikkerhetssystem  -  F5",
		"  norsk tipping ",
	} {
		if !catalogueMatches(n, CatalogueRequest{Query: query}, time.Time{}) {
			t.Errorf("%q did not find %q", query, n.Title)
		}
	}
	for _, query := range []string{"Applikasjonssikkerhetssystem F5", "sikkerhet-system"} {
		if catalogueMatches(n, CatalogueRequest{Query: query}, time.Time{}) {
			t.Errorf("%q found %q: a dash is folded, not dropped", query, n.Title)
		}
	}
	hyphenated := Result{Title: "Drift - fagsystem"}
	if !catalogueMatches(hyphenated, CatalogueRequest{Query: "drift – fagsystem"}, time.Time{}) {
		t.Error("an en dash in the query did not find a hyphen in the title")
	}
}
