package docread

import (
	"archive/zip"
	"bytes"
	"errors"
	"slices"
	"strings"
	"testing"
)

func TestReadDOCXKeepsTheTextOfParagraphsInOrder(t *testing.T) {
	t.Parallel()

	body := para("Kapittel 3 Kvalifikasjonskrav") +
		// A word split over runs, as Word does when the formatting changes mid-word.
		`<w:p><w:r><w:t>Leverandø</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>ren skal</w:t></w:r><w:r><w:tab/><w:t>ha ISO 9001</w:t></w:r><w:r><w:br/><w:t>eller tilsvarende.</w:t></w:r></w:p>` +
		`<w:p></w:p>` +
		para("   ") +
		para("Æøå ÆØÅ")
	c, err := readDOCX(docxFile(t, body), DefaultLimits)
	if err != nil {
		t.Fatalf("readDOCX: %v", err)
	}
	want := []string{"Kapittel 3 Kvalifikasjonskrav", "Leverandøren skal ha ISO 9001 eller tilsvarende.", "Æøå ÆØÅ"}
	if got := texts(c.Units); !slices.Equal(got, want) {
		t.Errorf("units = %q, want %q", got, want)
	}
	for i, u := range c.Units {
		if u.Paragraph != i+1 || u.Page != 0 {
			t.Errorf("unit %d = %+v", i, u)
		}
	}
	if c.Kind != KindDOCX || c.Extractor != "docx" {
		t.Errorf("kind %q extractor %q", c.Kind, c.Extractor)
	}
}

func TestReadDOCXTakesInsertionsButNotDeletions(t *testing.T) {
	t.Parallel()

	body := `<w:p><w:r><w:t>Krav: </w:t></w:r>` +
		`<w:del><w:r><w:delText>ISO 14001</w:delText></w:r></w:del>` +
		`<w:ins><w:r><w:t>ISO 9001</w:t></w:r></w:ins>` +
		`<w:r><w:instrText> PAGE </w:instrText></w:r></w:p>`
	c, err := readDOCX(docxFile(t, body), DefaultLimits)
	if err != nil {
		t.Fatalf("readDOCX: %v", err)
	}
	if got := texts(c.Units); !slices.Equal(got, []string{"Krav: ISO 9001"}) {
		t.Errorf("units = %q: a deleted run and a field code are not text", got)
	}
}

func TestReadDOCXMakesOneUnitOfATableRow(t *testing.T) {
	t.Parallel()

	row := func(cells ...string) string {
		var b strings.Builder
		b.WriteString("<w:tr>")
		for _, c := range cells {
			b.WriteString("<w:tc>" + para(c) + "</w:tc>")
		}
		b.WriteString("</w:tr>")
		return b.String()
	}
	body := para("Før") + "<w:tbl>" + row("Omsetning", "minst 5 MNOK") + row("Referanser", "3 stk", "siste 3 år") + "</w:tbl>" + para("Etter")
	c, err := readDOCX(docxFile(t, body), DefaultLimits)
	if err != nil {
		t.Fatalf("readDOCX: %v", err)
	}
	want := []string{"Før", "Omsetning | minst 5 MNOK", "Referanser | 3 stk | siste 3 år", "Etter"}
	if got := texts(c.Units); !slices.Equal(got, want) {
		t.Errorf("units = %q, want %q", got, want)
	}
}

func TestReadDOCXRefusesWhatIsNotADocx(t *testing.T) {
	t.Parallel()

	var noBody bytes.Buffer
	zw := zip.NewWriter(&noBody)
	w, _ := zw.Create("annet.xml")
	_, _ = w.Write([]byte("<x/>"))
	_ = zw.Close()

	for name, data := range map[string][]byte{
		"not a zip":        []byte("dette er ikke et arkiv"),
		"no document part": noBody.Bytes(),
		"broken xml":       docxFile(t, "<w:p><w:r><w:t>uavsluttet"),
	} {
		if _, err := readDOCX(data, DefaultLimits); !errors.Is(err, ErrMalformed) {
			t.Errorf("%s: err = %v, want ErrMalformed", name, err)
		}
	}
}

// The main part of a DOCX is XML that expands from a compressed file: it is held to a limit of its
// own, whatever the archive around it says.
func TestReadDOCXRefusesAMainPartOverTheLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxXMLBytes = 1 << 10
	if _, err := readDOCX(docxFile(t, para(strings.Repeat("a", 10_000))), lim); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
}

func TestReadDOCXKeepsTheCharacterLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 50
	var body strings.Builder
	for range 20 {
		body.WriteString(para("en avsnittstekst her"))
	}
	c, err := readDOCX(docxFile(t, body.String()), lim)
	if err != nil {
		t.Fatalf("readDOCX: %v", err)
	}
	if c.Chars > lim.MaxChars || !c.Truncated || len(c.Units) == 0 {
		t.Errorf("chars %d truncated %v units %d", c.Chars, c.Truncated, len(c.Units))
	}
}
