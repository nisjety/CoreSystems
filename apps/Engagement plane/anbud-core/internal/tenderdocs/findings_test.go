package tenderdocs

import (
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/match"
)

// finding is a finding of a capability and a class at a place in a document.
func finding(c match.Capability, class match.FindingClass, paragraph int) match.Finding {
	return match.Finding{Capability: c, Class: class, Document: "a.txt", Paragraph: paragraph, Snippet: fmt.Sprintf("%s %d", c, paragraph)}
}

func paragraphs(fs []match.Finding) []int {
	out := make([]int, len(fs))
	for i, f := range fs {
		out[i] = f.Paragraph
	}
	return out
}

// What is kept is chosen by weight and not by place, and then put back in the order of the document:
// 40 mentions come first and 30 requirements after them, 50 are kept, and the list reads from the top.
func TestFindingsAreKeptByWeightAndListedInDocumentOrder(t *testing.T) {
	t.Parallel()

	var all []match.Finding
	for i := 1; i <= 40; i++ {
		all = append(all, finding(match.CapISO9001, match.ClassMention, i))
	}
	for i := 41; i <= 70; i++ {
		all = append(all, finding(match.CapISO9001, match.ClassRequirement, i))
	}
	kept, dropped := selectWithin(all, 50, 1000)
	if len(kept) != 50 || dropped != 20 {
		t.Fatalf("kept %d dropped %d, want 50 and 20", len(kept), dropped)
	}
	got := paragraphs(kept)
	requirements := 0
	for _, f := range kept {
		if f.Class == match.ClassRequirement {
			requirements++
		}
	}
	if requirements != 30 {
		t.Errorf("%d requirements kept, want all 30: they are what the gate reads", requirements)
	}
	if !slices.IsSorted(got) || got[0] != 1 || got[len(got)-1] != 70 {
		t.Errorf("kept paragraphs %v: want them in document order, the 20 earliest mentions and then the 30 requirements", got)
	}
}

func TestEachCapabilityKeepsItsOwnShare(t *testing.T) {
	t.Parallel()

	var all []match.Finding
	for i := 1; i <= 10; i++ {
		all = append(all, finding(match.CapESPD, match.ClassMention, i)) // a capability that is named on every page
	}
	all = append(all, finding(match.CapISO9001, match.ClassRequirement, 11))
	kept, dropped := selectWithin(all, 3, 1000)
	if dropped != 7 || len(kept) != 4 {
		t.Fatalf("kept %d dropped %d, want 4 and 7", len(kept), dropped)
	}
	if kept[len(kept)-1].Capability != match.CapISO9001 {
		t.Errorf("the one sentence that asks for a certificate was pushed out by a capability that is mentioned everywhere: %+v", kept)
	}
}

// With the rules as they are the limit on the whole upload cannot bind; it must hold when it does.
func TestTheLimitOnTheWholeUploadHoldsWhenThePerCapabilityLimitsAddUpToMore(t *testing.T) {
	t.Parallel()

	var all []match.Finding
	n := 0
	for _, c := range []match.Capability{match.CapISO9001, match.CapISO14001, match.CapStartBank, match.CapAchilles} {
		for i := 0; i < 3; i++ {
			n++
			class := match.ClassMention
			if c == match.CapAchilles {
				class = match.ClassRequirement
			}
			all = append(all, finding(c, class, n))
		}
	}
	kept, dropped := selectWithin(all, 3, 5)
	if len(kept) != 5 || dropped != 7 {
		t.Fatalf("kept %d dropped %d, want 5 and 7", len(kept), dropped)
	}
	requirements := 0
	for _, f := range kept {
		if f.Capability == match.CapAchilles {
			requirements++
		}
	}
	if requirements != 3 {
		t.Errorf("%d of the 3 requirements kept: when the whole upload is over its limit the weightiest stay", requirements)
	}
	if got := paragraphs(all); len(got) != 12 {
		t.Fatalf("sanity: %v", got)
	}
}

func TestAFindingWithNoSentenceIsNotStored(t *testing.T) {
	t.Parallel()

	empty := finding(match.CapISO9001, match.ClassRequirement, 1)
	empty.Snippet = "\x00 \x00"
	stored := toStored([]match.Finding{empty, finding(match.CapISO9001, match.ClassRequirement, 2)})
	if len(stored) != 1 || stored[0].Paragraph != 2 {
		t.Errorf("stored %+v: a sentence the database cannot hold is left out", stored)
	}
}

