package match

import (
	"slices"
	"strings"
	"testing"
	"time"
	"unicode/utf8"
)

// These tests pin the finder's rules one at a time, and its edges: where a snippet is cut, what ends a
// sentence, what a list item inherits. Each is a way the finder could be changed without a test of the
// ordinary case noticing.

func TestWhatClassifyMakesOfEachCue(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		lower string
		want  FindingClass
	}{
		"a negation":                              {"leverandøren trenger ikke iso 9001", ClassNegated},
		"a negation in English":                   {"iso 9001 is not required", ClassNegated},
		"a negation before an award cue":          {"iso 9001 trenger ikke gi poeng", ClassNegated},
		"a negation before a requirement cue":     {"leverandøren skal ikke ha iso 9001, det kreves ikke", ClassNegated},
		"a qualification cue with no other":       {"kvalifikasjonskrav iso 9001", ClassRequirement},
		"a qualification cue before an award cue": {"kvalifikasjonskrav med poeng for iso 9001", ClassRequirement},
		"an award cue":                            {"det gis poeng for iso 9001", ClassAward},
		"an award cue before a requirement cue":   {"leverandøren skal ha iso 9001 for å få poeng", ClassAward},
		"a requirement cue":                       {"leverandøren skal ha iso 9001", ClassRequirement},
		"a requirement cue that starts it":        {"skal leverandøren ha iso 9001", ClassRequirement},
		"a requirement cue that ends it":          {"iso 9001 er noe leverandøren må", ClassRequirement},
		"the cue inside a word is not one":        {"skalering av iso 9001", ClassMention},
		"no cue":                                  {"iso 9001 er omtalt her", ClassMention},
	} {
		if got := classify(tc.lower); got != tc.want {
			t.Errorf("%s: classify(%q) = %s, want %s", name, tc.lower, got, tc.want)
		}
	}
}

func TestAListItemTakesTheClassOfItsLeadInAndNoMore(t *testing.T) {
	t.Parallel()

	got := found("Leverandøren skal dokumentere følgende:\n- ISO 9001\n- StartBANK\n\nISO 14001 er omtalt her.")
	slices.Sort(got)
	want := []string{"iso14001/mention", "iso9001/requirement", "startbank/requirement"}
	if !slices.Equal(got, want) {
		t.Errorf("found %v, want %v: the items are what the lead-in says, and the paragraph after the list is not an item", got, want)
	}
	// Without a lead-in an item is only what it says of itself.
	if got := found("- ISO 9001\n- StartBANK"); !slices.Equal(got, []string{"iso9001/mention", "startbank/mention"}) {
		t.Errorf("items with no lead-in: %v", got)
	}
	// A lead-in that is only a mention hands on nothing.
	if got := found("Se følgende:\n- ISO 9001"); !slices.Equal(got, []string{"iso9001/mention"}) {
		t.Errorf("a lead-in that is a mention: %v", got)
	}
}

// A paragraph that is not an item ends the list: an item after it is of another list, or of none, and takes
// nothing from the lead-in above.
func TestAParagraphThatIsNotAnItemEndsTheListAndALaterItemStartsAnother(t *testing.T) {
	t.Parallel()

	got := found("Leverandøren skal dokumentere følgende:\n- ISO 9001\n\nDette avsnittet er helt uten sertifikater.\n\n- StartBANK")
	slices.Sort(got)
	want := []string{"iso9001/requirement", "startbank/mention"}
	if !slices.Equal(got, want) {
		t.Errorf("found %v, want %v: an item after a paragraph of ordinary text inherited the class of the list before it", got, want)
	}
}

func TestEachCapabilityOfASentenceIsAFinding(t *testing.T) {
	t.Parallel()

	got := found("Leverandøren skal ha ISO 9001 og være registrert i StartBANK.")
	slices.Sort(got)
	if want := []string{"iso9001/requirement", "startbank/requirement"}; !slices.Equal(got, want) {
		t.Errorf("found %v, want %v", got, want)
	}
}

func TestBulletsAreNotQuoted(t *testing.T) {
	t.Parallel()

	for _, text := range []string{"- Leverandøren skal ha ISO 9001.", "• Leverandøren skal ha ISO 9001.", "1. Leverandøren skal ha ISO 9001.", "(a) Leverandøren skal ha ISO 9001."} {
		fs := FindInPassages("a.txt", []Passage{{Paragraph: 1, Text: text}})
		if len(fs) != 1 || fs[0].Snippet != "Leverandøren skal ha ISO 9001." {
			t.Errorf("%q: %+v, want the sentence without its bullet", text, fs)
		}
	}
}

