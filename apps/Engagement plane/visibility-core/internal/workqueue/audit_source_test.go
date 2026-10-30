package workqueue

import (
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
)

type originalAuditSource struct {
	*auditStub
	seenID string
}

func (s *originalAuditSource) LatestTaskCheck(_, _ string, _ int64, sourceAuditID, _, _ string) (siteaudit.CheckEvidence, error) {
	s.seenID = sourceAuditID
	check := s.check
	check.Status = "inconclusive"
	return check, s.err
}
func TestVerificationSuppliesOriginalServerSavedAuditToAwareSource(t *testing.T) {
	s, _, w, a, _ := auditService(t)
	task := saveAudit(t, s, w).Tasks[0]
	aware := &originalAuditSource{auditStub: a}
	s = s.WithAudit(aware)
	value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, task.Revision)
	if err != nil || aware.seenID != auditOne || value.Tasks[0].Verification.Status != "inconclusive" {
		t.Fatal("original saved proof bypassed", aware.seenID, value, err)
	}
}
