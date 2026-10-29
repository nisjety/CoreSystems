// Package docread turns the bytes of a tender document into text, with a place for each piece of it
// (a page number for a PDF, a paragraph number for the rest), so that whatever is found in the text
// can be cited. It reads PDF, DOCX, HTML and plain text. It says so plainly when a file is of a kind
// it does not read (a scan, a legacy Office file, a workbook): never binary noise passed on as text.
//
// The files are from strangers. A PDF is read in a child process that has none of the service's
// environment and a deadline (see pdfchild.go), because the library that reads it panics on damaged
// files and can neither be stopped nor bounded from inside the process; a DOCX is read with a limit
// on the XML it expands to; every kind is held to a limit on the text it may yield.
package docread

import (
	"context"
	"errors"
	"fmt"
	"math"
	"strings"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/unicode/norm"

	"github.com/triodelab/anbud/internal/textsafe"
)

// Kind is what a file is, decided from its bytes.
type Kind string

const (
	KindPDF     Kind = "pdf"
	KindDOCX    Kind = "docx"
	KindHTML    Kind = "html"
	KindText    Kind = "text"
	KindXLSX    Kind = "xlsx"    // recognised, not read
	KindPPTX    Kind = "pptx"    // recognised, not read
	KindOLE     Kind = "ole"     // legacy .doc, .xls, .ppt: recognised, not read
	KindRTF     Kind = "rtf"     // recognised, not read
	KindXML     Kind = "xml"     // recognised, not read
	KindImage   Kind = "image"   // a scan or a picture: it needs OCR, which is not here
	KindArchive Kind = "archive" // an archive that is not a .zip: not opened
	KindOther   Kind = "other"
)

// Unit is a piece of a document's text and where it is: a page of a PDF, or a paragraph of any
// other kind. Exactly one of Page and Paragraph is set, counting from 1.
type Unit struct {
	Page      int
	Paragraph int
	Text      string
}

// Content is what was read from one file.
type Content struct {
	Kind      Kind
	Units     []Unit // only units that hold text
	Pages     int    // the pages of a PDF, all of them, also those past the limit
	Chars     int    // characters in Units
	Truncated bool   // the page limit or the character limit stopped the reading
	Extractor string // "pdf", "docx", "html" or "text"
}

var (
	// ErrUnsupported means the file is of a kind that is not read. Content.Kind says which.
	ErrUnsupported = errors.New("docread: this kind of file is not read")
	// ErrMalformed means the file is damaged, or is not what its bytes say.
	ErrMalformed = errors.New("docread: the file is damaged")
	// ErrEncrypted means the file is protected by a password.
	ErrEncrypted = errors.New("docread: the file is encrypted")
	// ErrTimeout means reading the file took too long, or no reader was free in time.
	ErrTimeout = errors.New("docread: reading the file took too long")
	// ErrFailed means the file could not be read for a reason that is not the file's: the reader
	// crashed, could not start, or answered nonsense.
	ErrFailed = errors.New("docread: the file could not be read")
	// ErrTooLarge means the file is over the size this reader takes.
	ErrTooLarge = errors.New("docread: the file is too large")
)

// Limits bound what a file may yield.
type Limits struct {
	MaxPDFBytes int64 // the size of a PDF
	MaxPDFPages int   // the pages of a PDF that are read
	MaxChars    int   // the characters one file may yield
	// MaxUnits is the units (paragraphs, or rows of a table) one file may yield, whatever their characters: a unit
	// costs a good deal more than the character it may hold, and a file of millions of one-letter paragraphs would
	// otherwise cost hundreds of megabytes, and the finder a pass for each. Not limited when zero.
	MaxUnits    int
	MaxXMLBytes int64 // the XML of a DOCX, after it is expanded
}

// DefaultLimits are what a tender document needs, with room: tender PDFs run to hundreds of pages,
// and the largest notice ever seen was 540 KB of XML.
var DefaultLimits = Limits{
	MaxPDFBytes: 64 << 20,
	MaxPDFPages: 500,
	MaxChars:    3_000_000,
	MaxUnits:    250_000,
	MaxXMLBytes: 64 << 20,
}

