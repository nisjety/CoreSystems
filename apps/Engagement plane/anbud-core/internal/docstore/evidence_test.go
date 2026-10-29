package docstore

import (
	"fmt"
	"math"
	"math/rand"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"testing"
)

// evFinding is a finding of a capability and a class, numbered so that it is told from the others, with a snippet that is
// as long as asked.
func evFinding(capability, class string, n, snippetLen int) Finding {
	return Finding{
		Document: fmt.Sprintf("doc%d.pdf", n%7), Capability: capability, Class: class, Paragraph: n + 1,
		Snippet: fmt.Sprintf("%d:", n) + strings.Repeat("x", snippetLen),
	}
}

func paragraphsOf(fs []Finding) []int {
	var out []int
	for _, f := range fs {
		out = append(out, f.Paragraph)
	}
	return out
}

func TestSelectEvidenceKeepsEverythingThatFitsInTheOrderItCameIn(t *testing.T) {
	t.Parallel()

	all := []Finding{evFinding("iso9001", "mention", 0, 10), evFinding("startbank", "requirement", 1, 10), evFinding("iso9001", "award", 2, 10)}
	kept, dropped := selectEvidence(all, MaxEvidencePerCapability, MaxEvidencePerNotice, math.MaxInt)
	if dropped != 0 || !reflect.DeepEqual(kept, all) {
		t.Errorf("kept %v, dropped %d; want all three, as they came", paragraphsOf(kept), dropped)
	}
	if kept, dropped := selectEvidence(nil, 50, 400, 1<<20); len(kept) != 0 || dropped != 0 {
		t.Errorf("nothing: kept %v, dropped %d", kept, dropped)
	}
}

// What a company must have is what the gate looks for, so what asks for it is kept before what only names it, and of
// equal weight the earliest. What is kept comes back in the order it came in.
func TestSelectEvidenceKeepsWhatMattersMostFirstAndTheEarliestOfEqualWeight(t *testing.T) {
	t.Parallel()

	var all []Finding
	for n, class := range []string{"mention", "requirement", "mention", "award", "requirement", "negated", "mention", "requirement"} {
		all = append(all, evFinding("iso9001", class, n, 5))
	}
	for _, tc := range []struct {
		perNotice int
		want      []int // paragraph numbers, which are the index plus one
	}{
		{8, []int{1, 2, 3, 4, 5, 6, 7, 8}},
		{7, []int{1, 2, 3, 4, 5, 6, 8}}, // the last of the three mentions goes first
		{5, []int{2, 4, 5, 6, 8}},       // then the others
		{4, []int{2, 5, 6, 8}},          // a negation before an award
		{3, []int{2, 5, 8}},             // requirements only
		{1, []int{2}},                   // the first requirement of the capability, which is kept whatever, and nothing more
	} {
		kept, dropped := selectEvidence(all, 50, tc.perNotice, math.MaxInt)
		if !slices.Equal(paragraphsOf(kept), tc.want) || dropped != len(all)-len(tc.want) {
			t.Errorf("room for %d: kept %v, dropped %d; want %v", tc.perNotice, paragraphsOf(kept), dropped, tc.want)
		}
	}
}

// The first requirement of every capability is kept whatever else is cut, by the number in all, by the number of the
// capability and by the bytes: the gate must always be able to tell that a requirement is there, and where.
func TestSelectEvidenceAlwaysKeepsTheFirstRequirementOfEveryCapability(t *testing.T) {
	t.Parallel()

	var all []Finding
	for i := 0; i < 80; i++ { // a capability that is only named, 80 times, and then asked for
		all = append(all, evFinding("iso9001", "mention", i, 20))
	}
	all = append(all, evFinding("iso9001", "requirement", 80, 20))
	all = append(all, evFinding("startbank", "mention", 81, 20))
	all = append(all, evFinding("startbank", "requirement", 82, 20))
	all = append(all, evFinding("startbank", "requirement", 83, 20))

	for name, tc := range map[string]struct{ perCapability, perNotice, bytes int }{
		"no room by number":     {50, 0, math.MaxInt},
		"no room by capability": {0, 400, math.MaxInt},
		"no room by bytes":      {50, 400, 0},
		"no room at all":        {0, 0, 0},
	} {
		kept, dropped := selectEvidence(all, tc.perCapability, tc.perNotice, tc.bytes)
		if !slices.Equal(paragraphsOf(kept), []int{81, 83}) || dropped != len(all)-2 {
			t.Errorf("%s: kept %v, dropped %d; want the first requirement of each of the two capabilities, 81 and 83", name, paragraphsOf(kept), dropped)
		}
	}
}

