package tenderdocs

import (
	"context"
	"testing"

	"github.com/triodelab/anbud/internal/docstore"
)

// What the store left out of the evidence of a notice is counted, and the count reaches the gate and the demo, so that the
// totals they show are all of the findings and not only of those that are held.
func TestEvidenceCarriesTheCountOfWhatTheStoreLeftOut(t *testing.T) {
	f := newFixture(t)
	f.store.evidence["2026-100001"] = docstore.Evidence{
		Documents: 1, FindingsDropped: 7,
		Findings: []docstore.Finding{{Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 1, Snippet: "ISO 9001."}},
	}
	got, err := f.svc.Evidence(context.Background(), testOrg, []string{"2026-100001"})
	if err != nil {
		t.Fatal(err)
	}
	if ev := got["2026-100001"]; ev.FindingsDropped != 7 || len(ev.Findings) != 1 {
		t.Errorf("evidence = %+v, want the 7 that were left out counted and the 1 that is held", ev)
	}
}

// The evidence of a notice is cut the way an upload is, and the numbers are the same: they are two constants for one
// rule, in two packages, and must not drift apart.
func TestTheEvidenceOfANoticeKeepsAsMuchAsAnUploadDoes(t *testing.T) {
	if docstore.MaxEvidencePerCapability != perCapability || docstore.MaxEvidencePerNotice != docstore.MaxFindings {
		t.Errorf("the evidence keeps %d of a capability and %d in all; an upload keeps %d and %d",
			docstore.MaxEvidencePerCapability, docstore.MaxEvidencePerNotice, perCapability, docstore.MaxFindings)
	}
}
