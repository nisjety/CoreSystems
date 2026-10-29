package demo

import (
	"testing"

	"github.com/triodelab/anbud/internal/match"
)

// The totals the demo shows are all of the findings of the uploaded documents, those the evidence holds and those it left
// out, and the list is of what is held.
func TestTheTotalsCountWhatTheEvidenceLeftOut(t *testing.T) {
	ev := match.Evidence{
		Documents: 1, FindingsDropped: 7,
		Findings: []match.Finding{
			{Capability: match.CapISO9001, Class: match.ClassRequirement, Document: "a.pdf", Page: 1, Snippet: "x"},
			{Capability: match.CapStartBank, Class: match.ClassAward, Document: "b.pdf", Paragraph: 2, Snippet: "y"},
		},
	}
	if v := documentsView(ev); v == nil || v.Findings != 9 {
		t.Errorf("documents view = %+v, want 9 findings: 2 held and 7 left out", v)
	}
	if d := auditDocuments(ev); d == nil || d.FindingsTotal != 9 || len(d.Findings) != 2 {
		t.Errorf("audit documents = %+v, want 2 listed of 9", d)
	}
}