// A large package says the same capability on every page, and that must not push the others out.
func TestSelectEvidenceKeepsNoMoreOfACapabilityThanItMay(t *testing.T) {
	t.Parallel()

	var all []Finding
	for i := 0; i < 60; i++ {
		all = append(all, evFinding("espd", "requirement", i, 5))
	}
	all = append(all, evFinding("iso9001", "requirement", 60, 5), evFinding("iso9001", "mention", 61, 5))
	kept, dropped := selectEvidence(all, 50, 400, math.MaxInt)
	perCapability := map[string]int{}
	for _, f := range kept {
		perCapability[f.Capability]++
	}
	if perCapability["espd"] != 50 || perCapability["iso9001"] != 2 || dropped != 10 {
		t.Errorf("kept %v, dropped %d; want 50 of espd and both of iso9001, and 10 left out", perCapability, dropped)
	}
	if got := paragraphsOf(kept)[:3]; !slices.Equal(got, []int{1, 2, 3}) {
		t.Errorf("of equal weight the earliest are kept: starts with %v", got)
	}
}

// The bytes are exact: what a finding takes is its text and findingOverhead, and the first that does not fit ends the list,
// so that a small one of less weight does not take the place of a large one that matters more.
func TestSelectEvidenceIsExactToTheByteAndEndsAtTheFirstThatDoesNotFit(t *testing.T) {
	t.Parallel()

	floor := evFinding("iso9001", "requirement", 0, 100) // kept whatever
	large := evFinding("iso9001", "negated", 1, 400)     // the best of the rest
	small := evFinding("iso9001", "mention", 2, 10)      // of less weight
	all := []Finding{floor, large, small}
	room := findingBytes(floor) + findingBytes(large)

	if kept, dropped := selectEvidence(all, 50, 400, room); !slices.Equal(paragraphsOf(kept), []int{1, 2}) || dropped != 1 {
		t.Errorf("room for exactly the floor and the large one: kept %v, dropped %d", paragraphsOf(kept), dropped)
	}
	if kept, dropped := selectEvidence(all, 50, 400, room-1); !slices.Equal(paragraphsOf(kept), []int{1}) || dropped != 2 {
		t.Errorf("a byte short: kept %v, dropped %d; want the floor alone, and not the small one in the place of the large", paragraphsOf(kept), dropped)
	}
	if kept, dropped := selectEvidence(all, 50, 400, room+findingBytes(small)); len(kept) != 3 || dropped != 0 {
		t.Errorf("room for all: kept %v, dropped %d", paragraphsOf(kept), dropped)
	}
}

// A notice with a great many findings is cut as it grows, and the cut as it grows is the cut of all of it at once: so the
// findings that are held never come to more than a notice keeps and a few more. This holds with the bytes out of the
// way, which is how the cuts as it grows are made; the bytes are applied once, at the end.
func TestSelectEvidenceCutAsItGrowsIsTheSameAsCutAtOnce(t *testing.T) {
	t.Parallel()

	classes := []string{"requirement", "negated", "award", "mention"}
	capabilities := []string{"iso9001", "iso14001", "startbank", "espd", "achilles"}
	rng := rand.New(rand.NewSource(11))
	for trial := 0; trial < 300; trial++ {
		var all []Finding
		for i, n := 0, 1+rng.Intn(300); i < n; i++ {
			all = append(all, evFinding(capabilities[rng.Intn(len(capabilities))], classes[rng.Intn(len(classes))], i, 1+rng.Intn(50)))
		}
		perCapability, perNotice := 1+rng.Intn(12), 1+rng.Intn(40)

		once, droppedOnce := selectEvidence(all, perCapability, perNotice, math.MaxInt)

		var held []Finding
		droppedByParts := 0
		for rest := all; len(rest) > 0; {
			n := min(len(rest), 1+rng.Intn(40))
			held = append(held, rest[:n]...)
			rest = rest[n:]
			var dropped int
			held, dropped = selectEvidence(held, perCapability, perNotice, math.MaxInt)
			droppedByParts += dropped
		}
		if !reflect.DeepEqual(held, once) || droppedByParts != droppedOnce {
			t.Fatalf("trial %d (%d findings, %d per capability, %d in all): cut as it grew: %v and %d dropped; cut at once: %v and %d dropped",
				trial, len(all), perCapability, perNotice, paragraphsOf(held), droppedByParts, paragraphsOf(once), droppedOnce)
		}
	}
}

