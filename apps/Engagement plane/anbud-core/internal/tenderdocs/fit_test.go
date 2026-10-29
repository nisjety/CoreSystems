package tenderdocs

import (
	"math/rand"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/match"
)

// storedFinding is a finding of a class whose snippet is as long as asked.
func storedFinding(class match.FindingClass, paragraph, snippetLen int) docstore.Finding {
	return docstore.Finding{
		Document: "a.txt", Capability: "iso9001", Class: string(class), Paragraph: paragraph,
		Snippet: strings.Repeat("x", snippetLen),
	}
}

func bytesOf(t *testing.T, f docstore.Finding) int {
	t.Helper()
	n, err := docstore.FindingBytes(f)
	if err != nil {
		t.Fatalf("FindingBytes: %v", err)
	}
	return n
}

// storedOfSize is a finding that takes exactly size bytes of the encoded list, whatever its class: the class is
// written out, and its name is not as long as another's.
func storedOfSize(t *testing.T, class match.FindingClass, paragraph, size int) docstore.Finding {
	t.Helper()
	f := storedFinding(class, paragraph, 0)
	f.Snippet = strings.Repeat("x", size-bytesOf(t, f))
	if got := bytesOf(t, f); got != size {
		t.Fatalf("a finding of %d bytes came to %d", size, got)
	}
	return f
}

func paragraphNumbers(fs []docstore.Finding) []int {
	var out []int
	for _, f := range fs {
		out = append(out, f.Paragraph)
	}
	return out
}

func TestFitKeepsEverythingThatFitsAndSaysSo(t *testing.T) {
	t.Parallel()

	found := []docstore.Finding{storedFinding(match.ClassRequirement, 1, 50), storedFinding(match.ClassMention, 2, 80), storedFinding(match.ClassAward, 3, 10)}
	kept, dropped := fit(found, docstore.MaxEncodedBytes)
	if dropped != 0 || !slices.Equal(paragraphNumbers(kept), []int{1, 2, 3}) {
		t.Errorf("kept %v, dropped %d; want all three and none dropped", paragraphNumbers(kept), dropped)
	}
	if kept, dropped := fit(nil, docstore.MaxEncodedBytes); len(kept) != 0 || dropped != 0 {
		t.Errorf("nothing: kept %v, dropped %d", kept, dropped)
	}
}

// The budget is exact to the byte: the brackets, and every finding with the comma after it but the last.
func TestFitIsExactToTheByte(t *testing.T) {
	t.Parallel()

	found := []docstore.Finding{storedFinding(match.ClassRequirement, 1, 50), storedFinding(match.ClassRequirement, 2, 70), storedFinding(match.ClassRequirement, 3, 90)}
	exact := 1
	for _, f := range found {
		exact += bytesOf(t, f)
	}
	if kept, dropped := fit(found, exact); dropped != 0 || len(kept) != 3 {
		t.Errorf("a budget of exactly what they take: kept %v, dropped %d", paragraphNumbers(kept), dropped)
	}
	if kept, dropped := fit(found, exact-1); dropped != 1 || !slices.Equal(paragraphNumbers(kept), []int{1, 2}) {
		t.Errorf("a byte short: kept %v, dropped %d; want the last of them left out", paragraphNumbers(kept), dropped)
	}
	if kept, dropped := fit(found, bytesOf(t, found[0])); dropped != 3 || len(kept) != 0 {
		t.Errorf("a byte short of the first: kept %v, dropped %d; want none", paragraphNumbers(kept), dropped)
	}
	if kept, dropped := fit(found, bytesOf(t, found[0])+1); dropped != 2 || !slices.Equal(paragraphNumbers(kept), []int{1}) {
		t.Errorf("room for the first alone: kept %v, dropped %d", paragraphNumbers(kept), dropped)
	}
}

// What is left out is what matters least: a requirement before a negation, a negation before an award, an award
// before a mention, and of equal weight the earliest. What is kept comes back in the order it came in.
func TestFitLeavesOutWhatMattersLeastAndKeepsTheOrderItCameIn(t *testing.T) {
	t.Parallel()

	const size = 200
	found := []docstore.Finding{
		storedOfSize(t, match.ClassMention, 0, size),
		storedOfSize(t, match.ClassRequirement, 1, size),
		storedOfSize(t, match.ClassMention, 2, size),
		storedOfSize(t, match.ClassRequirement, 3, size),
		storedOfSize(t, match.ClassNegated, 4, size),
		storedOfSize(t, match.ClassAward, 5, size),
	}
	for _, tc := range []struct {
		room int // how many of them the budget holds
		want []int
	}{
		{6, []int{0, 1, 2, 3, 4, 5}},
		{5, []int{0, 1, 3, 4, 5}}, // of the two mentions the later goes first
		{4, []int{1, 3, 4, 5}},
		{3, []int{1, 3, 4}},
		{2, []int{1, 3}},
		{1, []int{1}}, // of the two requirements the earlier stays
		{0, nil},
	} {
		kept, dropped := fit(found, 1+tc.room*size)
		if !slices.Equal(paragraphNumbers(kept), tc.want) || dropped != len(found)-len(tc.want) {
			t.Errorf("room for %d: kept %v, dropped %d; want %v", tc.room, paragraphNumbers(kept), dropped, tc.want)
		}
	}
}

