package siteaudit

import "slices"

func (s *Service) Finding(id, site string, revision int64, auditID, findingID string) (Finding, error) {
	if revision < 1 || !findingPattern.MatchString(findingID) {
		return Finding{}, ErrInvalid
	}
	if _, err := s.binding(id, site, revision); err != nil {
		return Finding{}, err
	}
	v, err := s.GetRun(id, site, auditID)
	if err != nil {
		return Finding{}, err
	}
	if v.Status != "complete" {
		return Finding{}, ErrUnavailable
	}
	for _, f := range v.Findings {
		if f.ID == findingID {
			return f, nil
		}
	}
	return Finding{}, ErrNotFound
}
func (s *Service) Check(id, site string, revision int64, auditID, pageURL, ruleID string) (CheckEvidence, error) {
	if revision < 1 || !validRule(ruleID) {
		return CheckEvidence{}, ErrInvalid
	}
	if _, err := s.binding(id, site, revision); err != nil {
		return CheckEvidence{}, err
	}
	v, err := s.GetRun(id, site, auditID)
	if err != nil {
		return CheckEvidence{}, err
	}
	return checkEvidence(v, pageURL, ruleID)
}
func (s *Service) LatestCheck(id, site string, revision int64, pageURL, ruleID string) (CheckEvidence, error) {
	return s.latestCheck(id, site, revision, "", pageURL, ruleID)
}

// LatestTaskCheck binds a current check to the original audit selected from a
// persisted task, not browser-supplied evidence. An unknown/evicted original
// audit cannot be verified by a later, unrelated incident's resolution.
func (s *Service) LatestTaskCheck(id, site string, revision int64, sourceAuditID, pageURL, ruleID string) (CheckEvidence, error) {
	if !auditPattern.MatchString(sourceAuditID) {
		return CheckEvidence{}, ErrInvalid
	}
	return s.latestCheck(id, site, revision, sourceAuditID, pageURL, ruleID)
}
func (s *Service) latestCheck(id, site string, revision int64, sourceAuditID, pageURL, ruleID string) (CheckEvidence, error) {
	if revision < 1 || !validRule(ruleID) {
		return CheckEvidence{}, ErrInvalid
	}
	if _, err := s.binding(id, site, revision); err != nil {
		return CheckEvidence{}, err
	}
	// Full report and condition ledger are cloned under ONE repository lock.
	// A new running receipt cannot slip between separate list/ledger reads and
	// cause an older passing observation to masquerade as the latest result.
	ledger, _, current := s.repo.insight(id, site, revision)
	check, err := checkEvidence(current, pageURL, ruleID)
	if err != nil {
		return CheckEvidence{}, err
	}
	if check.Status == "passed" {
		check = guardLatestProof(check, ledger, current.ID)
		if sourceAuditID != "" && !slices.Contains(ledger.Processed, sourceAuditID) {
			check.Status = "inconclusive"
		}
	}
	if _, err = s.binding(id, site, revision); err != nil {
		return CheckEvidence{}, err
	}
	return check, nil
}
func guardLatestProof(check CheckEvidence, ledger insightRecord, auditID string) CheckEvidence {
	for _, condition := range ledger.Conditions {
		if condition.PageURL == check.PageURL && condition.RuleID == check.RuleID {
			if condition.ProofAuditID != auditID || condition.CheckStatus != "passed" || !condition.SeenIssue || !condition.EverPassed || condition.Unresolved || condition.DependencyLimited {
				check.Status = "inconclusive"
			}
			return check
		}
	}
	// A missing/pruned issue record cannot be replaced by a healthy baseline.
	check.Status = "inconclusive"
	return check
}
func checkEvidence(v Snapshot, pageURL, ruleID string) (CheckEvidence, error) {
	if v.Status != "complete" {
		return CheckEvidence{}, ErrUnavailable
	}
	for _, p := range v.Pages {
		if p.URL != pageURL || p.Status != "checked" {
			continue
		}
		for _, c := range p.Checks {
			if c.RuleID == ruleID {
				value := CheckEvidence{AuditID: v.ID, PageURL: p.URL, RuleID: ruleID, Status: c.Status, ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint}
				for _, f := range v.Findings {
					if f.PageURL == p.URL && f.RuleID == ruleID {
						value.FindingID = f.ID
						break
					}
				}
				return value, nil
			}
		}
	}
	return CheckEvidence{}, ErrUnavailable
}
