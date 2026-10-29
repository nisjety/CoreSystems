package docread

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"

	"github.com/ledongthuc/pdf"
)

func fixturePDF(t *testing.T) []byte {
	t.Helper()
	data, err := os.ReadFile("testdata/konkurransegrunnlag.pdf")
	if err != nil {
		t.Fatalf("read the fixture: %v", err)
	}
	return data
}

// The fixture is a PDF made by a browser, which is how most tender PDFs are made: embedded
// subset fonts and a character map, not a hand-written Helvetica. Its text is invented.
func TestExtractPDFReadsTheTextOfEachPage(t *testing.T) {
	t.Parallel()

	res, err := ExtractPDF(fixturePDF(t), 100)
	if err != nil {
		t.Fatalf("ExtractPDF: %v", err)
	}
	if res.TotalPages != 2 || len(res.Pages) != 2 || res.Truncated {
		t.Fatalf("pages %d (%d read), truncated %v; want 2 pages", res.TotalPages, len(res.Pages), res.Truncated)
	}
	for _, want := range []string{"Kvalifikasjonskrav", "sertifisert etter ISO 9001 eller tilsvarende", "registrert i StartBANK", "omsetning på minst 5 MNOK", "æøå ÆØÅ"} {
		if !strings.Contains(res.Pages[0], want) {
			t.Errorf("page 1 does not hold %q:\n%s", want, res.Pages[0])
		}
	}
	for _, want := range []string{"Tildelingskriterier", "trenger ikke Miljøfyrtårn-", "sertifisering"} {
		if !strings.Contains(res.Pages[1], want) {
			t.Errorf("page 2 does not hold %q:\n%s", want, res.Pages[1])
		}
	}
	if strings.Contains(res.Pages[0], "Tildelingskriterier") {
		t.Errorf("page 2's text is on page 1")
	}
}

func TestExtractPDFKeepsAPagePlaceWhenAPageHasNoText(t *testing.T) {
	t.Parallel()

	res, err := ExtractPDF(miniPDF("Side en", "", "Side tre"), 100)
	if err != nil {
		t.Fatalf("ExtractPDF: %v", err)
	}
	got := []string{}
	for _, p := range res.Pages {
		got = append(got, strings.TrimSpace(p))
	}
	if len(got) != 3 || got[0] != "Side en" || got[1] != "" || got[2] != "Side tre" {
		t.Errorf("pages = %q, want the empty page kept in its place so page 3 stays page 3", got)
	}
}

func TestExtractPDFReadsNorwegianLettersFromAWinAnsiFont(t *testing.T) {
	t.Parallel()

	res, err := ExtractPDF(miniPDF("Størrelse på ærlig Åpent ØL"), 100)
	if err != nil {
		t.Fatalf("ExtractPDF: %v", err)
	}
	if got := strings.TrimSpace(res.Pages[0]); got != "Størrelse på ærlig Åpent ØL" {
		t.Errorf("text = %q", got)
	}
}

func TestExtractPDFStopsAtThePageLimit(t *testing.T) {
	t.Parallel()

	res, err := ExtractPDF(miniPDF("en", "to", "tre", "fire", "fem"), 3)
	if err != nil {
		t.Fatalf("ExtractPDF: %v", err)
	}
	if len(res.Pages) != 3 || res.TotalPages != 5 || !res.Truncated {
		t.Errorf("read %d of %d pages, truncated %v; want 3 of 5, truncated", len(res.Pages), res.TotalPages, res.Truncated)
	}
	if res, err := ExtractPDF(miniPDF("en", "to", "tre"), 3); err != nil || res.Truncated || len(res.Pages) != 3 {
		t.Errorf("exactly the limit: %d pages, truncated %v, %v", len(res.Pages), res.Truncated, err)
	}
}

// The library panics on bad input in many places. Whatever the bytes are, the answer is an error.
func TestExtractPDFAnswersAnErrorWhateverTheBytesAre(t *testing.T) {
	t.Parallel()

	good := fixturePDF(t)
	mini := miniPDF("tekst")
	for name, data := range map[string][]byte{
		"nothing":           nil,
		"text":              []byte("dette er ikke en pdf"),
		"only a header":     []byte("%PDF-1.7\n"),
		"header and noise":  append([]byte("%PDF-1.4\n"), []byte(strings.Repeat("\x00\xff\x7f noise ", 500))...),
		"half a real file":  good[:len(good)/2],
		"the start":         good[:200],
		"the end":           good[len(good)-300:],
		"a mini pdf, cut":   mini[:len(mini)-60],
		"a damaged xref":    []byte(strings.Replace(string(mini), "xref", "xxxx", 1)),
		"a wrong object id": []byte(strings.Replace(string(mini), "1 0 obj", "9 0 obj", 1)),
	} {
		func() {
			defer func() {
				if r := recover(); r != nil {
					t.Errorf("%s: a panic got out: %v", name, r)
				}
			}()
			if _, err := ExtractPDF(data, 100); err == nil {
				t.Errorf("%s: no error", name)
			} else if !errors.Is(err, ErrMalformed) {
				t.Errorf("%s: err = %v, want ErrMalformed", name, err)
			}
		}()
	}
}

func TestMapPDFErrorSaysEncryptedForAPasswordError(t *testing.T) {
	t.Parallel()

	if err := mapPDFError(fmt.Errorf("open: %w", pdf.ErrInvalidPassword)); !errors.Is(err, ErrEncrypted) {
		t.Errorf("err = %v, want ErrEncrypted", err)
	}
	if err := mapPDFError(errors.New("malformed PDF: reading at offset 0: stream not present")); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
}
