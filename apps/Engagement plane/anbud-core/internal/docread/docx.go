package docread

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"errors"
	"io"
	"strings"

	"github.com/triodelab/anbud/internal/ziputil"
)

// The namespaces of WordprocessingML: the transitional one Word writes, and the strict one of ISO 29500.
const (
	nsWordTransitional = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
	nsWordStrict       = "http://purl.oclc.org/ooxml/wordprocessingml/main"
)

// maxDOCXEntries is how many parts a DOCX may list. A real one has a few dozen.
const maxDOCXEntries = 10_000

// maxXMLTag is the most a token of markup may take of a main part: a tag from its "<" to its ">" with the
// values of its attributes, or a comment, an instruction, a declaration or a CDATA section. A real tag is a few
// hundred bytes, and the root of a document, which declares the namespaces, a few thousand. The decoder holds
// every attribute and every namespace declaration of a tag it has read, so a tag of four million attributes costs
// half a gigabyte from nineteen megabytes of XML, and the limit on the part's size does not stop it.
const maxXMLTag = 64 << 10

// maxXMLOpenTags is the most the start tags of the elements that are open at one time may take together. The
// decoder holds the namespace declarations of every element that is open, so tags that are each within
// maxXMLTag could still cost a great deal when two hundred and fifty of them are nested. A real document's are a
// few thousand bytes in all.
const maxXMLOpenTags = 256 << 10

// xmlTextLimit is the most a run of text between two tags may take of a main part: what the character limit
// allows, at the most that each of its characters is written as an entity, and no more. The decoder holds the
// whole run, and the character limit cuts what is read only after it.
func xmlTextLimit(maxChars int) int64 {
	if maxChars <= 0 {
		return 64 << 20
	}
	return int64(maxChars)*8 + 4096
}

// maxXMLDepth is how deep the elements of a main part may nest. A real document is under thirty. The
// decoder keeps every element that is open, so a part of nothing but unclosed tags would otherwise cost
// memory for each of them.
const maxXMLDepth = 256

// readDOCX reads the text of a Word document: a paragraph for each paragraph, the cells of a table
// row on one line separated by " | ". Text that was deleted with tracked changes is not text any
// more, and text that was inserted is. Headers, footers, notes and field codes are not read.
func readDOCX(data []byte, lim Limits) (Content, error) {
	c := Content{Kind: KindDOCX, Extractor: "docx"}
	zr, err := ziputil.NewReader(data, maxDOCXEntries)
	if err != nil {
		return c, malformed("not a DOCX")
	}
	var part *zip.File
	for _, f := range zr.File {
		if strings.EqualFold(f.Name, "word/document.xml") {
			part = f
			break
		}
	}
	if part == nil {
		return c, malformed("no main document part")
	}
	if part.UncompressedSize64 > uint64(lim.MaxXMLBytes) {
		return c, malformed("the main part expands beyond the limit")
	}
	rc, err := part.Open()
	if err != nil {
		return c, malformed("the main part cannot be opened")
	}
	defer rc.Close()

	b := builderFor(KindDOCX, "docx", lim)
	if err := wordParagraphs(io.LimitReader(rc, lim.MaxXMLBytes+1), lim.MaxChars, b.add, b.cut); err != nil {
		return c, err
	}
	return b.c, nil
}

func isWord(n xml.Name) bool { return n.Space == nsWordTransitional || n.Space == nsWordStrict }

// wordTable is where the reading of a table is: the row and the cell it is in, and what they hold so far. Tables
// nest, a table in a cell of another, so each has its own, and the one that is open is the one on the top.
type wordTable struct {
	inRow, inCell bool
	cell          []string // paragraphs of the current cell
	cellLen       int      // their bytes
	rowCells      []string
	rowLen        int // the bytes of the cells of the row so far
}

// elementWeight is what an element of a cell or a row (a paragraph, a cell) is counted as in bytes, besides its own
// text: it costs a string and a place in a slice, and a cell of millions of one-byte paragraphs would otherwise
// cost many times what its text comes to.
const elementWeight = 32

