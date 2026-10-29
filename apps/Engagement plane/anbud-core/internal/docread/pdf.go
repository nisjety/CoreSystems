package docread

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/ledongthuc/pdf"
)

// ExtractPDF reads the text of the first maxPages pages of a PDF, one string for each. A page with
// no text is an empty string, so page numbers stay true. It answers an error for anything it cannot
// read and never panics: the library underneath panics on damaged files in many places, and each is
// caught here, for the whole file and for each page.
//
// It runs in the process that calls it. The service does not call it directly: it runs it in a child
// process (ChildPDF), where a file that makes the library loop or allocate without end costs a process
// that is killed, not the service.
func ExtractPDF(data []byte, maxPages int) (res PDFResult, err error) {
	defer func() {
		if r := recover(); r != nil {
			res, err = PDFResult{}, malformed("the reader gave up on the file")
		}
	}()
	if len(data) == 0 {
		return PDFResult{}, malformed("the file is empty")
	}
	r, err := pdf.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return PDFResult{}, mapPDFError(err)
	}
	return collectPages(r.NumPage(), maxPages, func(number int) (string, bool) {
		return safePage(func() (string, error) { return r.Page(number).GetPlainText(nil) })
	})
}

// collectPages reads the first maxPages of the total pages with readPage, which says whether it could
// read the page. A page that could not be read is an empty page, so that page numbers stay true; if no
// page could be read the file is damaged, and if some could it is not.
func collectPages(total, maxPages int, readPage func(number int) (string, bool)) (PDFResult, error) {
	if total <= 0 {
		return PDFResult{}, malformed("the file has no pages")
	}
	n, truncated := total, false
	if maxPages > 0 && n > maxPages {
		n, truncated = maxPages, true
	}
	pages := make([]string, n)
	failed := 0
	for i := 1; i <= n; i++ {
		text, ok := readPage(i)
		if !ok {
			failed++
		}
		pages[i-1] = text
	}
	if failed == n {
		// Every page failed: the file was opened and nothing could be read of it. That is a damaged
		// file, not a file with no text.
		return PDFResult{}, malformed("no page could be read")
	}
	return PDFResult{Pages: pages, TotalPages: total, Truncated: truncated}, nil
}

// safePage reads one page and says whether it could: the library panics on a damaged page, and that is
// that page's failure and not the file's.
func safePage(read func() (string, error)) (text string, ok bool) {
	defer func() {
		if recover() != nil {
			text, ok = "", false
		}
	}()
	t, err := read()
	if err != nil {
		return "", false
	}
	return t, true
}

// mapPDFError says what an error from opening a PDF means. A password error is its own answer: the
// file is not damaged, it is closed.
func mapPDFError(err error) error {
	if errors.Is(err, pdf.ErrInvalidPassword) || strings.Contains(strings.ToLower(err.Error()), "encrypt") {
		return fmt.Errorf("%w: the file asks for a password", ErrEncrypted)
	}
	return malformed("the file cannot be opened as a PDF")
}

// InProcessPDF reads PDFs in the calling process. See ExtractPDF for why the service does not.
type InProcessPDF struct{}

// Run reads the PDF.
func (InProcessPDF) Run(ctx context.Context, data []byte, maxPages int) (PDFResult, error) {
	if err := ctx.Err(); err != nil {
		return PDFResult{}, err
	}
	return ExtractPDF(data, maxPages)
}
