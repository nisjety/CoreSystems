package eval

import (
	"testing"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

func TestSameEvalIntentRejectsIdempotencyPayloadChanges(t *testing.T) {
	base := &model.EvalRun{OrgID: "org-a", Strategy: "hybrid", Corpus: "recent"}
	if !sameEvalIntent(base, &model.EvalRun{OrgID: "org-a", Strategy: "hybrid", Corpus: "recent"}) {
		t.Fatal("identical evaluation request was rejected")
	}
	for name, changed := range map[string]*model.EvalRun{
		"tenant":   {OrgID: "org-b", Strategy: "hybrid", Corpus: "recent"},
		"strategy": {OrgID: "org-a", Strategy: "dense", Corpus: "recent"},
		"corpus":   {OrgID: "org-a", Strategy: "hybrid", Corpus: "all"},
	} {
		t.Run(name, func(t *testing.T) {
			if sameEvalIntent(base, changed) {
				t.Fatal("conflicting idempotent replay was accepted")
			}
		})
	}
}