// PDFResult is the text of a PDF, one string for each page it read. An empty string is a page with
// no text, kept so that page numbers stay true.
type PDFResult struct {
	Pages      []string
	TotalPages int
	Truncated  bool
}

// PDFRunner reads the text of a PDF. ChildPDF runs the reader in a child process, which is what the
// service uses; InProcessPDF runs it here, for tests and for the child itself.
type PDFRunner interface {
	Run(ctx context.Context, pdf []byte, maxPages int) (PDFResult, error)
}

// Reader reads documents.
type Reader struct {
	pdf PDFRunner
	lim Limits
}

// NewReader returns a Reader that reads PDFs with pdf.
func NewReader(pdf PDFRunner, lim Limits) *Reader { return &Reader{pdf: pdf, lim: lim} }

// Read detects what the bytes are and reads them. A kind that is not read gives ErrUnsupported with
// Content.Kind set, so a caller can say what the file was. A file that holds no text (a scan) gives
// no units and no error.
func (r *Reader) Read(ctx context.Context, data []byte) (Content, error) {
	if err := ctx.Err(); err != nil {
		return Content{}, err
	}
	switch kind := Detect(data); kind {
	case KindPDF:
		return r.readPDF(ctx, data)
	case KindDOCX:
		return readDOCX(data, r.lim)
	case KindHTML:
		return readHTML(data, r.lim)
	case KindText:
		return readText(data, r.lim)
	default:
		return Content{Kind: kind}, ErrUnsupported
	}
}

func (r *Reader) readPDF(ctx context.Context, data []byte) (Content, error) {
	c := Content{Kind: KindPDF, Extractor: "pdf"}
	if int64(len(data)) > r.lim.MaxPDFBytes {
		return c, ErrTooLarge
	}
	res, err := r.pdf.Run(ctx, data, r.lim.MaxPDFPages)
	if err != nil {
		return c, err
	}
	c.Pages, c.Truncated = res.TotalPages, res.Truncated
	for i, page := range res.Pages {
		clipped, cut := clip(page, r.lim.MaxChars)
		if cut {
			c.Truncated = true // text past what could be kept was not read, and the file says so
		}
		if text := cleanText(clipped); text != "" {
			c.Units = append(c.Units, Unit{Page: i + 1, Text: text})
		}
	}
	capUnits(&c, r.lim.MaxChars)
	return c, nil
}

// capUnits counts the characters of the units, and cuts them at max.
func capUnits(c *Content, max int) {
	c.Chars = 0
	for i, u := range c.Units {
		n := utf8.RuneCountInString(u.Text)
		if c.Chars+n > max {
			room := max - c.Chars
			c.Truncated = true
			if room > 0 {
				c.Units[i].Text = string([]rune(u.Text)[:room])
				c.Chars += room
				c.Units = c.Units[:i+1]
			} else {
				c.Units = c.Units[:i]
			}
			return
		}
		c.Chars += n
	}
}

// cleaningLimit is how many bytes of a piece of text are worth cleaning when a file may yield max characters:
// four for each, which is the most a character takes, and a little more. Nothing but that much of a paragraph
// can be part of what is kept, because what is kept is cut at max characters.
func cleaningLimit(max int) int {
	if max <= 0 {
		return math.MaxInt
	}
	return 4*max + 4096
}

// clip is s cut to what is worth cleaning for a file that may yield max characters, at the start of a
// character, and whether it was cut. Cleaning a piece of text takes a good many times its length (the
// characters are counted, split into words and joined again), and the character limit is applied only
// afterwards, so a paragraph of tens of megabytes would otherwise cost the better part of a gigabyte to be cut
// at three million characters. Cleaning can make text shorter, so a paragraph that is mostly blanks loses some
// of what the limit would have let through: the reader that cuts says so.
func clip(s string, max int) (string, bool) {
	n := cleaningLimit(max)
	if len(s) <= n {
		return s, false
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n], true
}

