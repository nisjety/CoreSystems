package jobs

import (
	"testing"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

func TestSameJobIntentRejectsIdempotencyPayloadChanges(t *testing.T) {
	base := &model.Job{OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-a", "doc-b"}}
	if !sameJobIntent(base, &model.Job{OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-a", "doc-b"}}) {
		t.Fatal("identical job request was rejected")
	}
	for name, changed := range map[string]*model.Job{
		"tenant":    {OrgID: "org-b", JobType: model.JobReindex, DocumentIDs: []string{"doc-a", "doc-b"}},
		"job type":  {OrgID: "org-a", JobType: model.JobGraphBuild, DocumentIDs: []string{"doc-a", "doc-b"}},
		"documents": {OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-a"}},
		"order":     {OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-b", "doc-a"}},
	} {
		t.Run(name, func(t *testing.T) {
			if sameJobIntent(base, changed) {
				t.Fatal("conflicting idempotent replay was accepted")
			}
		})
	}
}
