package match

import (
	"slices"
	"strings"
	"testing"
	"unicode/utf8"
)

// found runs the finder over one passage and returns what it found as "capability/class".
func found(text string) []string {
	var out []string
	for _, f := range FindInPassages("konkurransegrunnlag.pdf", []Passage{{Page: 1, Text: text}}) {
		out = append(out, string(f.Capability)+"/"+string(f.Class))
	}
	return out
}

func TestTheFinderClassifiesWhatASentenceSaysAboutACapability(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		text string
		want []string
	}{
		"asked for":               {"Leverandøren skal være sertifisert etter ISO 9001 eller tilsvarende.", []string{"iso9001/requirement"}},
		"must be":                 {"Leverandøren må være registrert i StartBANK.", []string{"startbank/requirement"}},
		"must provide":            {"Skatteattest for skatt og merverdiavgift skal fremlegges.", []string{"tax_certificate/requirement"}},
		"required in English":     {"The supplier must be certified to ISO 9001.", []string{"iso9001/requirement"}},
		"not asked for":           {"Leverandøren trenger ikke Miljøfyrtårn-sertifisering.", []string{"miljofyrtarn/negated"}},
		"no requirement":          {"Det stilles ikke krav til ISO 14001 i denne konkurransen.", []string{"iso14001/negated"}},
		"rewarded, not asked for": {"Tilbydere med ISO 14001 får uttelling i evalueringen, inntil 10 poeng.", []string{"iso14001/award"}},
		"a qualification criterion that also mentions scoring": {"Kvalifikasjonskrav: ISO 9001 skal dokumenteres. Poeng gis ikke for dette.", []string{"iso9001/requirement"}},
		"only named":             {"Bedrifter med ISO 9001 er velkomne til å delta på oppstartsmøtet.", []string{"iso9001/mention"}},
		"two in one sentence":    {"Leverandøren skal ha ISO 9001 og ISO 14001.", []string{"iso14001/requirement", "iso9001/requirement"}},
		"spelled with a hyphen":  {"Leverandøren skal ha ISO-9001.", []string{"iso9001/requirement"}},
		"with a year":            {"Leverandøren skal ha ISO 9001:2015.", []string{"iso9001/requirement"}},
		"any case":               {"LEVERANDØREN SKAL HA ISO 9001.", []string{"iso9001/requirement"}},
		"nothing the rules know": {"Leverandøren skal ha relevant erfaring og tilstrekkelig kapasitet.", nil},
		"empty":                  {"", nil},
	} {
		got := found(tc.text)
		slices.Sort(got)
		want := slices.Clone(tc.want)
		slices.Sort(want)
		if !slices.Equal(got, want) {
			t.Errorf("%s: %q\nfound %v, want %v", name, tc.text, got, want)
		}
	}
}

// Each sentence is judged on its own: a negation in the next sentence does not cancel a requirement.
func TestTheFinderJudgesEachSentenceAlone(t *testing.T) {
	t.Parallel()

	got := found("Leverandøren skal ha ISO 9001. Leverandøren trenger ikke ISO 14001. Leverandøren skal være registrert i StartBANK.")
	slices.Sort(got)
	want := []string{"iso14001/negated", "iso9001/requirement", "startbank/requirement"}
	if !slices.Equal(got, want) {
		t.Errorf("found %v, want %v", got, want)
	}
}

// The text of a PDF page has a line break wherever the page wraps. A paragraph is rebuilt from
// the lines, a heading stays apart from the sentence under it, and a word split at a hyphen is joined.
func TestTheFinderRebuildsParagraphsFromPageLines(t *testing.T) {
	t.Parallel()

	page := "Kapittel 3 Kvalifikasjonskrav\n" +
		"3.1 Leverandøren skal være sertifisert etter ISO 9001 eller tilsvarende, og kunne\n" +
		"dokumentere dette ved tilbudsinnlevering.\n" +
		"3.2 Leverandøren må være registrert i StartBANK. Skatteattest for skatt og\n" +
		"merverdiavgift skal fremlegges.\n" +
		"Pris 40 prosent, kvalitet 60 prosent. Leverandøren trenger ikke Miljøfyrtårn-\n" +
		"sertifisering."
	findings := FindInPassages("kg.pdf", []Passage{{Page: 7, Text: page}})
	byCap := map[Capability]Finding{}
	for _, f := range findings {
		byCap[f.Capability] = f
	}
	iso := byCap[CapISO9001]
	if iso.Class != ClassRequirement || iso.Page != 7 || iso.Paragraph != 0 || iso.Document != "kg.pdf" {
		t.Errorf("iso9001 = %+v", iso)
	}
	if want := "3.1 Leverandøren skal være sertifisert etter ISO 9001 eller tilsvarende, og kunne dokumentere dette ved tilbudsinnlevering."; iso.Snippet != want {
		t.Errorf("snippet = %q\nwant %q: the wrapped lines are one sentence, and the heading is not part of it", iso.Snippet, want)
	}
	if got := byCap[CapStartBank].Snippet; got != "3.2 Leverandøren må være registrert i StartBANK." {
		t.Errorf("startbank snippet = %q", got)
	}
	if got := byCap[CapTaxCertificate].Snippet; got != "Skatteattest for skatt og merverdiavgift skal fremlegges." {
		t.Errorf("tax certificate snippet = %q", got)
	}
	miljo := byCap[CapMiljofyrtarn]
	if miljo.Class != ClassNegated || miljo.Snippet != "Leverandøren trenger ikke Miljøfyrtårn-sertifisering." {
		t.Errorf("miljofyrtarn = %+v: a word split at its hyphen is one word", miljo)
	}
}

