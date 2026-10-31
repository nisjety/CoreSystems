package siteaudit

import (
	"regexp"
	"strings"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

type Heading struct {
	Level int    `json:"level"`
	Text  string `json:"text"`
}
type Alternate struct {
	Lang string `json:"lang"`
	URL  string `json:"url"`
}
type Redirect struct {
	URL    string `json:"url"`
	Status int    `json:"status"`
	Target string `json:"target"`
}
type StructuredData struct {
	Documents int      `json:"documents"`
	Invalid   int      `json:"invalid"`
	Types     []string `json:"types"`
	Partial   bool     `json:"partial"`
}
type SEO struct {
	Version              string         `json:"version"`
	Headings             []Heading      `json:"headings"`
	HeadingsPartial      bool           `json:"headingsPartial"`
	Noindex              bool           `json:"noindex"`
	Nofollow             bool           `json:"nofollow"`
	RobotsStatus         int            `json:"robotsStatus"`
	WordCount            int            `json:"wordCount"`
	Alternates           []Alternate    `json:"alternates"`
	InvalidAlternates    int            `json:"invalidAlternates"`
	AlternatesPartial    bool           `json:"alternatesPartial"`
	StructuredData       StructuredData `json:"structuredData"`
	Redirects            []Redirect     `json:"redirects"`
	CanonicalOutsideSite bool           `json:"canonicalOutsideSite"`
}

func copySEO(v *SEO) *SEO {
	if v == nil {
		return nil
	}
	out := *v
	out.Headings = append([]Heading{}, v.Headings...)
	out.Alternates = append([]Alternate{}, v.Alternates...)
	out.Redirects = append([]Redirect{}, v.Redirects...)
	out.StructuredData.Types = append([]string{}, v.StructuredData.Types...)
	return &out
}

func CloneSEO(v *SEO) *SEO { return copySEO(v) }
func validSEO(site string, v *SEO) bool {
	if v == nil {
		return true
	}
	if v.Version != "seo_page_v1" || v.Headings == nil || len(v.Headings) > 200 || v.Alternates == nil || len(v.Alternates) > 50 || v.Redirects == nil || len(v.Redirects) > 3 || v.WordCount < 0 || v.WordCount > 1048576 || v.RobotsStatus != 0 && (v.RobotsStatus < 100 || v.RobotsStatus > 599) || v.InvalidAlternates < 0 || v.InvalidAlternates > 50 || v.StructuredData.Documents < 0 || v.StructuredData.Documents > 32 || v.StructuredData.Invalid < 0 || v.StructuredData.Invalid > v.StructuredData.Documents || v.StructuredData.Types == nil || len(v.StructuredData.Types) > 32 {
		return false
	}
	for _, h := range v.Headings {
		if h.Level < 1 || h.Level > 6 || !safeText(h.Text, 2048) {
			return false
		}
	}
	for _, a := range v.Alternates {
		if !safeText(a.Lang, 40) || a.Lang == "" || !discovery.SafeURL(site, a.URL) {
			return false
		}
	}
	for _, r := range v.Redirects {
		if !discovery.SafeURL(site, r.URL) || !discovery.SafeURL(site, r.Target) || r.Status != 301 && r.Status != 302 && r.Status != 303 && r.Status != 307 && r.Status != 308 {
			return false
		}
	}
	for _, t := range v.StructuredData.Types {
		if !safeText(t, 128) {
			return false
		}
	}
	return true
}

var languagePattern = regexp.MustCompile(`^(?:[a-z]{2,3}(?:-[a-z0-9]{2,8})*|x-default)$`)

// These are structural observations, not Google index status or rich-result
// eligibility. Absence of optional schema/hreflang is inconclusive, not failure.
func seoChecks(p *Page) []Finding {
	v := p.SEO
	if v == nil {
		return nil
	}
	out := []Finding{}
	add := func(rule, status, severity string, related []string) {
		p.Checks = append(p.Checks, Check{RuleID: rule, Status: status})
		if status == "failed" || status == "review" {
			out = append(out, finding(*p, rule, severity, related))
		}
	}
	h1 := 0
	skipped := false
	previous := 0
	for _, h := range v.Headings {
		if h.Level == 1 {
			h1++
		}
		if previous > 0 && h.Level > previous+1 {
			skipped = true
		}
		previous = h.Level
	}
	status := "passed"
	if h1 == 0 {
		status = "failed"
	}
	if v.HeadingsPartial && h1 == 0 {
		status = "inconclusive"
	}
	if h1 > 1 {
		status = "review"
	}
	add("h1_structure", status, "warning", nil)
	status = "passed"
	if skipped {
		status = "review"
	}
	if v.HeadingsPartial && !skipped {
		status = "inconclusive"
	}
	add("heading_sequence", status, "review", nil)
	status = "passed"
	if v.Noindex {
		status = "review"
	}
	add("noindex_directive", status, "review", nil)
	status = "passed"
	if len(v.Redirects) > 1 {
		status = "review"
	}
	add("redirect_chain_review", status, "review", nil)
	status = "inconclusive"
	if v.StructuredData.Documents > 0 && !v.StructuredData.Partial {
		status = "passed"
	}
	if v.StructuredData.Invalid > 0 {
		status = "failed"
	}
	add("structured_data_parse", status, "warning", nil)
	status = "inconclusive"
	langs := map[string]string{}
	invalid := v.InvalidAlternates > 0
	for _, a := range v.Alternates {
		lang := strings.ToLower(a.Lang)
		if !languagePattern.MatchString(lang) || langs[lang] != "" && langs[lang] != a.URL {
			invalid = true
		}
		langs[lang] = a.URL
	}
	if len(v.Alternates) > 0 && !v.AlternatesPartial {
		status = "passed"
	}
	if invalid {
		status = "review"
	}
	add("hreflang_syntax", status, "review", nil)
	status = "passed"
	if v.CanonicalOutsideSite {
		status = "review"
	}
	add("canonical_outside_site", status, "review", nil)
	return out
}

func hreflangReciprocity(p Page, pages []Page) (string, []string) {
	if len(p.SEO.Alternates) == 0 || p.SEO.AlternatesPartial {
		return "inconclusive", nil
	}
	unknown := false
	related := []string{}
	for _, a := range p.SEO.Alternates {
		if a.URL == p.URL || a.URL == p.FinalURL {
			continue
		}
		var target *Page
		for i := range pages {
			if pages[i].Status == "checked" && (pages[i].URL == a.URL || pages[i].FinalURL == a.URL) {
				target = &pages[i]
				break
			}
		}
		if target == nil || target.SEO == nil || target.SEO.AlternatesPartial {
			unknown = true
			continue
		}
		found := false
		for _, back := range target.SEO.Alternates {
			if back.URL == p.URL || back.URL == p.FinalURL {
				found = true
			}
		}
		if !found || target.HTTPStatus >= 400 {
			related = append(related, a.URL)
		}
	}
	if len(related) > 0 {
		return "review", related
	}
	if unknown {
		return "inconclusive", nil
	}
	return "passed", nil
}
