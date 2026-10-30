package match

import (
	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
	"testing"
	"time"
)

func TestProfessionalContextRanksButCannotAdmitOrPassEligibility(t *testing.T) {
	now := time.Date(2026, 10, 29, 8, 0, 0, 0, time.UTC)
	future := now.Add(48 * time.Hour)
	n := eforms.Notice{Title: "Cybersecurity service", Deadline: &future, CPVMain: []string{"72000000"}}
	p := Profile{CPV: []string{"03000000"}, UserKeywords: []string{"cybersecurity"}}
	assessment := Assess(n, normalize.Notice(n), p, Balanced, now)
	if assessment.Band != None || ContextRank(assessment) != 3 {
		t.Fatalf("work role admitted a company mismatch: %+v", assessment)
	}
	p.SEOKeywords = []string{"cybersecurity"}
	assessment = Assess(n, normalize.Notice(n), p, Balanced, now)
	if assessment.Band == None {
		t.Fatal("company SEO did not provide a scope signal")
	}
	p.Blocked = true
	assessment = Assess(n, normalize.Notice(n), p, Balanced, now)
	if assessment.Verdict != Fail {
		t.Fatal("advisory terms overrode a failed eligibility check")
	}
}

func TestContextKeywordsUseUnicodeWordBoundaries(t *testing.T) {
	n := eforms.Notice{Title: "Digital maintenance miljøanalyse"}
	if contextKeywordCheck(n, []string{"IT", "miljø"}, "user_focus").Outcome == Pass {
		t.Fatal("substring false positive")
	}
	if contextKeywordCheck(n, []string{"digital maintenance"}, "company_seo").Outcome != Pass {
		t.Fatal("exact source phrase missing")
	}
}

func TestShortCompanyKeywordDoesNotMatchInsideUnrelatedWords(t *testing.T) {
	p := Profile{Keywords: []string{"IT"}}
	if keywordCheck(eforms.Notice{Title: "Activities and maintenance"}, p).Outcome == Pass {
		t.Fatal("IT matched an unrelated word")
	}
	if keywordCheck(eforms.Notice{Title: "IT-drift og support"}, p).Outcome != Pass {
		t.Fatal("IT service did not match")
	}
}
