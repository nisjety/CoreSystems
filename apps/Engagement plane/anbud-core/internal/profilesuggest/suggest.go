// Package profilesuggest drafts a company's tender profile from the awards it has won. The codes are
// the work the company has been awarded, so a new organisation starts from what it has delivered, and
// the user expands the profile from there. Nothing here calls a register or a provider: the awards are
// the ones the notice store already holds.
package profilesuggest

import (
	"sort"
	"strings"

	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/normalize"
)

// MaxCodes bounds each list. A suggestion is a starting point, not a register of everything won.
const MaxCodes = 10

// Source labels say where each field of a suggested profile came from, so the page can show it and a
// later change can tell a suggestion from what the user typed.
const (
	SourceOrganisation = "organisation"
	SourceWins         = "wins"
)

// Suggestion is what an organisation's wins suggest for its profile.
type Suggestion struct {
	OrgNumber string   `json:"org_number"`
	Wins      int      `json:"wins"`
	CPV       []string `json:"cpv"`
	NUTS      []string `json:"nuts"`
}

// FromWins reads the awards the organisation won. An award counts once, and each of its codes counts
// once per award. An award whose winners do not name the organisation counts for nothing.
func FromWins(awards []incumbency.Award, orgNumber string) Suggestion {
	s := Suggestion{OrgNumber: orgNumber, CPV: []string{}, NUTS: []string{}}
	cpv := map[string]int{}
	nuts := map[string]int{}
	for _, a := range awards {
		if !won(a, orgNumber) {
			continue
		}
		s.Wins++
		for _, code := range distinct(mainCodes(a.CPV)) {
			cpv[code]++
		}
		for _, code := range distinct(a.NUTS) {
			nuts[code]++
		}
	}
	s.CPV = top(cpv)
	s.NUTS = top(nuts)
	return s
}

// Profile is the stored profile this suggestion makes, in the shape the tender pages read. Its sources
// say where each list came from. It is not confirmed: the organisation checks it, and saves it, before any
// tender is searched with it. suggestedFrom says how many wins it was drawn from.
func (s Suggestion) Profile(strictness string) map[string]any {
	return map[string]any{
		"orgNumber":     s.OrgNumber,
		"cpv":           s.CPV,
		"nuts":          s.NUTS,
		"keywords":      []string{},
		"strictness":    strictness,
		"confirmed":     false,
		"suggestedFrom": map[string]int{"wins": s.Wins},
		"sources": map[string]string{
			"orgNumber": SourceOrganisation,
			"cpv":       SourceWins,
			"nuts":      SourceWins,
		},
	}
}

func won(a incumbency.Award, orgNumber string) bool {
	for _, w := range a.Winners {
		if n, err := normalize.OrgNumber(w.OrgNumber); err == nil && n == orgNumber {
			return true
		}
	}
	return false
}

// mainCodes keeps the main code of each CPV code: 72212000-7 is 72212000.
func mainCodes(codes []string) []string {
	out := make([]string, 0, len(codes))
	for _, code := range codes {
		out = append(out, strings.SplitN(code, "-", 2)[0])
	}
	return out
}

func distinct(values []string) []string {
	seen := make(map[string]bool, len(values))
	out := make([]string, 0, len(values))
	for _, v := range values {
		if v != "" && !seen[v] {
			seen[v] = true
			out = append(out, v)
		}
	}
	return out
}

// top returns the codes counted most often, ties broken by code, at most MaxCodes of them.
func top(counts map[string]int) []string {
	codes := make([]string, 0, len(counts))
	for code := range counts {
		codes = append(codes, code)
	}
	sort.Slice(codes, func(i, j int) bool {
		if counts[codes[i]] != counts[codes[j]] {
			return counts[codes[i]] > counts[codes[j]]
		}
		return codes[i] < codes[j]
	})
	if len(codes) > MaxCodes {
		codes = codes[:MaxCodes]
	}
	return codes
}
