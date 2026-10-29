package eforms

import (
	"bytes"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"regexp"
	"slices"
	"strings"
)

var (
	// ErrNotEForms means the document is not an eForms notice this package can
	// read: it is not well-formed XML, it is not shaped like a UBL notice, or it
	// carries no usable CustomizationID. Callers must fail closed on it rather
	// than guess.
	ErrNotEForms = errors.New("eforms: not an eForms notice")

	// ErrTooLarge means the document is over MaxNoticeBytes. Unlike ErrNotEForms
	// it says nothing about what the document is, so a caller should quarantine
	// it for a person to look at instead of discarding it.
	ErrTooLarge = errors.New("eforms: notice exceeds the size limit")
)

// Limits. The largest notice captured from Doffin is about 540 KB, 8,130
// elements, 12 levels deep, with at most 8 attributes on an element, 9 namespace
// declarations and 2.8 KB between two tags. Each limit sits at least 7 times
// above what was seen: far enough that no real notice is refused, close enough
// that a hostile one cannot make the sniffer allocate much more than the input.
const (
	// MaxNoticeBytes is the largest document Sniff reads.
	MaxNoticeBytes = 8 << 20

	// maxRunBytes bounds the stretch between two '<' bytes, which is the longest a
	// single start tag (and so its attribute list) or a text node can be.
	// encoding/xml builds a start tag's whole attribute list in memory, so without
	// this one tag of attribute stuffing costs roughly 18 times its own size.
	maxRunBytes = 1 << 20

	maxDepth            = 64
	maxElements         = 500_000
	maxAttributes       = 64 // on one element
	maxNamespaceDecls   = 64 // in the whole document; encoding/xml keeps them until their element closes
	maxHeaderValueBytes = 512
)

// XML namespaces of the notice header. Elements are matched by namespace and
// local name, never by prefix, which is whatever the author chose.
const (
	nsUBLPrefix = "urn:oasis:names:specification:ubl:schema:xsd:"
	nsCBC       = nsUBLPrefix + "CommonBasicComponents-2"
	nsEXT       = nsUBLPrefix + "CommonExtensionComponents-2"
	nsEFEXT     = "http://data.europa.eu/p27/eforms-ubl-extensions/1"
	nsEFAC      = "http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1"
)

// subTypeAncestors is the path from the root to cbc:SubTypeCode, which is the
// only place a notice states its own subtype.
var subTypeAncestors = [...]xml.Name{
	{Space: nsEXT, Local: "UBLExtensions"},
	{Space: nsEXT, Local: "UBLExtension"},
	{Space: nsEXT, Local: "ExtensionContent"},
	{Space: nsEFEXT, Local: "EformsExtension"},
	{Space: nsEFAC, Local: "NoticeSubType"},
}

var utf8BOM = []byte{0xEF, 0xBB, 0xBF}

// Flavour is the national variant of eForms, named by the suffix the Norwegian
// SDK adds to cbc:CustomizationID.
type Flavour string

const (
	// FlavourEU is a notice submitted to TED (above threshold or voluntary).
	FlavourEU Flavour = "eu"
	// FlavourNational is a Norwegian national notice (below threshold).
	FlavourNational Flavour = "national"
	// FlavourNationalE is the 2026 below-threshold variant (in testing).
	FlavourNationalE Flavour = "national-e"
)

// Known reports whether f is a flavour this code has field maps for. A notice
// with any other flavour, including none, must not be parsed by guesswork.
func (f Flavour) Known() bool {
	switch f {
	case FlavourEU, FlavourNational, FlavourNationalE:
		return true
	}
	return false
}

// Header is what can be learned about a notice from its first elements. It is
// how a notice is routed to the right field map and code lists, and how an
// unexpected document is recognised before any real parsing.
//
// The Doffin identifier and the TED publication number are deliberately absent:
// they are not part of the eForms payload (DFØ, "Notice identifiers").
type Header struct {
	Root            string  // local name of the root element, e.g. "ContractNotice"
	CustomizationID string  // verbatim, e.g. "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu"
	SDKVersion      string  // "1.14"
	Flavour         Flavour // empty for plain EU eForms with no Norwegian extension
	NoticeID        string  // cbc:ID, the stable notice-identifier
	VersionID       string  // cbc:VersionID, the notice-version, e.g. "01"
	ProcedureID     string  // cbc:ContractFolderID, the procedure-identifier; absent on planning notices
	NoticeTypeCode  string  // e.g. "cn-standard"
	SubTypeCode     string  // eForms notice subtype, e.g. "16"
}