// cleanText brings text from any source to one form: Unicode normal form C, one kind of space, one
// kind of newline, no control or invisible characters (a soft hyphen, a zero-width space, a
// bidirectional override), no runs of blanks, no more than one empty line in a row, no blanks at the
// ends of lines or of the text.
func cleanText(s string) string {
	s = strings.ReplaceAll(s, "\r\n", "\n")
	s = strings.ReplaceAll(s, "\r", "\n")
	s = norm.NFC.String(s)
	var b strings.Builder
	b.Grow(len(s))
	for _, r := range s {
		switch {
		case r == '\n':
			b.WriteByte('\n')
		case r == '\t' || unicode.Is(unicode.Zs, r):
			b.WriteByte(' ')
		case textsafe.Invisible(r):
			// not drawn: a soft hyphen, a zero-width character, a bidirectional control, a tag character, a variation selector
		case unicode.IsControl(r) || r == utf8.RuneError:
		default:
			b.WriteRune(r)
		}
	}

	// The lines are taken one at a time and written as they are made, and not split into a slice first: a text of
	// millions of line breaks would otherwise hold a string header for every one of them, many times its length.
	cleaned := b.String()
	var out strings.Builder
	out.Grow(len(cleaned))
	blank := 0
	first := true
	for more := true; more; {
		var line string
		line, cleaned, more = strings.Cut(cleaned, "\n")
		words := strings.Fields(line)
		if len(words) == 0 {
			if blank++; blank > 1 {
				continue
			}
		} else {
			blank = 0
		}
		if !first {
			out.WriteByte('\n')
		}
		first = false
		for i, w := range words {
			if i > 0 {
				out.WriteByte(' ')
			}
			out.WriteString(w)
		}
	}
	return strings.TrimSpace(out.String())
}

// builder collects the paragraphs of a file into units, and stops when it holds max characters. It
// exists so that a reader need not build every paragraph of a large file before the limit is applied:
// a file of 32 million one-letter lines would otherwise cost gigabytes to be cut at three million characters.
type builder struct {
	c        Content
	max      int
	maxUnits int // 0 for no limit
	full     bool
}

func newBuilder(kind Kind, extractor string, max int) *builder {
	return &builder{c: Content{Kind: kind, Extractor: extractor}, max: max}
}

// builderFor is the builder of a reader that is held to lim.
func builderFor(kind Kind, extractor string, lim Limits) *builder {
	b := newBuilder(kind, extractor, lim.MaxChars)
	b.maxUnits = lim.MaxUnits
	return b
}

// add cleans a paragraph and keeps it when it holds text. It says whether there is room for more: after
// the paragraph that crosses the limit, which is cut at it and marks the content truncated, it is false.
func (b *builder) add(paragraph string) bool {
	if b.full {
		return false
	}
	clipped, cut := clip(paragraph, b.max)
	if cut {
		b.c.Truncated = true // text past what could be kept was not read, and the file says so
	}
	text := cleanText(clipped)
	if text == "" {
		return true
	}
	if b.maxUnits > 0 && len(b.c.Units) >= b.maxUnits {
		b.c.Truncated, b.full = true, true // one more paragraph than a file may yield is one too many
		return false
	}
	n := utf8.RuneCountInString(text)
	if b.c.Chars+n > b.max {
		b.c.Truncated, b.full = true, true
		if room := b.max - b.c.Chars; room > 0 {
			b.c.Units = append(b.c.Units, Unit{Paragraph: len(b.c.Units) + 1, Text: string([]rune(text)[:room])})
			b.c.Chars += room
		}
		return false
	}
	b.c.Units = append(b.c.Units, Unit{Paragraph: len(b.c.Units) + 1, Text: text})
	b.c.Chars += n
	return true
}

// cut notes that a reader cut text before it handed it over, which the file says by being truncated.
func (b *builder) cut() { b.c.Truncated = true }

// malformed wraps ErrMalformed with a fixed description. It never carries the file's own text.
func malformed(what string) error { return fmt.Errorf("%w: %s", ErrMalformed, what) }
