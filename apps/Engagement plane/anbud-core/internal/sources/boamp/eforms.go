package boamp

import (
	"bytes"
	"encoding/json"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"regexp"
	"slices"
	"strconv"
	"strings"
)

// BOAMP carries an eForms notice as JSON in the record's donnees field: the notice's XML converted to
// JSON, with the element prefixes kept ("cbc:ID") and attributes as "@name" members. The eForms parser
// reads XML and matches each element by namespace, so the conversion writes the XML back with the
// namespaces the prefixes stand for (internal/eforms, sniff.go and tree.go). The output is the notice
// the parser reads; a document the conversion cannot write is refused rather than guessed.

// ubl is the namespace stem of the UBL schemas eForms is written in.
const ubl = "urn:oasis:names:specification:ubl:schema:xsd:"

// nsByPrefix maps the prefixes BOAMP's JSON uses to the namespaces the eForms parser reads. A prefix
// outside this table is refused.
var nsByPrefix = map[string]string{
	"cac":   ubl + "CommonAggregateComponents-2",
	"cbc":   ubl + "CommonBasicComponents-2",
	"efac":  "http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1",
	"efbc":  "http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1",
	"efext": "http://data.europa.eu/p27/eforms-ubl-extensions/1",
	"ext":   ubl + "CommonExtensionComponents-2",
}

// noticeRoots are the UBL roots of the eForms notices BOAMP carries.
var noticeRoots = []string{"ContractNotice", "ContractAwardNotice", "PriorInformationNotice"}

// maxDepth bounds the nesting of a record. The eForms parser refuses deeper notices anyway.
const maxDepth = 64

var (
	elementPattern   = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_.-]*(:[A-Za-z_][A-Za-z0-9_.-]*)?$`)
	attributePattern = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_.-]*$`)
)

// errNotEForms means the record is not an eForms notice: a French national form, which BOAMP also
// publishes and the parser does not read.
var errNotEForms = errors.New("boamp: the record is not an eForms notice")

// object is a JSON object with its members in the order the document gives them. A Go map would lose
// that order, and the order is part of the notice's bytes and so of its hash.
type object []member

type member struct {
	key   string
	value any
}

// noticeBody is the body the raw store keeps for a record: the eForms XML of an eForms notice, and
// otherwise the record's own JSON, which the eForms parser refuses as it refuses any other form. A
// conversion that fails on an eForms record also keeps the JSON, so no evidence is lost; the test over
// a day of records is what keeps that from happening silently.
func noticeBody(donnees string) ([]byte, string) {
	body, err := eFormsXML([]byte(donnees))
	if err != nil {
		return []byte(donnees), "application/json"
	}
	return body, "application/xml"
}

// eFormsXML writes the eForms notice a record carries as XML. It returns errNotEForms for another
// form, and another error for an eForms record it cannot write.
func eFormsXML(donnees []byte) ([]byte, error) {
	top, err := decodeRecord(donnees)
	if err != nil {
		return nil, err
	}
	if len(top) != 1 || top[0].key != "EFORMS" {
		return nil, errNotEForms
	}
	forms, ok := top[0].value.(object)
	if !ok || len(forms) != 1 {
		return nil, errors.New("boamp: the eForms record does not hold exactly one notice")
	}
	root := forms[0].key
	if !slices.Contains(noticeRoots, root) {
		return nil, errNotEForms
	}
	notice, ok := forms[0].value.(object)
	if !ok {
		return nil, errors.New("boamp: the eForms notice is not an object")
	}

	var b bytes.Buffer
	b.WriteString(`<?xml version="1.0" encoding="UTF-8"?>` + "\n")
	decl := fmt.Sprintf(` xmlns="%s%s-2"`, ubl, root)
	for _, prefix := range []string{"cac", "cbc", "efac", "efbc", "efext", "ext"} {
		decl += fmt.Sprintf(` xmlns:%s="%s"`, prefix, nsByPrefix[prefix])
	}
	if err := writeElement(&b, root, notice, decl); err != nil {
		return nil, err
	}
	b.WriteByte('\n')
	return b.Bytes(), nil
}