var (
	sdkVersionPattern = regexp.MustCompile(`^eforms-sdk-(\d+(?:\.\d+){1,2})`)
	flavourPattern    = regexp.MustCompile(`urn:fdc:anskaffelser\.no:\d{4}:eforms:([a-z0-9-]+)`)
)

// Sniff reads the identifying header of an eForms notice and checks that the
// document is shaped like one: a single UBL root, no text outside it, no DTD,
// each header field present at most once, and every size and nesting limit above
// respected. It streams the document with encoding/xml, which resolves no
// external entities.
//
// Errors carry a fixed description and, for malformed XML, a line number, never
// text from the document: the decoder's own messages quote element names, which
// are attacker-chosen and can be arbitrarily long or contain terminal escapes.
func Sniff(data []byte) (Header, error) {
	if len(data) > MaxNoticeBytes {
		return Header{}, fmt.Errorf("%w: %d bytes, limit %d", ErrTooLarge, len(data), MaxNoticeBytes)
	}
	data = bytes.TrimPrefix(data, utf8BOM)
	if longestRun(data, '<') > maxRunBytes {
		return Header{}, reject("a tag or text run is implausibly long")
	}

	decoder := xml.NewDecoder(bytes.NewReader(data))
	decoder.Strict = true

	var s sniffer
	for {
		token, err := decoder.Token()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return Header{}, malformed(err)
		}
		if err := s.accept(token); err != nil {
			return Header{}, err
		}
	}
	return s.finish()
}

// field names a header field, so that "seen twice" can be tracked.
type field uint8

const (
	fieldNone field = iota
	fieldCustomizationID
	fieldNoticeID
	fieldVersionID
	fieldProcedureID
	fieldNoticeTypeCode
	fieldSubTypeCode
)

// sniffer is the state of one Sniff call.
type sniffer struct {
	h          Header
	path       []xml.Name
	rootClosed bool
	elements   int
	nsDecls    int
	seen       uint16 // bit f is set once header field f has been stored
	collecting field  // the header field whose text is being read, if any
	text       []byte
}

func (s *sniffer) accept(token xml.Token) error {
	switch t := token.(type) {
	case xml.StartElement:
		return s.start(t)
	case xml.EndElement:
		return s.end()
	case xml.CharData:
		return s.chars(t)
	case xml.Directive:
		// eForms notices have no DTD. Refusing one outright is simpler than
		// reasoning about what a hostile internal subset might do.
		return reject("a document type declaration is not allowed")
	}
	return nil // comments and processing instructions carry nothing
}

func (s *sniffer) start(t xml.StartElement) error {
	if s.rootClosed {
		return reject("more than one top-level element")
	}
	if s.collecting != fieldNone {
		return reject("a header element has child elements")
	}
	s.path = append(s.path, t.Name)
	if len(s.path) > maxDepth {
		return reject("nesting is deeper than any notice")
	}
	if s.elements++; s.elements > maxElements {
		return reject("more elements than any notice has")
	}
	if err := s.countAttributes(t); err != nil {
		return err
	}
	if len(s.path) == 1 {
		if !strings.HasPrefix(t.Name.Space, nsUBLPrefix) {
			return reject("the root element is not a UBL document")
		}
		s.h.Root = t.Name.Local
		return nil
	}
	s.collecting = headerField(s.path)
	s.text = s.text[:0]
	return nil
}

func (s *sniffer) countAttributes(t xml.StartElement) error {
	if len(t.Attr) > maxAttributes {
		return reject("an element has more attributes than any notice")
	}
	for _, a := range t.Attr {
		if a.Name.Space == "xmlns" || (a.Name.Space == "" && a.Name.Local == "xmlns") {
			s.nsDecls++
		}
	}
	if s.nsDecls > maxNamespaceDecls {
		return reject("more namespace declarations than any notice has")
	}
	return nil
}