func TestFindingsSayWhereTheyAre(t *testing.T) {
	t.Parallel()

	fs := FindInPassages("vedlegg/krav.pdf", []Passage{
		{Page: 3, Text: "Ingenting her."},
		{Page: 4, Text: "Leverandøren skal ha ISO 9001."},
		{Paragraph: 7, Text: "Leverandøren må være registrert i StartBANK."},
	})
	if len(fs) != 2 {
		t.Fatalf("findings = %+v", fs)
	}
	if f := fs[0]; f.Document != "vedlegg/krav.pdf" || f.Page != 4 || f.Paragraph != 0 || f.Capability != CapISO9001 {
		t.Errorf("first = %+v", f)
	}
	if f := fs[1]; f.Document != "vedlegg/krav.pdf" || f.Page != 0 || f.Paragraph != 7 || f.Capability != CapStartBank {
		t.Errorf("second = %+v", f)
	}
}

// ---------------------------------------------------------------- snippets

// words is n words of seven characters, each ending in a space: "ord0001 ord0002 ...".
func words(n int) string {
	var b strings.Builder
	for i := 1; i <= n; i++ {
		b.WriteString("ord")
		b.WriteString(strings.Repeat("0", 4-len(itoa(i))))
		b.WriteString(itoa(i))
		b.WriteByte(' ')
	}
	return b.String()
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var d []byte
	for ; n > 0; n /= 10 {
		d = append([]byte{byte('0' + n%10)}, d...)
	}
	return string(d)
}

func snippetOf(t *testing.T, sentence string) string {
	t.Helper()
	fs := FindInPassages("a.txt", []Passage{{Paragraph: 1, Text: sentence}})
	if len(fs) != 1 {
		t.Fatalf("findings = %d for %q", len(fs), sentence)
	}
	return fs[0].Snippet
}

func TestASentenceAtTheSnippetLimitIsQuotedWhole(t *testing.T) {
	t.Parallel()

	// 320 characters, the keyword in the middle.
	pad := strings.Repeat("x", (maxSnippetRunes-len(" skal ISO 9001 "))/2)
	sentence := pad + " skal ISO 9001 " + pad
	sentence += strings.Repeat("y", maxSnippetRunes-utf8.RuneCountInString(sentence))
	if n := utf8.RuneCountInString(sentence); n != maxSnippetRunes {
		t.Fatalf("the test sentence has %d characters", n)
	}
	if got := snippetOf(t, sentence); got != sentence {
		t.Errorf("a sentence of exactly the limit is cut: %q", got)
	}
	over := sentence + "z"
	if got := snippetOf(t, over); got == over {
		t.Errorf("a sentence one over the limit is quoted whole")
	}
}

func TestALongSentenceIsCutAroundTheKeywordAtWordBoundariesAndSaysSo(t *testing.T) {
	t.Parallel()

	sentence := words(60) + "skal ha ISO 9001 " + words(60)
	got := snippetOf(t, sentence)
	if !strings.HasPrefix(got, "…") || !strings.HasSuffix(got, "…") {
		t.Fatalf("a snippet cut at both ends has no ellipsis at both: %q", got)
	}
	inner := strings.TrimSuffix(strings.TrimPrefix(got, "…"), "…")
	if !strings.Contains(inner, "ISO 9001") {
		t.Errorf("the keyword is not in the snippet: %q", got)
	}
	if n := utf8.RuneCountInString(inner); n > maxSnippetRunes {
		t.Errorf("the snippet has %d characters, more than %d", n, maxSnippetRunes)
	}
	// Cut at the edge of a word, both sides: it starts with a whole "ord" and ends on a whole number.
	if !strings.HasPrefix(inner, "ord") {
		t.Errorf("the snippet starts in the middle of a word: %q", inner[:12])
	}
	if last := inner[strings.LastIndexByte(strings.TrimRight(inner, " "), ' ')+1:]; !strings.HasPrefix(strings.TrimRight(last, " "), "ord") || len(strings.TrimRight(last, " ")) != 7 {
		t.Errorf("the snippet ends in the middle of a word: %q", inner[len(inner)-12:])
	}
}

