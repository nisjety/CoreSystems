package siteaudit

import "testing"

func goodSEO() *SEO {
	return &SEO{Version: "seo_page_v1", Headings: []Heading{{1, "Title"}, {2, "Section"}}, Alternates: []Alternate{}, Redirects: []Redirect{}, StructuredData: StructuredData{Types: []string{}}}
}
func statuses(p Page) map[string]string {
	out := map[string]string{}
	for _, c := range p.Checks {
		out[c.RuleID] = c.Status
	}
	return out
}
func TestStructuralChecksRequireObservedEvidence(t *testing.T) {
	p := testPage(testSite, 200, "Home")
	p.SEO = goodSEO()
	p.SEO.Noindex = true
	p.SEO.Headings = []Heading{{2, "Section"}, {4, "Skip"}}
	p.SEO.StructuredData = StructuredData{Documents: 1, Invalid: 1, Types: []string{}}
	p.SEO.CanonicalOutsideSite = true
	pages, findings := evaluate([]Page{p})
	c := statuses(pages[0])
	for rule, want := range map[string]string{"h1_structure": "failed", "heading_sequence": "review", "noindex_directive": "review", "structured_data_parse": "failed", "hreflang_syntax": "inconclusive", "hreflang_reciprocity": "inconclusive", "canonical_outside_site": "review"} {
		if c[rule] != want {
			t.Errorf("%s: %s", rule, c[rule])
		}
	}
	if len(findings) < 5 {
		t.Fatal(findings)
	}
	p.SEO.HeadingsPartial = true
	pages, _ = evaluate([]Page{p})
	if statuses(pages[0])["h1_structure"] != "inconclusive" {
		t.Fatal("truncated headings falsely failed")
	}
	p.SEO = nil
	pages, _ = evaluate([]Page{p})
	if _, ok := statuses(pages[0])["h1_structure"]; ok {
		t.Fatal("invented checks for legacy capture")
	}
}
func TestHreflangReturnRequiresCheckedTarget(t *testing.T) {
	p := testPage(testSite, 200, "Home")
	p.SEO = goodSEO()
	p.SEO.Alternates = []Alternate{{"en", testSite + "en"}}
	pages, _ := evaluate([]Page{p})
	if statuses(pages[0])["hreflang_reciprocity"] != "inconclusive" {
		t.Fatal("unvisited target treated as evidence")
	}
	q := testPage(testSite+"en", 200, "English")
	q.SEO = goodSEO()
	pages, _ = evaluate([]Page{p, q})
	if statuses(pages[0])["hreflang_reciprocity"] != "review" {
		t.Fatal("missing return not detected")
	}
	q.SEO.Alternates = []Alternate{{"nb", testSite}}
	pages, _ = evaluate([]Page{p, q})
	if statuses(pages[0])["hreflang_reciprocity"] != "passed" {
		t.Fatal("observed return missing")
	}
	q.SEO.AlternatesPartial = true
	pages, _ = evaluate([]Page{p, q})
	if statuses(pages[0])["hreflang_reciprocity"] != "inconclusive" {
		t.Fatal("truncated target returned false proof")
	}
}
func TestSEOValidationRejectsForeignOrUnboundedEvidence(t *testing.T) {
	for _, change := range []func(*SEO){func(v *SEO) { v.Headings[0].Level = 7 }, func(v *SEO) { v.Alternates = []Alternate{{"en", "https://other.example/"}} }, func(v *SEO) { v.Redirects = []Redirect{{testSite, 200, testSite + "a"}} }, func(v *SEO) { v.StructuredData.Invalid = 2 }, func(v *SEO) { v.Headings = nil }} {
		v := goodSEO()
		change(v)
		if validSEO(testSite, v) {
			t.Fatal(v)
		}
	}
	if !validSEO(testSite, goodSEO()) || !validSEO(testSite, nil) {
		t.Fatal("valid/legacy evidence rejected")
	}
}
