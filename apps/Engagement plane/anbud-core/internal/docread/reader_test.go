package docread

import (
	"context"
	"errors"
	"slices"
	"strings"
	"testing"
)

func reader() *Reader { return NewReader(InProcessPDF{}, DefaultLimits) }

func TestReaderReadsEachKindAndSaysWhereItCameFrom(t *testing.T) {
	t.Parallel()

	r := reader()
	pdfC, err := r.Read(context.Background(), fixturePDF(t))
	if err != nil {
		t.Fatalf("pdf: %v", err)
	}
	if pdfC.Kind != KindPDF || pdfC.Extractor != "pdf" || pdfC.Pages != 2 || len(pdfC.Units) != 2 || pdfC.Units[0].Page != 1 || pdfC.Units[1].Page != 2 || pdfC.Units[0].Paragraph != 0 {
		t.Errorf("pdf: kind %q extractor %q pages %d units %+v", pdfC.Kind, pdfC.Extractor, pdfC.Pages, pdfC.Units)
	}
	if pdfC.Chars == 0 {
		t.Errorf("pdf: no characters counted")
	}

	for name, tc := range map[string]struct {
		data      []byte
		kind      Kind
		extractor string
		want      []string
	}{
		"docx": {docxFile(t, para("Hei fra Word")), KindDOCX, "docx", []string{"Hei fra Word"}},
		"html": {[]byte("<html><body><p>Hei fra nettet</p></body></html>"), KindHTML, "html", []string{"Hei fra nettet"}},
		"text": {[]byte("Hei fra en tekstfil"), KindText, "text", []string{"Hei fra en tekstfil"}},
	} {
		c, err := r.Read(context.Background(), tc.data)
		if err != nil || c.Kind != tc.kind || c.Extractor != tc.extractor || !slices.Equal(texts(c.Units), tc.want) {
			t.Errorf("%s: %+v, %v", name, c, err)
		}
	}
}

func TestReaderSaysWhatItCannotRead(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		data []byte
		kind Kind
	}{
		"an image":      {[]byte("\x89PNG\r\n\x1a\n...."), KindImage},
		"legacy office": {[]byte{0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1}, KindOLE},
		"a workbook":    {zipWith(t, "[Content_Types].xml", "xl/workbook.xml"), KindXLSX},
		"a deck":        {zipWith(t, "[Content_Types].xml", "ppt/presentation.xml"), KindPPTX},
		"rtf":           {[]byte(`{\rtf1 Hei}`), KindRTF},
		"xml":           {[]byte(`<?xml version="1.0"?><a/>`), KindXML},
		"an archive":    {[]byte("Rar!\x1a\x07\x00"), KindArchive},
		"binary":        {[]byte{0, 1, 2, 0, 3}, KindOther},
		"nothing":       {nil, KindOther},
	} {
		c, err := reader().Read(context.Background(), tc.data)
		if !errors.Is(err, ErrUnsupported) || c.Kind != tc.kind || len(c.Units) != 0 {
			t.Errorf("%s: kind %q units %d err %v, want %q and ErrUnsupported", name, c.Kind, len(c.Units), err, tc.kind)
		}
	}
}

// A page that is only a picture has no text. That is not an error: the file was read, and held no
// text, which a person would want told apart from a file that could not be read at all.
func TestReaderGivesNoUnitsForAPDFWithNoTextLayer(t *testing.T) {
	t.Parallel()

	c, err := reader().Read(context.Background(), miniPDF("", ""))
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(c.Units) != 0 || c.Chars != 0 || c.Pages != 2 {
		t.Errorf("units %d chars %d pages %d, want a two-page PDF with no text", len(c.Units), c.Chars, c.Pages)
	}
}

func TestReaderKeepsPageNumbersAcrossAnEmptyPage(t *testing.T) {
	t.Parallel()

	c, err := reader().Read(context.Background(), miniPDF("første", "", "tredje"))
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(c.Units) != 2 || c.Units[0].Page != 1 || c.Units[1].Page != 3 {
		t.Errorf("units = %+v, want pages 1 and 3", c.Units)
	}
}

func TestReaderCleansTheTextItReads(t *testing.T) {
	t.Parallel()

	c, err := reader().Read(context.Background(), miniPDF("a\u00a0\u00a0b"))
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(c.Units) != 1 || c.Units[0].Text != "a b" {
		t.Errorf("units = %+v", c.Units)
	}
}

func TestReaderCapsTheCharactersAcrossPages(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxChars = 30
	c, err := NewReader(InProcessPDF{}, lim).Read(context.Background(), miniPDF(strings.Repeat("a", 20), strings.Repeat("b", 20), strings.Repeat("c", 20)))
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if c.Chars > lim.MaxChars || !c.Truncated || len(c.Units) < 1 {
		t.Errorf("chars %d truncated %v units %d", c.Chars, c.Truncated, len(c.Units))
	}
}

func TestReaderCarriesThePDFPageCapIntoTruncated(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxPDFPages = 2
	c, err := NewReader(InProcessPDF{}, lim).Read(context.Background(), miniPDF("en", "to", "tre"))
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if c.Pages != 3 || len(c.Units) != 2 || !c.Truncated {
		t.Errorf("pages %d units %d truncated %v, want 3 pages, 2 read, truncated", c.Pages, len(c.Units), c.Truncated)
	}
}

func TestReaderRefusesAPDFOverItsByteLimit(t *testing.T) {
	t.Parallel()

	lim := DefaultLimits
	lim.MaxPDFBytes = 100
	if _, err := NewReader(InProcessPDF{}, lim).Read(context.Background(), fixturePDF(t)); !errors.Is(err, ErrTooLarge) {
		t.Errorf("err = %v, want ErrTooLarge", err)
	}
}

func TestReaderStopsOnACancelledContext(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := reader().Read(ctx, fixturePDF(t)); !errors.Is(err, context.Canceled) {
		t.Errorf("err = %v, want context.Canceled", err)
	}
}

// What the reader says of a damaged file does not depend on which runner reads PDFs.
func TestReaderPassesAPDFErrorThrough(t *testing.T) {
	t.Parallel()

	if _, err := reader().Read(context.Background(), []byte("%PDF-1.4\nnonsense")); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
}
