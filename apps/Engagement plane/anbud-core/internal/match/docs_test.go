package match

import (
	"reflect"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
)

func assessWith(n eforms.Notice, p Profile, s Strictness, ev Evidence) Assessment {
	return AssessWith(n, normalize.Notice(n), p, s, now, ev)
}

func finding(c Capability, class FindingClass, doc string, page int, snippet string) Finding {
	return Finding{Capability: c, Class: class, Document: doc, Page: page, Snippet: snippet}
}

// ev is what an upload of two readable documents said.
func ev(findings ...Finding) Evidence { return Evidence{Documents: 2, Findings: findings} }

func documentChecks(a Assessment) []Check {
	var out []Check
	for _, c := range a.Gate {
		if c.Rule == RuleDocumentRequirement {
			out = append(out, c)
		}
	}
	return out
}

func TestADocumentRequirementIsJudgedByLeadTimeAndCitedToItsPassage(t *testing.T) {
	t.Parallel()

	a := assessWith(tender(), profile(), Balanced, ev(
		finding(CapStartBank, ClassRequirement, "konkurransegrunnlag.pdf", 12, "Leverandøren må være registrert i StartBANK."),
	))
	checks := documentChecks(a)
	if len(checks) != 1 {
		t.Fatalf("document checks = %+v, want one", checks)
	}
	c := checks[0]
	// StartBANK takes 14 days, the tender has 20: it can be done in time.
	if c.Outcome != Conditional || c.DaysLeft == nil || *c.DaysLeft != 20 {
		t.Errorf("check = %+v, want conditional with 20 days left", c)
	}
	want := Cite{Field: "document", Document: "konkurransegrunnlag.pdf", Page: 12, Snippet: "Leverandøren må være registrert i StartBANK."}
	if c.Cite != want {
		t.Errorf("cite = %+v\nwant %+v", c.Cite, want)
	}
	for _, s := range []string{"startbank", "uploaded documents", "1 passage in 1 document", "keyword"} {
		if !strings.Contains(c.Detail, s) {
			t.Errorf("detail %q lacks %q", c.Detail, s)
		}
	}
	if a.Verdict != Conditional {
		t.Errorf("verdict = %v", a.Verdict)
	}
}

func TestADocumentRequirementTheCompanyHoldsPasses(t *testing.T) {
	t.Parallel()

	p := profile()
	p.Held = map[Capability]bool{CapStartBank: true}
	a := assessWith(tender(), p, Balanced, ev(finding(CapStartBank, ClassRequirement, "a.pdf", 1, "Krav om StartBANK.")))
	if c := documentChecks(a); len(c) != 1 || c[0].Outcome != Pass {
		t.Errorf("checks = %+v, want one pass", c)
	}
}

// A passage found by keyword is not a requirement proved. It can ask a person to look, and it cannot
// close a tender to a company: nothing read from a document fails the gate.
func TestNothingReadFromADocumentFailsTheGate(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots[0].Deadline = at(5) // StartBANK takes 14 days: the notice's own criteria would fail this
	a := assessWith(n, profile(), Balanced, ev(finding(CapStartBank, ClassRequirement, "a.pdf", 1, "Krav om StartBANK.")))
	c := documentChecks(a)
	if len(c) != 1 || c[0].Outcome != Conditional {
		t.Fatalf("checks = %+v, want one conditional", c)
	}
	if a.Verdict != Conditional {
		t.Errorf("verdict = %v: a keyword match in a document failed the gate", a.Verdict)
	}
	if !strings.Contains(c[0].Detail, "5 days left") {
		t.Errorf("detail = %q: the days left must still be said", c[0].Detail)
	}
}

func TestOnlyARequirementPassageIsACheck(t *testing.T) {
	t.Parallel()

	a := assessWith(tender(), profile(), Balanced, ev(
		finding(CapISO9001, ClassNegated, "a.pdf", 1, "Krav om ISO 9001 stilles ikke."),
		finding(CapISO14001, ClassAward, "a.pdf", 2, "ISO 14001 gir poeng."),
		finding(CapAchilles, ClassMention, "a.pdf", 3, "Achilles er kjent."),
	))
	if c := documentChecks(a); len(c) != 0 {
		t.Errorf("checks = %+v, want none: a negation, an award and a mention are not requirements", c)
	}
}

func TestACapabilityIsOneCheckWhateverItsPassages(t *testing.T) {
	t.Parallel()

	a := assessWith(tender(), profile(), Balanced, ev(
		finding(CapISO9001, ClassMention, "a.pdf", 1, "Først bare nevnt."),
		finding(CapISO9001, ClassRequirement, "a.pdf", 4, "Første krav."),
		finding(CapISO9001, ClassRequirement, "b.docx", 0, "Andre krav."),
		finding(CapISO9001, ClassRequirement, "a.pdf", 9, "Tredje krav."),
	))
	c := documentChecks(a)
	if len(c) != 1 {
		t.Fatalf("checks = %+v, want one", c)
	}
	if c[0].Cite.Document != "a.pdf" || c[0].Cite.Page != 4 || c[0].Cite.Snippet != "Første krav." {
		t.Errorf("cite = %+v, want the first passage that asks for it", c[0].Cite)
	}
	if !strings.Contains(c[0].Detail, "3 passages in 2 documents") {
		t.Errorf("detail = %q", c[0].Detail)
	}
}

