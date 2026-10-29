package docread

import (
	"bytes"
	"strings"

	"github.com/triodelab/anbud/internal/ziputil"
)

// maxDirectoryEntries is how many entries of a ZIP-based file Detect will list to tell a DOCX from
// a workbook. More than any office file has; a file with more is not one.
const maxDirectoryEntries = 10_000

// Detect says what a file is, from its bytes alone. The name a file was sent under is the sender's
// word, and nothing here reads it. A file is a PDF only when the PDF header opens it, so a sentence
// that mentions the header is still text.
func Detect(data []byte) Kind {
	if len(data) == 0 {
		return KindOther
	}
	head := data
	if len(head) > 1024 {
		head = head[:1024]
	}

	if i := bytes.Index(head, []byte("%PDF-")); i >= 0 && len(bytes.Trim(head[:i], " \t\r\n\x00\xef\xbb\xbf")) == 0 {
		return KindPDF
	}
	switch {
	case bytes.HasPrefix(data, []byte{0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1}):
		return KindOLE
	case bytes.HasPrefix(data, []byte(`{\rtf`)):
		return KindRTF
	case bytes.HasPrefix(data, []byte("\x89PNG\r\n\x1a\n")), bytes.HasPrefix(data, []byte{0xFF, 0xD8, 0xFF}),
		bytes.HasPrefix(data, []byte("GIF87a")), bytes.HasPrefix(data, []byte("GIF89a")),
		bytes.HasPrefix(data, []byte{'I', 'I', '*', 0}), bytes.HasPrefix(data, []byte{'M', 'M', 0, '*'}):
		return KindImage
	case bytes.HasPrefix(data, []byte{'7', 'z', 0xBC, 0xAF, 0x27, 0x1C}), bytes.HasPrefix(data, []byte("Rar!\x1a\x07")),
		bytes.HasPrefix(data, []byte{0x1F, 0x8B}):
		return KindArchive
	case bytes.HasPrefix(data, []byte("PK\x03\x04")):
		return detectZipBased(data)
	}
	return detectText(data)
}

// detectZipBased tells a DOCX, a workbook and a deck from an archive by the parts they hold.
func detectZipBased(data []byte) Kind {
	zr, err := ziputil.NewReader(data, maxDirectoryEntries)
	if err != nil {
		return KindOther
	}
	var word, sheet, slides bool
	for _, f := range zr.File {
		switch strings.ToLower(f.Name) {
		case "word/document.xml":
			word = true
		case "xl/workbook.xml":
			sheet = true
		case "ppt/presentation.xml":
			slides = true
		}
	}
	switch {
	case word:
		return KindDOCX
	case sheet:
		return KindXLSX
	case slides:
		return KindPPTX
	}
	return KindArchive
}

// detectText says whether the bytes are text, and of which kind. Text is UTF-8, UTF-16 with a byte
// order mark, or Windows-1252 (what older Norwegian software writes) with no control characters.
func detectText(data []byte) Kind {
	if bytes.HasPrefix(data, []byte{0xFF, 0xFE}) || bytes.HasPrefix(data, []byte{0xFE, 0xFF}) {
		return KindText
	}
	sample := data
	if len(sample) > 8192 {
		sample = sample[:8192]
	}
	for _, b := range sample {
		if b == 0 || (b < 0x20 && b != '\t' && b != '\n' && b != '\r' && b != '\f') || b == 0x7f {
			return KindOther
		}
	}

	start := bytes.TrimLeft(bytes.TrimPrefix(sample, []byte{0xEF, 0xBB, 0xBF}), " \t\r\n")
	lower := strings.ToLower(string(start[:minInt(len(start), 64)]))
	switch {
	case strings.HasPrefix(lower, "<!doctype html"), strings.HasPrefix(lower, "<html"),
		strings.HasPrefix(lower, "<head"), strings.HasPrefix(lower, "<body"):
		return KindHTML
	case strings.HasPrefix(lower, "<?xml"):
		return KindXML
	}
	return KindText
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}
