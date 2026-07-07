// Package docgen produces small, dependency-free PDF documents: demo
// shipping labels for the mock carrier fleet, CN22-style customs
// declarations, and end-of-day manifests. The writer emits genuinely valid
// single-page PDFs (PDF 1.4, built-in Helvetica, correct xref table) so
// downloads open in any viewer — while staying a few hundred lines instead
// of pulling in a PDF library for what is essentially "text on a page".
// Real carrier labels come from the carrier APIs, never from here.
package docgen

import (
	"bytes"
	"fmt"
	"strings"
)

// Line is one row of text on the page.
type Line struct {
	Text string
	Size int  // font size in points; 0 → 11
	Bold bool // uses Helvetica-Bold
}

// H1 is a convenience constructor for a heading line.
func H1(text string) Line { return Line{Text: text, Size: 20, Bold: true} }

// H2 is a convenience constructor for a sub-heading line.
func H2(text string) Line { return Line{Text: text, Size: 14, Bold: true} }

// Txt is a convenience constructor for a body line.
func Txt(text string) Line { return Line{Text: text} }

// A6 landscape (shipping-label-ish) and A4 portrait page sizes in points.
var (
	PageLabel = [2]int{420, 298} // A6 landscape
	PageA4    = [2]int{595, 842} // A4 portrait
)

// PDF renders lines top-down on a single page of the given size and returns
// the complete PDF document bytes.
func PDF(page [2]int, lines []Line) []byte {
	var content bytes.Buffer
	y := page[1] - 40
	for _, line := range lines {
		size := line.Size
		if size <= 0 {
			size = 11
		}
		font := "/F1"
		if line.Bold {
			font = "/F2"
		}
		// Skip past the line height before drawing so consecutive lines stack.
		y -= size + 6
		if y < 20 {
			break // single-page writer: clip honestly rather than overflow
		}
		fmt.Fprintf(&content, "BT %s %d Tf 24 %d Td (%s) Tj ET\n", font, size, y, escapePDFText(line.Text))
	}

	objects := []string{
		"<< /Type /Catalog /Pages 2 0 R >>",
		"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
		fmt.Sprintf("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>", page[0], page[1]),
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>",
		fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", content.Len(), content.String()),
	}

	var out bytes.Buffer
	out.WriteString("%PDF-1.4\n")
	offsets := make([]int, len(objects)+1)
	for i, obj := range objects {
		offsets[i+1] = out.Len()
		fmt.Fprintf(&out, "%d 0 obj\n%s\nendobj\n", i+1, obj)
	}
	xrefStart := out.Len()
	fmt.Fprintf(&out, "xref\n0 %d\n", len(objects)+1)
	out.WriteString("0000000000 65535 f \n")
	for i := 1; i <= len(objects); i++ {
		fmt.Fprintf(&out, "%010d 00000 n \n", offsets[i])
	}
	fmt.Fprintf(&out, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(objects)+1, xrefStart)
	return out.Bytes()
}

// escapePDFText escapes the characters PDF string literals reserve. Non-ASCII
// is transliterated to '?' — the built-in Helvetica encoding here is ASCII-safe
// only, and a demo label must never silently corrupt an address.
func escapePDFText(s string) string {
	var b strings.Builder
	for _, r := range s {
		switch {
		case r == '(' || r == ')' || r == '\\':
			b.WriteByte('\\')
			b.WriteRune(r)
		case r > 126 || r < 32:
			b.WriteByte('?')
		default:
			b.WriteRune(r)
		}
	}
	return b.String()
}