// Tender documents list what is asked for under a sentence that asks: "The supplier shall document
// the following:" and then one item to a line. The items are what the lead-in says they are.
func TestTheFinderGivesListItemsTheClassOfTheirLeadIn(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		text string
		want []string
	}{
		"a lead-in that asks": {
			"Følgende skal dokumenteres:\n• ISO 9001\n• Registrering i StartBANK\n- Skatteattest",
			[]string{"iso9001/requirement", "startbank/requirement", "tax_certificate/requirement"},
		},
		"a lead-in that says not": {
			"Følgende er ikke krav i denne konkurransen:\n1. ISO 9001\n2. Achilles",
			[]string{"achilles/negated", "iso9001/negated"},
		},
		"a lead-in with no cue": {
			"Dokumenter vi har fått:\n• ISO 9001\n• StartBANK",
			[]string{"iso9001/mention", "startbank/mention"},
		},
		"the lead-in's class ends with the list": {
			"Følgende skal dokumenteres:\n• ISO 9001\nBedrifter med Achilles er velkomne.",
			[]string{"achilles/mention", "iso9001/requirement"},
		},
		"an item's own cue wins": {
			"Følgende er vedlagt:\n• Leverandøren skal ha ISO 9001",
			[]string{"iso9001/requirement"},
		},
	} {
		got := found(tc.text)
		slices.Sort(got)
		want := slices.Clone(tc.want)
		slices.Sort(want)
		if !slices.Equal(got, want) {
			t.Errorf("%s:\nfound %v, want %v", name, got, want)
		}
	}
}

func TestTheFinderNamesParagraphsAsWellAsPages(t *testing.T) {
	t.Parallel()

	got := FindInPassages("kontrakt.docx", []Passage{
		{Paragraph: 4, Text: "Ingenting her."},
		{Paragraph: 9, Text: "Leverandøren skal ha ISO 9001."},
	})
	if len(got) != 1 || got[0].Paragraph != 9 || got[0].Page != 0 || got[0].Document != "kontrakt.docx" {
		t.Errorf("findings = %+v", got)
	}
}

func TestTheFinderClipsALongSentenceAroundTheKeyword(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("blah ", 200) + "Leverandøren skal ha ISO 9001 i tillegg" + strings.Repeat(" bla", 200) + "."
	f := FindInPassages("a.pdf", []Passage{{Page: 1, Text: long}})
	if len(f) != 1 {
		t.Fatalf("findings = %+v", f)
	}
	if n := utf8.RuneCountInString(f[0].Snippet); n > maxSnippetRunes+2 {
		t.Errorf("snippet has %d runes, want at most %d (and an ellipsis at each cut end)", n, maxSnippetRunes+2)
	}
	if !strings.Contains(f[0].Snippet, "ISO 9001") {
		t.Errorf("the clipped snippet lost the keyword: %q", f[0].Snippet)
	}
}

// An abbreviation does not end a sentence, and a section number does not either.
func TestTheFinderDoesNotSplitAtAbbreviationsOrSectionNumbers(t *testing.T) {
	t.Parallel()

	f := FindInPassages("a.pdf", []Passage{{Page: 1, Text: "Dette gjelder bl.a. kap. 3.1 og pkt. 4: leverandøren skal ha ISO 9001, f.eks. fra DNV."}})
	if len(f) != 1 || f[0].Snippet != "Dette gjelder bl.a. kap. 3.1 og pkt. 4: leverandøren skal ha ISO 9001, f.eks. fra DNV." {
		t.Errorf("findings = %+v: the sentence was cut at an abbreviation", f)
	}
}

func TestTheFinderLeavesOutWhatItCannotCite(t *testing.T) {
	t.Parallel()

	if got := FindInPassages("a.pdf", nil); len(got) != 0 {
		t.Errorf("findings of no passages: %+v", got)
	}
	if got := FindInPassages("a.pdf", []Passage{{Page: 1, Text: "   \n\n  "}}); len(got) != 0 {
		t.Errorf("findings of blank text: %+v", got)
	}
}
