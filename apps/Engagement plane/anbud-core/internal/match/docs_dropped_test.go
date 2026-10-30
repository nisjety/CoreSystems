package match

import (
	"strings"
	"testing"
)

// The evidence of a notice holds only the best of its findings when there are a great many, and what it left out is
// counted. The numbers of passages and documents that a document requirement says are then of what is held, and say
// so: at least. When nothing was left out they are what they were.
func TestTheCountsOfADocumentRequirementAreLowerBoundsWhenFindingsWereLeftOut(t *testing.T) {
	t.Parallel()

	held := ev(
		finding(CapStartBank, ClassRequirement, "a.pdf", 1, "Leverandøren må være registrert i StartBANK."),
		finding(CapStartBank, ClassRequirement, "b.pdf", 2, "StartBANK kreves."),
	)
	whole := documentChecks(assessWith(tender(), profile(), Balanced, held))
	if len(whole) != 1 || !strings.Contains(whole[0].Detail, "(2 passages in 2 documents)") || strings.Contains(whole[0].Detail, "at least") {
		t.Fatalf("with nothing left out: %+v", whole)
	}

	held.FindingsDropped = 40
	cut := documentChecks(assessWith(tender(), profile(), Balanced, held))
	if len(cut) != 1 || !strings.Contains(cut[0].Detail, "(at least 2 passages in at least 2 documents)") {
		t.Errorf("with findings left out: %+v; want the counts to say that they are at least", cut)
	}
}