// wordParagraphs decodes WordprocessingML into paragraphs of text, which it hands to emit as it makes them: a
// paragraph, or a row of a table with its cells on one line. It stops reading when emit says it wants no more,
// which is when the character limit has been reached. It calls cut when it leaves text out because there is more
// of it than could be kept (the rest of a paragraph, of a cell or of a row), which the file says by being truncated.
func wordParagraphs(r io.Reader, maxChars int, emit func(string) bool, cut func()) error {
	meter := newXMLMeter(r, maxXMLTag, xmlTextLimit(maxChars))
	d := xml.NewDecoder(meter)
	d.Strict = true

	var (
		para    strings.Builder
		inText  bool
		deleted int // depth of w:del we are inside
		tables  []wordTable
		depth   int  // elements open now
		hasText bool // the paragraph so far holds something that is not blank
		full    bool // emit wants no more
		// openTags is what the start tags of the elements that are open took of the input, and tagSizes what
		// each of them took.
		openTags int64
		tagSizes []int64
	)
	put := func(text string) {
		if !emit(text) {
			full = true
		}
	}
	// table is the one that is open. A row or a cell outside any table (a document that is not well made) gets
	// one of its own, which nothing closes.
	table := func() *wordTable {
		if len(tables) == 0 {
			tables = append(tables, wordTable{})
		}
		return &tables[len(tables)-1]
	}
	endParagraph := func() {
		text := strings.TrimSpace(para.String())
		para.Reset()
		hasText = false
		if text == "" {
			return
		}
		if len(tables) > 0 && tables[len(tables)-1].inCell {
			// What a cell holds past what is worth cleaning is never kept, and the cell is not made larger for it.
			if t := &tables[len(tables)-1]; t.cellLen < cleaningLimit(maxChars) {
				t.cell = append(t.cell, text)
				t.cellLen += len(text) + elementWeight
			} else {
				cut()
			}
			return
		}
		put(text)
	}

	for !full {
		meter.begin()
		tok, err := d.Token()
		if err != nil {
			switch {
			case errors.Is(err, io.EOF):
			case errors.Is(err, errXMLTokenTooLong):
				return malformed("the main part holds a token that is too long")
			default:
				return malformed("the main part is not valid XML")
			}
			break
		}
		switch t := tok.(type) {
		case xml.StartElement:
			if depth++; depth > maxXMLDepth {
				return malformed("the main part is nested too deeply")
			}
			used := meter.used()
			if openTags += used; openTags > maxXMLOpenTags {
				return malformed("the start tags of the main part are too long together")
			}
			tagSizes = append(tagSizes, used)
			if !isWord(t.Name) {
				continue
			}
			switch t.Name.Local {
			case "p":
				// A paragraph inside another (a text box) ends the text so far. Whether there is any is a flag
				// and not a scan of the paragraph: a paragraph of megabytes of blanks would be scanned at
				// every one of the paragraphs inside it.
				if hasText {
					endParagraph()
				}
			case "t":
				inText = true
			case "del":
				deleted++
			case "tab", "br", "cr", "noBreakHyphen":
				if deleted == 0 {
					para.WriteByte(' ')
				}
			case "tbl":
				tables = append(tables, wordTable{})
			case "tr":
				tb := table()
				tb.inRow, tb.rowCells, tb.rowLen = true, nil, 0
			case "tc":
				tb := table()
				tb.inCell, tb.cell, tb.cellLen = true, nil, 0
			}
		case xml.EndElement:
			depth--
			if n := len(tagSizes); n > 0 {
				openTags -= tagSizes[n-1]
				tagSizes = tagSizes[:n-1]
			}
			if !isWord(t.Name) {
				continue
			}
			switch t.Name.Local {
			case "t":
				inText = false
			case "del":
				if deleted > 0 {
					deleted--
				}
			case "p":
				endParagraph()
			case "tc":
				tb := table()
				if text := strings.Join(tb.cell, " "); text != "" {
					if tb.rowLen < cleaningLimit(maxChars) {
						tb.rowCells = append(tb.rowCells, text)
						tb.rowLen += len(text) + elementWeight
					} else {
						cut()
					}
				}
				tb.inCell, tb.cell, tb.cellLen = false, nil, 0
			case "tr":
				tb := table()
				if tb.inRow && len(tb.rowCells) > 0 {
					put(strings.Join(tb.rowCells, " | "))
				}
				tb.inRow, tb.rowCells, tb.rowLen = false, nil, 0
			case "tbl":
				if n := len(tables); n > 0 {
					tables = tables[:n-1]
				}
			}
		case xml.CharData:
			if inText && deleted == 0 {
				if para.Len() < cleaningLimit(maxChars) {
					para.Write(t)
				} else if len(t) > 0 {
					cut() // what is past that is never kept, and the paragraph is not made larger for it
				}
				if !hasText && len(bytes.TrimSpace(t)) > 0 {
					hasText = true
				}
			}
		}
	}
	if !full {
		endParagraph()
	}
	return nil
}