// The cut falls where the length says, and is moved to the edge of a word from either side. The words here are
// ten characters and a space, so that the length puts the cut inside a word at both ends.
func TestACutSnippetStartsAndEndsOnWholeWordsWhereverTheLengthPutsTheCut(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct{ before, after string }{
		"words of ten":    {strings.Repeat("abcdefghij ", 40), strings.Repeat("klmnopqrst ", 40)},
		"words of nine":   {strings.Repeat("abcdefghi ", 40), strings.Repeat("klmnopqrs ", 40)},
		"words of eleven": {strings.Repeat("abcdefghijk ", 40), strings.Repeat("klmnopqrstu ", 40)},
	} {
		sentence := tc.before + "skal ha ISO 9001 " + tc.after
		got := snippetOf(t, sentence)
		if !strings.HasPrefix(got, "…") || !strings.HasSuffix(got, "…") {
			t.Fatalf("%s: a snippet cut at both ends has no ellipsis at both: %q", name, got)
		}
		whole := map[string]bool{"skal": true, "ha": true, "ISO": true, "9001": true}
		for _, w := range strings.Fields(tc.before + tc.after) {
			whole[w] = true
		}
		for _, w := range strings.Fields(strings.Trim(got, "…")) {
			if !whole[w] {
				t.Errorf("%s: the snippet holds %q, which is no whole word of the sentence: %q", name, w, got)
			}
		}
		if !strings.Contains(got, "ISO 9001") {
			t.Errorf("%s: the keyword is not in the snippet: %q", name, got)
		}
	}
}

func TestACutSnippetHasAnEllipsisOnlyWhereItWasCut(t *testing.T) {
	t.Parallel()

	atStart := snippetOf(t, "Leverandøren skal ha ISO 9001 "+words(80))
	if strings.HasPrefix(atStart, "…") || !strings.HasSuffix(atStart, "…") || !strings.HasPrefix(atStart, "Leverandøren") {
		t.Errorf("a keyword near the start: %q", atStart)
	}
	atEnd := snippetOf(t, words(80)+"Leverandøren skal ha ISO 9001.")
	if !strings.HasPrefix(atEnd, "…") || strings.HasSuffix(atEnd, "…") || !strings.HasSuffix(atEnd, "ISO 9001.") {
		t.Errorf("a keyword near the end: %q", atEnd)
	}
}

// The snippet is cut around the first keyword of the sentence.
func TestTheSnippetIsCutAroundTheEarliestKeyword(t *testing.T) {
	t.Parallel()

	sentence := "Leverandøren skal ha ISO 9001 og " + words(70) + "ISO-9001 igjen " + words(10)
	got := snippetOf(t, sentence)
	if !strings.Contains(got, "Leverandøren skal ha ISO 9001") || strings.Contains(got, "ISO-9001") {
		t.Errorf("the snippet is not around the first keyword: %q", got)
	}
}

// ---------------------------------------------------------------- paragraphs and sentences

func TestLinesAreJoinedIntoParagraphs(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("ord ", 18) + "ende" // 75 characters
	for name, tc := range map[string]struct {
		text string
		want []string
	}{
		"lines of a wrapped paragraph are joined":        {long + "\n" + long, []string{long + " " + long}},
		"a line that ends a sentence ends the paragraph": {long + ".\n" + long, []string{long + ".", long}},
		"a blank line ends it":                           {long + "\n\n" + long, []string{long, long}},
		"a word split at a hyphen is one word":           {"Miljøfyrtårn-\nsertifisering", []string{"Miljøfyrtårn-sertifisering"}},
		"a hyphen before a capital is not a split":       {"Standard ISO-\nStandard for kvalitet", []string{"Standard ISO- Standard for kvalitet"}},
		"an abbreviation does not end a sentence":        {long + " bl.a.\n" + long, []string{long + " bl.a. " + long}},
		"items are paragraphs of their own":              {"Følgende:\n- en\n- to", []string{"Følgende:", "en", "to"}},
	} {
		var got []string
		for _, p := range paragraphsOf(tc.text) {
			got = append(got, p.text)
		}
		if !slices.Equal(got, tc.want) {
			t.Errorf("%s:\n got %q\nwant %q", name, got, tc.want)
		}
	}
}