func TestAFindingIsHashedByEveryPartOfItAndWhereEachPartEnds(t *testing.T) {
	t.Parallel()

	base := Finding{Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 1, Paragraph: 2, Snippet: "text"}
	hash := func(f Finding) [32]byte { h, _ := hashFinding(nil, f); return h }
	first, again := hash(base), hash(base)
	if first != again {
		t.Fatal("the same finding has two hashes")
	}
	for name, f := range map[string]Finding{
		"document":   {Document: "b.pdf", Capability: "iso9001", Class: "requirement", Page: 1, Paragraph: 2, Snippet: "text"},
		"capability": {Document: "a.pdf", Capability: "iso14001", Class: "requirement", Page: 1, Paragraph: 2, Snippet: "text"},
		"class":      {Document: "a.pdf", Capability: "iso9001", Class: "mention", Page: 1, Paragraph: 2, Snippet: "text"},
		"page":       {Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 2, Paragraph: 2, Snippet: "text"},
		"paragraph":  {Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 1, Paragraph: 3, Snippet: "text"},
		"snippet":    {Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 1, Paragraph: 2, Snippet: "texts"},
		// where one part ends and the next begins
		"a part's end moved into the next": {Document: "a.pd", Capability: "fiso9001", Class: "requirement", Page: 1, Paragraph: 2, Snippet: "text"},
		"a page that is a paragraph":       {Document: "a.pdf", Capability: "iso9001", Class: "requirement", Page: 2, Paragraph: 1, Snippet: "text"},
	} {
		if hash(f) == hash(base) {
			t.Errorf("a finding that differs in its %s has the hash of the other", name)
		}
	}
}

// What the builder sums is what the store has always said: a file that is in two archives is one file, and a finding that
// two archives share is one finding, kept once, the oldest upload first.
func TestTheBuilderSumsTheUploadsOfANoticeOnce(t *testing.T) {
	t.Parallel()

	read := func(name, hash string) Document {
		return Document{Path: name, Kind: "pdf", SHA256: sha(hash), Status: StatusRead}
	}
	empty := func(name, hash string) Document {
		return Document{Path: name, Kind: "pdf", SHA256: sha(hash), Status: StatusEmpty}
	}
	skipped := func(path string) Document {
		return Document{Path: path, Kind: "other", Status: StatusSkipped, Reason: "too_deep"}
	}
	first := []Finding{{Document: "kg.pdf", Capability: "iso9001", Class: "requirement", Page: 3, Snippet: "ISO 9001."}}
	second := []Finding{first[0], {Document: "rev/kontrakt.pdf", Capability: "startbank", Class: "requirement", Page: 2, Snippet: "StartBANK."}}

	b := newEvidenceBuilder(MaxEvidenceBytes)
	b.add("2026-100001", []Document{read("kg.pdf", "kg"), empty("skann.pdf", "skann"), {Path: "pris.xlsx", Kind: "xlsx", SHA256: sha("pris"), Status: StatusUnsupported}, skipped("a/b")}, first)
	// the second archive: the same specification again, a contract that is new, the same scan, the same skipped path, and a
	// file that could not be read in the first and is read here
	b.add("2026-100001", []Document{read("rev/kg.pdf", "kg"), read("rev/kontrakt.pdf", "kontrakt"), empty("rev/skann.pdf", "skann"), skipped("a/b"), read("pris.xlsx", "pris")}, second)
	b.add("2026-100002", []Document{read("x.pdf", "x")}, nil)
	got := b.result()

	one := got["2026-100001"]
	// read: kg, kontrakt and pris (read in the second, unsupported in the first: a file read from one archive is not
	// unread because another could not); unread: skann, and the skipped path
	if one.Documents != 3 || one.Unread != 2 {
		t.Errorf("documents %d, unread %d; want 3 and 2", one.Documents, one.Unread)
	}
	if want := []Finding{first[0], second[1]}; !reflect.DeepEqual(one.Findings, want) || one.FindingsDropped != 0 {
		t.Errorf("findings %v, %d dropped; want %v and none: the finding the second archive repeats is kept once", one.Findings, one.FindingsDropped, want)
	}
	if two := got["2026-100002"]; two.Documents != 1 || two.Unread != 0 || len(two.Findings) != 0 {
		t.Errorf("the other notice: %+v", two)
	}
	if len(got) != 2 {
		t.Errorf("%d notices in the answer, want 2", len(got))
	}
}