// The first that does not fit ends the list: a small finding of less weight does not take the place of a large one
// that matters more, which would make what is kept depend on what happens to be small.
func TestFitEndsTheListAtTheFirstThatDoesNotFit(t *testing.T) {
	t.Parallel()

	found := []docstore.Finding{
		storedOfSize(t, match.ClassMention, 1, 160),     // small, and of the least weight
		storedOfSize(t, match.ClassRequirement, 2, 500), // large, and first of the two that matter most
		storedOfSize(t, match.ClassRequirement, 3, 400), // does not fit after it, in what the mention would leave
	}
	for _, tc := range []struct {
		name string
		room int
		want []int
	}{
		{"the first requirement, and what is left is the mention's size", 1 + 500 + 160, []int{2}},
		{"both requirements", 1 + 500 + 400, []int{2, 3}},
		{"everything", 1 + 500 + 400 + 160, []int{1, 2, 3}},
		{"the first requirement alone", 1 + 500, []int{2}},
	} {
		kept, dropped := fit(found, tc.room)
		if !slices.Equal(paragraphNumbers(kept), tc.want) || dropped != len(found)-len(tc.want) {
			t.Errorf("%s: kept %v, dropped %d; want %v", tc.name, paragraphNumbers(kept), dropped, tc.want)
		}
	}
}

// Whatever is thrown at it, what fit keeps is encoded within the budget, and is as much as could be kept in the order
// of weight: the next finding in that order would not have fitted.
func TestWhatFitKeepsIsWithinWhatTheStoreHoldsAndIsAsMuchAsFitted(t *testing.T) {
	t.Parallel()

	classes := []match.FindingClass{match.ClassRequirement, match.ClassNegated, match.ClassAward, match.ClassMention}
	rng := rand.New(rand.NewSource(7))
	cut := 0
	for trial := 0; trial < 200; trial++ {
		var found []docstore.Finding
		for i, n := 0, 150+rng.Intn(250); i < n; i++ {
			found = append(found, storedFinding(classes[rng.Intn(len(classes))], i, 1+rng.Intn(9000)))
		}
		kept, dropped := fit(found, docstore.MaxEncodedBytes)
		if len(kept)+dropped != len(found) {
			t.Fatalf("trial %d: %d kept and %d dropped of %d", trial, len(kept), dropped, len(found))
		}
		_, encoded, err := docstore.NewUpload{Findings: kept}.Encoded()
		if err != nil {
			t.Fatalf("trial %d: what was kept does not fit: %v", trial, err)
		}
		if !slices.IsSorted(paragraphNumbers(kept)) {
			t.Fatalf("trial %d: what was kept is out of the order it came in", trial)
		}
		if dropped == 0 {
			continue
		}
		cut++
		// The best of what was left out would not have fitted: the list is as long as it could be.
		keptSet := map[int]bool{}
		for _, f := range kept {
			keptSet[f.Paragraph] = true
		}
		best := -1
		for i, f := range found {
			if keptSet[f.Paragraph] {
				continue
			}
			if best < 0 || classRank[match.FindingClass(f.Class)] < classRank[match.FindingClass(found[best].Class)] {
				best = i
			}
		}
		if len(encoded)+bytesOf(t, found[best]) <= docstore.MaxEncodedBytes {
			t.Fatalf("trial %d: the best finding left out (%d bytes) would have fitted: the list is %d bytes of %d", trial, bytesOf(t, found[best]), len(encoded), docstore.MaxEncodedBytes)
		}
	}
	if cut < 100 {
		t.Errorf("only %d of the trials had to cut: the test does not try what it is for", cut)
	}
}

// Of equal weight the earliest is kept, whatever the number of them: a sort that is not stable keeps an order of its own
// among equals once there are more than a dozen.
func TestFitKeepsTheEarliestOfEqualWeightWhateverTheNumber(t *testing.T) {
	t.Parallel()

	const size = 150
	var found []docstore.Finding
	for i := 0; i < 300; i++ {
		class := match.ClassRequirement
		if i%3 == 0 {
			class = match.ClassMention
		}
		found = append(found, storedOfSize(t, class, i, size))
	}
	kept, dropped := fit(found, 1+100*size)
	var want []int
	for i := 0; i < 300 && len(want) < 100; i++ {
		if i%3 != 0 { // the requirements first, of them the earliest
			want = append(want, i)
		}
	}
	if !slices.Equal(paragraphNumbers(kept), want) || dropped != 200 {
		t.Errorf("kept %v, dropped %d; want the first hundred requirements %v", paragraphNumbers(kept), dropped, want)
	}
}
