package siteaudit

import "slices"

// When a referenced full report remains available, compact provenance must
// agree exactly with that report. Eviction does not fabricate missing proof;
// the protected compact record remains the explicitly retained projection.
func validateInsightProofs(state diskState) error {
	for _, ledger := range state.Insights {
		for _, run := range state.Runs {
			if run.ID == ledger.LatestAuditID && (run.WebsiteID != ledger.WebsiteID || run.Site != ledger.Site || run.WebsiteRevision != ledger.WebsiteRevision || run.RuleVersion != ledger.RuleVersion || run.Status != ledger.LatestStatus || run.StartedAt != ledger.LatestStartedAt || run.CompletedAt != ledger.LatestCompletedAt || run.Reason != ledger.LatestReason) {
				return ErrInvalid
			}
			for _, condition := range ledger.Conditions {
				if condition.ProofAuditID != run.ID {
					continue
				}
				if run.WebsiteID != ledger.WebsiteID || run.Site != ledger.Site || run.WebsiteRevision != ledger.WebsiteRevision || run.RuleVersion != ledger.RuleVersion || !conditionMatchesReport(condition, run) {
					return ErrInvalid
				}
			}
		}
	}
	return nil
}
func conditionMatchesReport(c conditionRecord, run Snapshot) bool {
	for _, p := range run.Pages {
		if p.URL != c.PageURL {
			continue
		}
		if p.Status != "checked" || p.FinalURL != c.FinalURL || p.ObservedAt != c.ObservedAt || p.RunID != c.RunID || p.Fingerprint != c.Fingerprint {
			return false
		}
		matched := false
		for _, check := range p.Checks {
			if check.RuleID == c.RuleID && (check.Status == c.CheckStatus || c.CheckStatus == "unavailable") {
				matched = true
				break
			}
		}
		if !matched {
			return false
		}
		expected := []string{}
		for _, finding := range run.Findings {
			if finding.PageURL == p.URL && finding.RuleID == c.RuleID {
				expected = finding.RelatedURLs
				break
			}
		}
		return slices.Equal(expected, c.RelatedURLs)
	}
	return false
}