// uploadOf is one upload of a notice with findings of eight capabilities, fifty of each, as many as an upload keeps, and
// each different from the findings of every other upload.
func uploadOf(upload int) []Finding {
	capabilities := []string{"iso9001", "iso14001", "miljofyrtarn", "startbank", "achilles", "tax_certificate", "hms", "espd"}
	classes := []string{"requirement", "negated", "award", "mention"}
	var out []Finding
	for c, capability := range capabilities {
		for i := 0; i < MaxEvidencePerCapability; i++ {
			f := evFinding(capability, classes[(c+i)%len(classes)], upload*1000+c*100+i, 30)
			out = append(out, f)
		}
	}
	return out
}

// A notice with five uploads of as many findings as an upload keeps has five times as many distinct findings, and the
// evidence holds what one upload may keep and says how many it left out, so that len(Findings) and FindingsDropped are all
// of them.
func TestTheBuilderKeepsWhatANoticeKeepsAndCountsWhatItLeavesOut(t *testing.T) {
	t.Parallel()

	b := newEvidenceBuilder(MaxEvidenceBytes)
	for upload := 0; upload < 5; upload++ {
		b.add("2026-100001", nil, uploadOf(upload))
	}
	ev := b.result()["2026-100001"]
	if got := len(ev.Findings) + ev.FindingsDropped; got != 5*8*MaxEvidencePerCapability {
		t.Errorf("%d kept and %d left out make %d, want %d: every distinct finding is counted", len(ev.Findings), ev.FindingsDropped, got, 5*8*MaxEvidencePerCapability)
	}
	if len(ev.Findings) != MaxEvidencePerNotice {
		t.Errorf("%d findings held, want the %d that one upload keeps", len(ev.Findings), MaxEvidencePerNotice)
	}
	perCapability := map[string]int{}
	for _, f := range ev.Findings {
		perCapability[f.Capability]++
	}
	for capability, n := range perCapability {
		if n != MaxEvidencePerCapability {
			t.Errorf("%s: %d held, want %d", capability, n, MaxEvidencePerCapability)
		}
	}
	// What is held of a capability is its best: the requirements, which every upload has some of, come before what only
	// rewards or names it.
	for _, f := range ev.Findings {
		if f.Class != "requirement" && f.Capability == "iso9001" {
			t.Errorf("iso9001 holds a %s while its requirements, of which there are more than it may keep, were left out", f.Class)
			break
		}
	}
}

// The budget is the call's, shared by its notices in the order they come: a notice that has used it up holds the first
// requirement of each capability and no more, and says how many it left out, and what the notices say of their
// documents is whole.
func TestTheBudgetOfACallIsSharedByItsNotices(t *testing.T) {
	t.Parallel()

	one := uploadOf(0)
	budget := 0
	for _, f := range one[:60] {
		budget += findingBytes(f)
	}
	b := newEvidenceBuilder(budget)
	docs := []Document{{Path: "a.pdf", Kind: "pdf", SHA256: sha("a"), Status: StatusRead}}
	for i, notice := range []string{"2026-100001", "2026-100002", "2026-100003"} {
		b.add(notice, docs, uploadOf(i))
	}
	got := b.result()
	first, second := got["2026-100001"], got["2026-100002"]
	if len(first.Findings) < 8 || len(first.Findings) > 70 {
		t.Errorf("the first notice holds %d findings of a budget that is room for about 60", len(first.Findings))
	}
	for _, id := range []string{"2026-100002", "2026-100003"} {
		ev := got[id]
		floors := map[string]bool{}
		for _, f := range ev.Findings {
			if f.Class != "requirement" || floors[f.Capability] {
				t.Errorf("%s holds a finding that is not the first requirement of its capability: %+v", id, f)
				break
			}
			floors[f.Capability] = true
		}
		if len(floors) != 8 || len(ev.Findings)+ev.FindingsDropped != 8*MaxEvidencePerCapability {
			t.Errorf("%s: %d capabilities held, %d kept and %d left out; want the first requirement of all 8 and every finding counted", id, len(floors), len(ev.Findings), ev.FindingsDropped)
		}
		if ev.Documents != 1 {
			t.Errorf("%s: %d documents, want 1: what is said of the documents is whole", id, ev.Documents)
		}
	}
	if second.Documents != 1 || first.Documents != 1 {
		t.Errorf("documents %d and %d, want 1 and 1", first.Documents, second.Documents)
	}
}

