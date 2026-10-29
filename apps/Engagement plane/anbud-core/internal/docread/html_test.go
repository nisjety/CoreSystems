package docread

import (
	"context"
	"slices"
	"strings"
	"testing"
)

func TestReadHTMLMakesParagraphsOfBlocksAndDropsWhatIsNotContent(t *testing.T) {
	t.Parallel()

	src := `<!DOCTYPE html><html><head><title>Ikke innhold</title><style>p{color:red}</style><script>var x = "skal ikke med";</script></head>
<body><h1>Konkurransegrunnlag</h1><!-- en kommentar -->
<p>Leverandøren <b>skal</b> ha&nbsp;ISO&nbsp;9001 &amp; StartBANK.</p>
<ul><li>Første</li><li>Andre &aelig;&oslash;&aring;</li></ul>
<table><tr><td>Omsetning</td><td>5 MNOK</td></tr></table>
Løs tekst<br>etter br</body></html>`
	c, err := readHTML([]byte(src), DefaultLimits)
	if err != nil {
		t.Fatalf("readHTML: %v", err)
	}
	want := []string{"Konkurransegrunnlag", "Leverandøren skal ha ISO 9001 & StartBANK.", "Første", "Andre æøå", "Omsetning 5 MNOK", "Løs tekst", "etter br"}
	if got := texts(c.Units); !slices.Equal(got, want) {
		t.Errorf("units = %q\nwant %q", got, want)
	}
	for _, u := range c.Units {
		if strings.Contains(u.Text, "skal ikke med") || strings.Contains(u.Text, "Ikke innhold") || strings.Contains(u.Text, "color") || strings.Contains(u.Text, "kommentar") {
			t.Errorf("unit %q holds what is not content", u.Text)
		}
	}
}

// Older Norwegian software writes HTML in Windows-1252, in which æøå are single bytes. The tokenizer takes bytes as
// they come, so each of those was dropped as an invalid byte, with no error and no mark of truncation, and the word
// went with it ("Miljøsertifikat" came out as "Miljsertifikat"). Text is read the same way (see readText).
func TestHTMLThatIsNotUTF8IsWindows1252AndKeepsItsNorwegianLetters(t *testing.T) {
	t.Parallel()

	const page = "<html><head><meta charset=\"iso-8859-1\"><title>Side</title></head><body>" +
		"<p>Milj\xf8sertifikat \xe6 \xf8 \xe5 \xc6 \xd8 \xc5</p><p>Pris 100\x80 \x93sitat\x94</p></body></html>"
	want := []string{"Miljøsertifikat æ ø å Æ Ø Å", "Pris 100€ “sitat”"}

	c, err := readHTML([]byte(page), DefaultLimits)
	if err != nil {
		t.Fatalf("readHTML: %v", err)
	}
	if got := texts(c.Units); !slices.Equal(got, want) {
		t.Errorf("units = %q\nwant %q", got, want)
	}
	if c.Truncated {
		t.Error("nothing was left out, and the file says it was")
	}

	// Through the reader, which has to take it for HTML first.
	c, err = reader().Read(context.Background(), []byte(page))
	if err != nil || c.Kind != KindHTML {
		t.Fatalf("Read: kind %q, err %v", c.Kind, err)
	}
	if got := texts(c.Units); !slices.Equal(got, want) {
		t.Errorf("through the reader: units = %q\nwant %q", got, want)
	}
}

// A page that is valid UTF-8 is UTF-8, whatever it says it is: Windows-1252 text that happens to be valid UTF-8 is
// not a thing that is written, and a page that says one thing and is another is, so what it says is not read.
func TestHTMLThatIsValidUTF8IsReadAsUTF8EvenIfItDeclaresAnotherEncoding(t *testing.T) {
	t.Parallel()

	const page = "<html><head><meta charset=\"iso-8859-1\"></head><body><p>Miljøsertifikat ÆØÅ “sitat” 100€</p></body></html>"
	c, err := readHTML([]byte(page), DefaultLimits)
	if err != nil {
		t.Fatalf("readHTML: %v", err)
	}
	if got, want := texts(c.Units), []string{"Miljøsertifikat ÆØÅ “sitat” 100€"}; !slices.Equal(got, want) {
		t.Errorf("units = %q\nwant %q", got, want)
	}
}

// The byte order mark is taken off before the page is judged, as it is for text: a page that begins with one and is
// not UTF-8 is Windows-1252 all the same, and the mark is not read as three letters of it.
func TestAUTF8MarkBeforeHTMLIsNotReadAsWindows1252Letters(t *testing.T) {
	t.Parallel()

	mark := []byte{0xEF, 0xBB, 0xBF}
	for name, body := range map[string]string{
		"windows-1252": "<html><body><p>F\xf8rste</p></body></html>",
		"utf-8":        "<html><body><p>Første</p></body></html>",
	} {
		c, err := readHTML(append(slices.Clone(mark), body...), DefaultLimits)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if got, want := texts(c.Units), []string{"Første"}; !slices.Equal(got, want) {
			t.Errorf("%s: units = %q\nwant %q", name, got, want)
		}
	}
}

// What is decoded is the whole page and not its start: a page long enough for the decoder to work in many pieces
// keeps every letter, and the letters at the places where the pieces meet.
func TestAWindows1252PageIsDecodedAllTheWayThrough(t *testing.T) {
	t.Parallel()

	const n = 20_000
	page := "<html><body><p>" + strings.Repeat("Milj\xf8 ", n) + "</p></body></html>"
	c, err := readHTML([]byte(page), DefaultLimits)
	if err != nil {
		t.Fatalf("readHTML: %v", err)
	}
	if len(c.Units) != 1 {
		t.Fatalf("units = %d, want 1", len(c.Units))
	}
	if got := strings.Count(c.Units[0].Text, "Miljø"); got != n {
		t.Errorf("%d of %d words kept their letter", got, n)
	}
	if strings.ContainsRune(c.Units[0].Text, '�') {
		t.Error("a replacement character got through")
	}
}

func TestReadHTMLSurvivesBrokenMarkup(t *testing.T) {
	t.Parallel()

	for name, src := range map[string]string{
		"unclosed tags":   "<p>en <b>fet <i>kursiv",
		"unterminated":    "<p>tekst <a href=\"x",
		"only a tag":      "<",
		"deep nesting":    strings.Repeat("<div>", 20_000) + "bunn" + strings.Repeat("</div>", 20_000),
		"many attributes": "<p " + strings.Repeat(`a="1" `, 5000) + ">tekst</p>",
	} {
		if _, err := readHTML([]byte(src), DefaultLimits); err != nil {
			t.Errorf("%s: %v", name, err)
		}
	}
}