func TestACapabilityTheNoticeAlreadyStatesIsNotCheckedTwice(t *testing.T) {
	t.Parallel()

	n := tender()
	n.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Type: "other", Name: "Krav", Description: "Registrert i StartBANK."}}
	a := assessWith(n, profile(), Balanced, ev(
		finding(CapStartBank, ClassRequirement, "a.pdf", 1, "Krav om StartBANK."),
		finding(CapISO9001, ClassRequirement, "a.pdf", 2, "Krav om ISO 9001."),
	))
	c := documentChecks(a)
	if len(c) != 1 || !strings.Contains(c[0].Detail, "iso9001") {
		t.Errorf("checks = %+v, want only ISO 9001: the notice's own criterion already covers StartBANK", c)
	}
}

func TestDocumentChecksComeAfterTheNoticesOwnRequirements(t *testing.T) {
	t.Parallel()

	n := tender()
	n.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Type: "other", Name: "Krav", Description: "Noe uvanlig."}}
	a := assessWith(n, profile(), Balanced, ev(finding(CapISO9001, ClassRequirement, "a.pdf", 1, "Krav om ISO 9001.")))
	var rules []string
	for _, c := range a.Gate {
		rules = append(rules, c.Rule)
	}
	if got := strings.Join(rules, " "); got != "deadline requirement document_requirement" {
		t.Errorf("gate = %s", got)
	}
}

func TestTheDocumentsPointerSaysWhatWasUploaded(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{{Lot: "LOT-0001", Type: eforms.RequirementInTenderDocuments}}

	without := assessWith(n, profile(), Balanced, Evidence{})
	c := check(t, without, RuleDocuments)
	if !strings.Contains(c.Detail, "which are not read yet") {
		t.Errorf("without uploads: %q", c.Detail)
	}

	with := assessWith(n, profile(), Balanced, Evidence{Documents: 3, Unread: 1, Findings: []Finding{
		finding(CapISO9001, ClassRequirement, "a.pdf", 1, "Krav om ISO 9001."),
	}})
	c = check(t, with, RuleDocuments)
	if c.Outcome != Unknown {
		t.Errorf("outcome = %v: what the rules read is not all the criteria there are", c.Outcome)
	}
	for _, s := range []string{"3 uploaded documents were read", "1 could not be read", "turnover, references, staff"} {
		if !strings.Contains(c.Detail, s) {
			t.Errorf("detail %q lacks %q", c.Detail, s)
		}
	}
	if with.Verdict != Conditional {
		t.Errorf("verdict = %v", with.Verdict)
	}
}

// Uploaded documents are evidence even when the notice does not say where its criteria are.
func TestUploadedDocumentsAreReadWhetherOrNotTheNoticePointsToThem(t *testing.T) {
	t.Parallel()

	a := assessWith(tender(), profile(), Balanced, ev(finding(CapISO9001, ClassRequirement, "a.pdf", 1, "Krav om ISO 9001.")))
	for _, c := range a.Gate {
		if c.Rule == RuleDocuments {
			t.Errorf("a notice that does not point to its documents got %+v", c)
		}
	}
	if len(documentChecks(a)) != 1 {
		t.Errorf("the document check is missing: %+v", a.Gate)
	}
}

func TestUnreadDocumentsAreSaidEvenWithNoFindings(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{{Lot: "LOT-0001", Type: eforms.RequirementInTenderDocuments}}
	a := assessWith(n, profile(), Balanced, Evidence{Documents: 0, Unread: 2})
	c := check(t, a, RuleDocuments)
	if !strings.Contains(c.Detail, "2 uploaded documents could not be read") {
		t.Errorf("detail = %q: uploads that could not be read must be said, not mistaken for no uploads", c.Detail)
	}
}

func TestAssessWithNoEvidenceIsAssess(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Requirements = []eforms.Requirement{{Lot: "LOT-0001", Type: eforms.RequirementInTenderDocuments}}
	n.SelectionCriteria = []eforms.SelectionCriterion{{Lot: "LOT-0001", Name: "Krav", Description: "Registrert i StartBANK."}}
	a := Assess(n, normalize.Notice(n), profile(), Balanced, now)
	b := AssessWith(n, normalize.Notice(n), profile(), Balanced, now, Evidence{})
	if len(a.Gate) != len(b.Gate) || a.Verdict != b.Verdict || a.Band != b.Band {
		t.Errorf("Assess %+v\nAssessWith %+v", a, b)
	}
	for i := range a.Gate {
		if !reflect.DeepEqual(a.Gate[i], b.Gate[i]) {
			t.Errorf("check %d differs: %+v vs %+v", i, a.Gate[i], b.Gate[i])
		}
	}
}

func TestADocumentCheckUsesTheDaysToTheNextOpenDeadline(t *testing.T) {
	t.Parallel()

	n := tender()
	n.Lots = []eforms.Lot{{ID: "A", Deadline: at(30)}, {ID: "B", Deadline: at(3)}}
	a := assessWith(n, profile(), Balanced, ev(finding(CapStartBank, ClassRequirement, "a.pdf", 1, "Krav om StartBANK.")))
	c := documentChecks(a)
	if len(c) != 1 || c[0].DaysLeft == nil || *c[0].DaysLeft != 3 {
		t.Errorf("checks = %+v, want 3 days left", c)
	}
}