// Cutting after every passage gives what cutting everything once would: the two are the same selection.
func TestCuttingAsTheFindingsComeGivesWhatCuttingThemAllAtOnceWould(t *testing.T) {
	t.Parallel()

	classes := []match.FindingClass{match.ClassMention, match.ClassRequirement, match.ClassAward, match.ClassNegated}
	caps := []match.Capability{match.CapISO9001, match.CapISO14001, match.CapStartBank}
	var all []match.Finding
	for i := 1; i <= 400; i++ {
		all = append(all, finding(caps[i%3], classes[(i*7)%4], i))
	}
	once, droppedOnce := selectWithin(all, 20, 45)

	var kept []match.Finding
	droppedStepwise := 0
	for i := 0; i < len(all); i += 13 {
		var d int
		kept, d = selectWithin(append(kept, all[i:min(i+13, len(all))]...), 20, 45)
		droppedStepwise += d
	}
	if !slices.Equal(paragraphs(once), paragraphs(kept)) || droppedOnce != droppedStepwise {
		t.Errorf("stepwise %v (dropped %d)\nat once  %v (dropped %d)", paragraphs(kept), droppedStepwise, paragraphs(once), droppedOnce)
	}
}

// What is kept is cut as it grows and not only once at the end: a file that names every capability in every
// paragraph never holds more than twice what an upload may keep, and nothing is lost without a number. When the
// walk is done it is cut to exactly what an upload keeps.
func TestWhatIsKeptIsCutAsItGrowsAndAtTheEnd(t *testing.T) {
	t.Parallel()

	const passages = 300
	text := "Leverandøren skal ha ISO 14001. Leverandøren skal ha ISO 9001. Leverandøren skal ha Miljøfyrtårn. " +
		"Leverandøren skal være registrert i StartBANK. Leverandøren skal være registrert i Achilles. " +
		"Leverandøren skal levere skatteattest. Leverandøren skal levere HMS-egenerklæring. Leverandøren skal levere ESPD."
	w := &walk{}
	found, cuts := 0, 0
	for i := 1; i <= passages; i++ {
		p := match.Passage{Paragraph: i, Text: text}
		found += len(match.FindInPassages("a.txt", []match.Passage{p}))
		before := w.dropped
		w.addPassage("a.txt", p)
		if w.dropped != before {
			cuts++
		}

		if len(w.found) > cutAbove {
			t.Fatalf("after passage %d the walk holds %d findings, more than it may before it cuts (%d)", i, len(w.found), cutAbove)
		}
		if len(w.found)+w.dropped != found {
			t.Fatalf("after passage %d: %d kept and %d dropped of %d found: something was lost without a number", i, len(w.found), w.dropped, found)
		}
	}
	if cuts == 0 || cuts > passages/2 {
		t.Errorf("the walk cut %d times in %d passages: it should cut when it has grown to %d, and not at every passage", cuts, passages, cutAbove)
	}
	w.finish()
	byCapability := map[match.Capability]int{}
	for _, f := range w.found {
		byCapability[f.Capability]++
	}
	for c, n := range byCapability {
		if n > perCapability {
			t.Errorf("%d findings of %s are kept, more than %d", n, c, perCapability)
		}
	}
	if len(w.found) != len(match.DefaultRequirementRules)*perCapability {
		t.Errorf("kept %d findings, want %d: every capability fills its share", len(w.found), len(match.DefaultRequirementRules)*perCapability)
	}
	if len(w.found)+w.dropped != found {
		t.Errorf("at the end: %d kept and %d dropped of %d found", len(w.found), w.dropped, found)
	}
}

// What the walk is done with is what an upload keeps, even when it never grew to the point of a cut on its own.
func TestTheWalkCutsWhatIsKeptWhenItIsDone(t *testing.T) {
	t.Parallel()

	f := newFixture(t)
	var text []string
	for i := 0; i < 120; i++ { // 120 findings of one capability: over what an upload keeps, and well under where the walk cuts
		text = append(text, "Leverandøren skal ha ISO 9001 sertifisering.")
	}
	if _, _, err := f.upload(t, zipOf(t, file("a.txt", strings.Join(text, "\n\n")))); err != nil {
		t.Fatal(err)
	}
	in := f.store.creates[0]
	if len(in.Findings) != perCapability || in.FindingsDropped != 120-perCapability {
		t.Errorf("kept %d findings and dropped %d, want %d and %d", len(in.Findings), in.FindingsDropped, perCapability, 120-perCapability)
	}
}

// A passage that names nothing leaves what is kept as it is.
func TestAPassageThatNamesNothingChangesNothing(t *testing.T) {
	t.Parallel()

	w := &walk{}
	w.addPassage("a.txt", match.Passage{Paragraph: 1, Text: "Leverandøren skal ha ISO 9001."})
	before, dropped := slices.Clone(w.found), w.dropped
	w.addPassage("a.txt", match.Passage{Paragraph: 2, Text: "Ingenting om sertifikater her."})
	if !slices.Equal(before, w.found) || w.dropped != dropped {
		t.Errorf("a passage that names nothing changed what is kept: %+v, dropped %d", w.found, w.dropped)
	}
	if len(w.found) != 1 || w.found[0].Document != "a.txt" || w.found[0].Paragraph != 1 {
		t.Errorf("found = %+v: want one finding, in a.txt at paragraph 1", w.found)
	}
}