func (s *sniffer) end() error {
	if s.collecting != fieldNone {
		if err := s.store(s.collecting, strings.TrimSpace(string(s.text))); err != nil {
			return err
		}
		s.collecting = fieldNone
	}
	s.path = s.path[:len(s.path)-1]
	s.rootClosed = len(s.path) == 0
	return nil
}

func (s *sniffer) chars(t xml.CharData) error {
	switch {
	case len(s.path) == 0:
		if len(bytes.Trim(t, " \t\r\n")) > 0 {
			return reject("text outside the root element")
		}
	case s.collecting != fieldNone:
		if len(s.text)+len(t) > maxHeaderValueBytes {
			return reject("a header value is implausibly long")
		}
		s.text = append(s.text, t...)
	}
	return nil
}

// store records a header field. A field that appears twice is refused: which of
// the two would be "the" notice identifier is exactly what an attacker, or a
// buggy producer, could use to make two readers disagree.
func (s *sniffer) store(f field, value string) error {
	if s.seen&(1<<f) != 0 {
		return reject("a header field appears twice")
	}
	s.seen |= 1 << f
	switch f {
	case fieldCustomizationID:
		s.h.CustomizationID = value
	case fieldNoticeID:
		s.h.NoticeID = value
	case fieldVersionID:
		s.h.VersionID = value
	case fieldProcedureID:
		s.h.ProcedureID = value
	case fieldNoticeTypeCode:
		s.h.NoticeTypeCode = value
	case fieldSubTypeCode:
		s.h.SubTypeCode = value
	}
	return nil
}

func (s *sniffer) finish() (Header, error) {
	if s.h.Root == "" {
		return Header{}, reject("no root element")
	}
	if s.h.CustomizationID == "" {
		return Header{}, reject("no CustomizationID")
	}
	s.h.SDKVersion, s.h.Flavour = parseCustomization(s.h.CustomizationID)
	if s.h.SDKVersion == "" {
		return Header{}, reject("the CustomizationID names no eForms SDK version")
	}
	return s.h, nil
}

// headerField says which header field, if any, the element at path is. Only
// direct children of the root identify the notice: nested ID and VersionID
// elements belong to lots, projects and parties.
func headerField(path []xml.Name) field {
	leaf := path[len(path)-1]
	if leaf.Space != nsCBC {
		return fieldNone
	}
	switch len(path) {
	case 2:
		switch leaf.Local {
		case "CustomizationID":
			return fieldCustomizationID
		case "ID":
			return fieldNoticeID
		case "VersionID":
			return fieldVersionID
		case "ContractFolderID":
			return fieldProcedureID
		case "NoticeTypeCode":
			return fieldNoticeTypeCode
		}
	case len(subTypeAncestors) + 2:
		if leaf.Local == "SubTypeCode" && slices.Equal(path[1:len(path)-1], subTypeAncestors[:]) {
			return fieldSubTypeCode
		}
	}
	return fieldNone
}

// longestRun is the length of the longest stretch of data without the byte b.
func longestRun(data []byte, b byte) int {
	longest, start := 0, 0
	for {
		i := bytes.IndexByte(data[start:], b)
		if i < 0 {
			return max(longest, len(data)-start)
		}
		longest = max(longest, i)
		start += i + 1
	}
}

func reject(reason string) error { return fmt.Errorf("%w: %s", ErrNotEForms, reason) }

// malformed describes a decoding error without quoting it.
func malformed(err error) error {
	var syntax *xml.SyntaxError
	if errors.As(err, &syntax) {
		return reject(fmt.Sprintf("not well-formed XML near line %d", syntax.Line))
	}
	return reject("not well-formed XML, or in an unsupported encoding")
}

func parseCustomization(customization string) (sdk string, flavour Flavour) {
	if m := sdkVersionPattern.FindStringSubmatch(customization); m != nil {
		sdk = m[1]
	}
	if m := flavourPattern.FindStringSubmatch(customization); m != nil {
		flavour = Flavour(m[1])
	}
	return sdk, flavour
}