func TestWhatEndsASentence(t *testing.T) {
	t.Parallel()

	for line, want := range map[string]bool{
		"Dette er slutten.": true, "Er det slutt?": true, "Hurra!": true, "Følgende:": true, "En del; ": false,
		"En del;": true, "Se bl.a.": false, "Det koster 5 kr.": false, "Se ca.": false, "Ingen tegn": false,
		"": false, "Pkt.": false, "Slutten av en setning": false, "etc.": false, "Nr.": false,
	} {
		if got := endsSentence(line); got != want {
			t.Errorf("endsSentence(%q) = %v, want %v", line, got, want)
		}
	}
}

func TestSentencesAreCutWhereTheyEnd(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		in   string
		want []string
	}{
		"at a stop and a capital":                                          {"Første krav. Andre krav.", []string{"Første krav.", "Andre krav."}},
		"at a semicolon and a capital":                                     {"Første del; Andre del. Tredje.", []string{"Første del;", "Andre del.", "Tredje."}},
		"at a question mark and an exclamation":                            {"Er det slik? Ja! Neste.", []string{"Er det slik?", "Ja!", "Neste."}},
		"not at a stop inside a number":                                    {"Se kapittel 3.1 for krav. Neste.", []string{"Se kapittel 3.1 for krav.", "Neste."}},
		"not at an abbreviation":                                           {"Kravene gjelder bl.a. ISO 9001. Neste.", []string{"Kravene gjelder bl.a. ISO 9001.", "Neste."}},
		"not at a stop before a lower case letter":                         {"Prisen er 5 kr. men ikke mer. Neste setning.", []string{"Prisen er 5 kr. men ikke mer.", "Neste setning."}},
		"not at a stop before a lower case letter that is no abbreviation": {"Prisen er satt. men ikke endelig. Neste setning.", []string{"Prisen er satt. men ikke endelig.", "Neste setning."}},
		"at a stop before a digit":                                         {"Se kravet. 3 punkter følger.", []string{"Se kravet.", "3 punkter følger."}},
		"at a stop before a quotation mark":                                {"Han sa det. «Dette er et krav». Så gikk han.", []string{"Han sa det.", "«Dette er et krav».", "Så gikk han."}},
		"not at a stop inside a quotation":                                 {"Han sa «Dette er et krav.» Så gikk han.", []string{"Han sa «Dette er et krav.» Så gikk han."}},
		"at a stop before a bracket":                                       {"Se kravet. (Dette er viktig.)", []string{"Se kravet.", "(Dette er viktig.)"}},
		"what is left at the end":                                          {"Første. Resten uten punktum", []string{"Første.", "Resten uten punktum"}},
		"nothing":                                                          {"", nil},
	} {
		if got := splitSentences(tc.in); !slices.Equal(got, tc.want) {
			t.Errorf("%s:\n got %q\nwant %q", name, got, tc.want)
		}
	}
}

// A long run of abbreviations is read in time that grows with its length, not with its square.
func TestARunOfAbbreviationsIsReadInLinearTime(t *testing.T) {
	t.Parallel()

	text := strings.Repeat("ca. 4 ", 40_000)
	done := make(chan []string, 1)
	go func() { done <- splitSentences(text) }()
	select {
	case got := <-done:
		if len(got) != 1 {
			t.Errorf("%d sentences, want the one that nothing ends", len(got))
		}
	case <-time.After(3 * time.Second):
		t.Fatal("splitting 240 000 characters of abbreviations took over three seconds: the cost grows with the square of the length")
	}
}

// ---------------------------------------------------------------- the gate's wording

func TestUploadedDocumentsAreCountedInTheSingularAndThePlural(t *testing.T) {
	t.Parallel()

	for n, want := range map[int]string{1: "1 uploaded document", 2: "2 uploaded documents", 0: "0 uploaded documents", 17: "17 uploaded documents"} {
		if got := uploaded(n); got != want {
			t.Errorf("uploaded(%d) = %q, want %q", n, got, want)
		}
	}
	if got := uploadedWere(1); got != "1 uploaded document was" {
		t.Errorf("uploadedWere(1) = %q", got)
	}
	if got := uploadedWere(3); got != "3 uploaded documents were" {
		t.Errorf("uploadedWere(3) = %q", got)
	}
	if got := countOf(1, "passage"); got != "1 passage" {
		t.Errorf("countOf(1) = %q", got)
	}
	if got := countOf(4, "passage"); got != "4 passages" {
		t.Errorf("countOf(4) = %q", got)
	}
}
