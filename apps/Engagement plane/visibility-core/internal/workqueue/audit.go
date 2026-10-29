package workqueue

import (
	"context"
	"errors"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
)

// AuditSource resolves only completed, organization-bound persisted evidence.
// Neither saving nor verifying work triggers network/provider execution.
type AuditSource interface {
	Finding(id, site string, revision int64, auditID, findingID string) (siteaudit.Finding, error)
	LatestCheck(id, site string, revision int64, pageURL, ruleID string) (siteaudit.CheckEvidence, error)
}

// Optional extension keeps existing sources compatible while allowing Marketing
// Core to require the task's original server-saved audit in its bounded ledger.
type originalAuditChecker interface {
	LatestTaskCheck(id, site string, revision int64, sourceAuditID, pageURL, ruleID string) (siteaudit.CheckEvidence, error)
}

func (s *Service) WithAudit(source AuditSource) *Service {
	copy := *s
	copy.audit = source
	return &copy
}
func (s *Service) SaveAudit(_ context.Context, id, site string, websiteRevision int64, auditID, findingID string) (Snapshot, bool, error) {
	if websiteRevision < 1 || !auditIDPattern.MatchString(auditID) || !findingIDPattern.MatchString(findingID) {
		return Snapshot{}, false, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	if s.audit == nil {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	finding, err := s.audit.Finding(id, site, websiteRevision, auditID, findingID)
	if err != nil {
		return Snapshot{}, false, auditSourceError(err)
	}
	a := &AuditEvidence{AuditID: auditID, FindingID: finding.ID, PageURL: finding.PageURL, Severity: finding.Severity, RunID: finding.RunID, Fingerprint: finding.Fingerprint, RelatedURLs: append([]string{}, finding.RelatedURLs...)}
	evidence := Evidence{Kind: "site_audit_finding", Key: finding.RuleID, ObservedAt: finding.ObservedAt, Audit: a}
	stamp := s.now().UTC().Format(time.RFC3339Nano)
	if finding.ID != findingID || !validEvidence(evidence) || !auditSiteMatches(site, a) || newer(evidence.ObservedAt, stamp) {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	created, err := s.repo.create(id, site, evidence, stamp)
	if err != nil {
		return Snapshot{}, false, err
	}
	value, err := s.List(id, site)
	return value, created, err
}
func (s *Service) verifyAudit(id, site string, websiteRevision int64, task Task, revision int64) (Snapshot, error) {
	if s.audit == nil || task.Evidence.Audit == nil {
		return Snapshot{}, ErrSourceUnavailable
	}
	var check siteaudit.CheckEvidence
	var err error
	if source, ok := s.audit.(originalAuditChecker); ok {
		check, err = source.LatestTaskCheck(id, site, websiteRevision, task.Evidence.Audit.AuditID, task.Evidence.Audit.PageURL, task.Evidence.Key)
	} else {
		check, err = s.audit.LatestCheck(id, site, websiteRevision, task.Evidence.Audit.PageURL, task.Evidence.Key)
	}
	if err != nil {
		return Snapshot{}, auditSourceError(err)
	}
	value := Verification{Status: check.Status, ObservedAt: check.ObservedAt, RecordedAt: s.now().UTC().Format(time.RFC3339Nano), Audit: &AuditVerification{AuditID: check.AuditID, FindingID: check.FindingID, PageURL: check.PageURL, RuleID: check.RuleID, RunID: check.RunID, Fingerprint: check.Fingerprint}}
	if !validVerification(&value, task.Evidence) || (task.Verification != nil && !newer(value.ObservedAt, task.Verification.ObservedAt)) {
		return Snapshot{}, ErrConflict
	}
	if task.Verification != nil && task.Verification.Audit != nil && value.Audit.AuditID == task.Verification.Audit.AuditID {
		return Snapshot{}, ErrConflict
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, err
	}
	if err := s.repo.verify(id, site, task.ID, revision, value); err != nil {
		return Snapshot{}, err
	}
	return s.List(id, site)
}
func auditSourceError(err error) error {
	switch {
	case errors.Is(err, siteaudit.ErrInvalid):
		return ErrInvalid
	case errors.Is(err, siteaudit.ErrConflict):
		return ErrConflict
	case errors.Is(err, siteaudit.ErrNotFound):
		return ErrNotFound
	default:
		return ErrSourceUnavailable
	}
}
