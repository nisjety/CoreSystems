package match

import (
	"github.com/triodelab/anbud/internal/eforms"
	"strings"
	"unicode"
)

// Context is advisory relevance only. Professional fields never change eligibility
// or admit a tender by themselves. Terms use Unicode word boundaries, not substring
// matches (IT must not match "maintenance").
func contextTokens(text string) []string {
	return strings.FieldsFunc(strings.ToLower(text), func(r rune) bool { return !unicode.IsLetter(r) && !unicode.IsNumber(r) })
}
func contextKeywordCheck(n eforms.Notice, terms []string, rule string) Check {
	check := Check{Rule: rule, Outcome: Unknown, Detail: "no current context keywords available", Cite: Cite{Field: "title/description/lot title"}}
	if len(terms) == 0 {
		return check
	}
	text := n.Title + "\n" + n.Description
	for _, lot := range n.Lots {
		text += "\n" + lot.Title
	}
	haystack := " " + strings.Join(contextTokens(text), " ") + " "
	found := []string{}
	for _, term := range terms {
		tokens := contextTokens(term)
		if len(tokens) == 0 {
			continue
		}
		if strings.Contains(haystack, " "+strings.Join(tokens, " ")+" ") {
			found = append(found, term)
		}
	}
	if len(found) == 0 {
		check.Outcome = Fail
		check.Detail = "no context keyword found"
		return check
	}
	check.Outcome = Pass
	check.Detail = "matching context terms: " + strings.Join(found, ", ")
	return check
}

// ContextRank breaks ties within the same eligibility and company-fit band.
func ContextRank(a Assessment) int {
	score := 0
	for _, c := range a.Fit {
		if c.Outcome != Pass {
			continue
		}
		switch c.Rule {
		case "company_seo":
			score += 10
		case "user_focus":
			score += 3
		}
	}
	return score
}