// What the builder holds does not grow with what it is given. A hundred notices with an upload each of as many findings as
// an upload keeps, in characters of four bytes, are some hundred megabytes if they are held, and are held to the
// budget, and a few findings to each notice for the first requirements, if they are not. Not parallel: the heap is the
// process's.
func TestWhatTheBuilderHoldsDoesNotGrowWithWhatItIsGiven(t *testing.T) {
	letter := string(rune(0x1D538)) // a letter that takes four bytes
	heap := func() uint64 {
		runtime.GC()
		var m runtime.MemStats
		runtime.ReadMemStats(&m)
		return m.HeapAlloc
	}
	before := heap()
	b := newEvidenceBuilder(MaxEvidenceBytes)
	given := 0
	for notice := 0; notice < 100; notice++ {
		findings := uploadOf(notice)
		for i := range findings {
			findings[i].Document = strings.Repeat(letter, 100) + findings[i].Document
			findings[i].Snippet = strings.Repeat(letter, 300) + findings[i].Snippet
		}
		given += len(findings)
		b.add(fmt.Sprintf("2026-%06d", 100000+notice), nil, findings)
	}
	out := b.result()
	held := heap()
	runtime.KeepAlive(out)

	total, kept := 0, 0
	for _, ev := range out {
		total += len(ev.Findings) + ev.FindingsDropped
		kept += len(ev.Findings)
	}
	if total != given {
		t.Errorf("%d findings counted of %d given", total, given)
	}
	grew := int64(held) - int64(before)
	t.Logf("the builder holds %d MiB of %d findings given, %d of them kept", grew>>20, given, kept)
	if limit := int64(MaxEvidenceBytes) + 10<<20; grew > limit {
		t.Errorf("the builder holds %d MiB of %d findings given, %d of them kept; want no more than %d MiB", grew>>20, given, kept, limit>>20)
	}
}

// A notice with a great many uploads is cut as it grows, so that what is held while the uploads are added never comes to more
// than a notice keeps and an upload's worth, and not to the findings of every upload added so far. The bytes are out of the
// way here: it is the cut by number that holds what is held. Not parallel: the heap is the process's.
func TestANoticeWithAGreatManyUploadsIsCutAsItGrows(t *testing.T) {
	letter := string(rune(0x1D538)) // a letter that takes four bytes
	heap := func() uint64 {
		runtime.GC()
		var m runtime.MemStats
		runtime.ReadMemStats(&m)
		return m.HeapAlloc
	}
	before := heap()
	b := newEvidenceBuilder(math.MaxInt)
	var peak uint64
	given := 0
	for upload := 0; upload < 40; upload++ {
		findings := uploadOf(upload)
		for i := range findings {
			findings[i].Document = strings.Repeat(letter, 100) + findings[i].Document
			findings[i].Snippet = strings.Repeat(letter, 300) + findings[i].Snippet
		}
		given += len(findings)
		b.add("2026-100001", nil, findings)
		findings = nil
		peak = max(peak, heap())
	}
	ev := b.result()["2026-100001"]
	if len(ev.Findings)+ev.FindingsDropped != given || len(ev.Findings) != MaxEvidencePerNotice {
		t.Errorf("%d kept and %d left out of %d given; want %d kept and every one counted", len(ev.Findings), ev.FindingsDropped, given, MaxEvidencePerNotice)
	}
	grew := int64(peak) - int64(before)
	t.Logf("while the uploads were added the builder held at most %d MiB of the %d findings given", grew>>20, given)
	if limit := int64(12 << 20); grew > limit {
		t.Errorf("the builder held %d MiB while %d findings were added, want no more than %d MiB: a notice is cut as it grows", grew>>20, given, limit>>20)
	}
}