// writeElement writes one element. An object's "@" members are its attributes, its "#text" member is
// its text and its other members are its child elements, in document order. A scalar is text. decl
// holds the namespace declarations of the start tag, for the root only.
func writeElement(b *bytes.Buffer, name string, value any, decl string) error {
	if !elementPattern.MatchString(name) {
		return fmt.Errorf("boamp: %q is not an element name", name)
	}
	var attrs, children object
	var text any
	if obj, ok := value.(object); ok {
		for _, m := range obj {
			switch {
			case strings.HasPrefix(m.key, "@"):
				attrs = append(attrs, m)
			case m.key == "#text":
				text = m.value
			default:
				children = append(children, m)
			}
		}
	} else {
		text = value
	}

	fmt.Fprintf(b, "<%s%s", name, decl)
	for _, a := range attrs {
		attr := strings.TrimPrefix(a.key, "@")
		if !attributePattern.MatchString(attr) {
			return fmt.Errorf("boamp: %q is not an attribute the eForms parser reads", a.key)
		}
		s, present, err := scalarText(a.value)
		if err != nil {
			return fmt.Errorf("boamp: attribute %s: %w", attr, err)
		}
		if !present {
			continue
		}
		fmt.Fprintf(b, ` %s="`, attr)
		if err := xml.EscapeText(b, []byte(s)); err != nil {
			return err
		}
		b.WriteByte('"')
	}

	s, hasText, err := scalarText(text)
	if err != nil {
		return fmt.Errorf("boamp: element %s: %w", name, err)
	}
	if !hasText && len(children) == 0 {
		b.WriteString("/>")
		return nil
	}
	b.WriteByte('>')
	if hasText {
		if err := xml.EscapeText(b, []byte(s)); err != nil {
			return err
		}
	}
	for _, c := range children {
		prefix, _, prefixed := strings.Cut(c.key, ":")
		if !prefixed || nsByPrefix[prefix] == "" {
			return fmt.Errorf("boamp: element %q has no namespace the eForms parser reads", c.key)
		}
		if items, repeated := c.value.([]any); repeated {
			for _, item := range items {
				if err := writeElement(b, c.key, item, ""); err != nil {
					return err
				}
			}
			continue
		}
		if err := writeElement(b, c.key, c.value, ""); err != nil {
			return err
		}
	}
	fmt.Fprintf(b, "</%s>", name)
	return nil
}

// scalarText is the text of a JSON scalar, and whether there is any: null has none. An object or an
// array is not text.
func scalarText(v any) (string, bool, error) {
	switch t := v.(type) {
	case nil:
		return "", false, nil
	case string:
		return t, true, nil
	case json.Number:
		return t.String(), true, nil
	case bool:
		return strconv.FormatBool(t), true, nil
	}
	return "", false, errors.New("a value that is not text")
}

// decodeRecord reads a record's donnees into ordered objects. It reads numbers as written.
func decodeRecord(donnees []byte) (object, error) {
	dec := json.NewDecoder(bytes.NewReader(donnees))
	dec.UseNumber()
	v, err := decodeValue(dec, 0)
	if err != nil {
		return nil, fmt.Errorf("boamp: the record is not JSON: %w", err)
	}
	if _, err := dec.Token(); err != io.EOF {
		return nil, errors.New("boamp: the record has data after its JSON value")
	}
	obj, ok := v.(object)
	if !ok {
		return nil, errors.New("boamp: the record is not a JSON object")
	}
	return obj, nil
}

// decodeValue reads one JSON value. An object keeps its members in document order; an array is a
// []any; a scalar is what the decoder gives, with numbers as json.Number.
func decodeValue(dec *json.Decoder, depth int) (any, error) {
	if depth > maxDepth {
		return nil, errors.New("the record nests too deeply")
	}
	tok, err := dec.Token()
	if err != nil {
		return nil, err
	}
	delim, isDelim := tok.(json.Delim)
	if !isDelim {
		return tok, nil
	}
	switch delim {
	case '{':
		obj := object{}
		for dec.More() {
			keyTok, err := dec.Token()
			if err != nil {
				return nil, err
			}
			key, ok := keyTok.(string)
			if !ok {
				return nil, errors.New("an object key is not a string")
			}
			value, err := decodeValue(dec, depth+1)
			if err != nil {
				return nil, err
			}
			obj = append(obj, member{key: key, value: value})
		}
		if _, err := dec.Token(); err != nil {
			return nil, err
		}
		return obj, nil
	case '[':
		items := []any{}
		for dec.More() {
			item, err := decodeValue(dec, depth+1)
			if err != nil {
				return nil, err
			}
			items = append(items, item)
		}
		if _, err := dec.Token(); err != nil {
			return nil, err
		}
		return items, nil
	}
	return nil, errors.New("an unexpected delimiter")
}
