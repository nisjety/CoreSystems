// Package extract turns raw file bytes captured from Microsoft Graph into the
// plain UTF-8 text that Data Plane v2's POST /v1/documents requires. It is
// deliberately narrow: it knows how to read plain-text formats and PDFs, and
// reports every other format as "unsupported" so the caller skips it cleanly
// rather than forwarding binary noise as document content.
package extract

import (
	"bytes"
	"fmt"
	"html"
	"path/filepath"
	"strings"
	"unicode"

	"github.com/ledongthuc/pdf"
)

// Kind classifies how a file should be text-extracted.
type Kind int

const (
	// KindUnsupported means we have no honest way to pull text out of the file
	// (images, archives, office binaries we don't parse, …). Callers skip these.
	KindUnsupported Kind = iota
	// KindPlainText means the bytes are (or decode as) UTF-8 text.
	KindPlainText
	// KindPDF means the bytes are a PDF to run through the PDF text extractor.
	KindPDF
)

// Classify decides how to extract a file, preferring the Graph-reported MIME
// type and falling back to the filename extension. Exported so callers (and
// tests) can pre-filter without downloading unsupported bytes.
func Classify(mimeType, name string) Kind {
	m := strings.ToLower(strings.TrimSpace(mimeType))
	if i := strings.IndexByte(m, ';'); i >= 0 { // drop "; charset=utf-8"
		m = strings.TrimSpace(m[:i])
	}
	switch m {
	case "application/pdf":
		return KindPDF
	case "application/json", "application/x-ndjson",
		"application/xml", "application/markdown", "application/x-yaml":
		return KindPlainText
	}
	if strings.HasPrefix(m, "text/") {
		return KindPlainText
	}
	switch strings.ToLower(filepath.Ext(name)) {
	case ".pdf":
		return KindPDF
	case ".txt", ".md", ".markdown", ".csv", ".tsv", ".json",
		".xml", ".log", ".yaml", ".yml", ".html", ".htm", ".rst":
		return KindPlainText
	}
	return KindUnsupported
}

// Extract pulls plain UTF-8 text out of a file's bytes.
//
// Returns:
//   - text: the extracted plain text (empty when unsupported)
//   - supported: whether the format is one we know how to extract. Callers
//     should silently skip unsupported files (images, binaries, …).
//   - err: a real extraction failure for a SUPPORTED format (e.g. a corrupt
//     PDF). This is best-effort at the call site: one bad file must not abort a
//     whole sync, so callers log it and move on.
func Extract(mimeType, name string, data []byte) (text string, supported bool, err error) {
	switch Classify(mimeType, name) {
	case KindPlainText:
		return toValidUTF8(string(data)), true, nil
	case KindPDF:
		out, perr := extractPDF(data)
		return out, true, perr
	default:
		return "", false, nil
	}
}

// extractPDF runs the pure-Go PDF text extractor. The underlying library can
// panic on malformed/exotic PDFs, so we recover and surface it as a normal
// error — a single unparseable file must never crash the sync loop.
func extractPDF(data []byte) (out string, err error) {
	defer func() {
		if r := recover(); r != nil {
			out = ""
			err = fmt.Errorf("pdf extraction panicked: %v", r)
		}
	}()
	if len(data) == 0 {
		return "", fmt.Errorf("empty pdf payload")
	}
	r, e := pdf.NewReader(bytes.NewReader(data), int64(len(data)))
	if e != nil {
		return "", fmt.Errorf("open pdf: %w", e)
	}
	textReader, e := r.GetPlainText()
	if e != nil {
		return "", fmt.Errorf("read pdf text: %w", e)
	}
	var buf bytes.Buffer
	if _, e := buf.ReadFrom(textReader); e != nil {
		return "", fmt.Errorf("buffer pdf text: %w", e)
	}
	return toValidUTF8(strings.TrimSpace(buf.String())), nil
}

// toValidUTF8 strips invalid UTF-8 sequences. Data Plane v2 rejects documents
// whose content is not valid UTF-8, so this guarantees the extractor never
// produces a body the receiver will 400 on.
func toValidUTF8(s string) string {
	return strings.ToValidUTF8(s, "")
}

// HTMLToText reduces an HTML fragment to plain text: script/style subtrees
// are dropped whole, block-level tags become newlines, every other tag a
// space, entities are decoded, and whitespace is collapsed. It backs the
// SharePoint site-pages ingest, where Graph hands back each text web part as
// an `innerHtml` fragment — the forwarding contract is the same as for files
// (plain UTF-8 text only), so tags must not leak through as content.
func HTMLToText(input string) string {
	if strings.TrimSpace(input) == "" {
		return ""
	}

	var out strings.Builder
	out.Grow(len(input))
	rest := input
	for {
		lt := strings.IndexByte(rest, '<')
		if lt < 0 {
			out.WriteString(rest)
			break
		}
		out.WriteString(rest[:lt])
		rest = rest[lt:]

		gt := strings.IndexByte(rest, '>')
		if gt < 0 {
			// Unterminated tag — nothing textual left worth keeping.
			break
		}
		tag := strings.ToLower(strings.TrimSpace(strings.Trim(rest[1:gt], "/ ")))
		if name, _, _ := strings.Cut(tag, " "); name != "" {
			tag = name
		}
		rest = rest[gt+1:]

		switch tag {
		case "script", "style":
			// Skip everything up to (and including) the matching close tag.
			if end := strings.Index(strings.ToLower(rest), "</"+tag); end >= 0 {
				rest = rest[end:]
				if gt := strings.IndexByte(rest, '>'); gt >= 0 {
					rest = rest[gt+1:]
				} else {
					rest = ""
				}
			} else {
				rest = ""
			}
		case "p", "div", "br", "li", "ul", "ol", "tr", "table", "blockquote",
			"section", "article", "h1", "h2", "h3", "h4", "h5", "h6":
			out.WriteByte('\n')
		default:
			out.WriteByte(' ')
		}
	}

	return collapseWhitespace(toValidUTF8(html.UnescapeString(out.String())))
}

// collapseWhitespace normalizes runs of spaces/tabs to one space and runs of
// newlines (with surrounding spaces) to one newline, then trims the result.
func collapseWhitespace(s string) string {
	var out strings.Builder
	out.Grow(len(s))
	pendingSpace := false
	pendingNewline := false
	for _, r := range s {
		switch {
		case r == '\n' || r == '\r':
			pendingNewline = true
			pendingSpace = false
		case unicode.IsSpace(r):
			pendingSpace = true
		default:
			if pendingNewline {
				if out.Len() > 0 {
					out.WriteByte('\n')
				}
			} else if pendingSpace && out.Len() > 0 {
				out.WriteByte(' ')
			}
			pendingNewline = false
			pendingSpace = false
			out.WriteRune(r)
		}
	}
	return out.String()
}
