package eforms

import (
	"bytes"
	"encoding/xml"
	"io"
	"sort"
	"strings"
)

// nsCAC is the namespace of the aggregate components, where a notice's projects, parties and
// periods are. nsCBC (basic components) is in sniff.go.
const nsCAC = nsUBLPrefix + "CommonAggregateComponents-2"

// nsEFBC is the namespace of the eForms extension's basic components, where codes and numbers of
// the result and of award-criterion parameters are.
const nsEFBC = "http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1"

// maxUnmapped bounds how many unread paths a notice reports, so one hostile notice cannot make the
// report large.
const maxUnmapped = 200

// cac, cbc, ext, efext, efac and efbc name an element of a namespace. Elements are matched by namespace
// and local name, never by prefix.
func cac(local string) xml.Name   { return xml.Name{Space: nsCAC, Local: local} }
func cbc(local string) xml.Name   { return xml.Name{Space: nsCBC, Local: local} }
func ext(local string) xml.Name   { return xml.Name{Space: nsEXT, Local: local} }
func efext(local string) xml.Name { return xml.Name{Space: nsEFEXT, Local: local} }
func efac(local string) xml.Name  { return xml.Name{Space: nsEFAC, Local: local} }
func efbc(local string) xml.Name  { return xml.Name{Space: nsEFBC, Local: local} }

// node is one element of a notice. buildTree reads a whole document into nodes. Sniff has bounded
// the size and the nesting first, so the tree is no larger than the limits allow.
type node struct {
	name     xml.Name
	attrs    map[string]string // by local name; the namespace of an attribute is not needed
	text     []byte
	children []*node
}

// textValue is the element's own text, trimmed. Text of its children is not included.
func (n *node) textValue() string { return strings.TrimSpace(string(n.text)) }

// attr is the value of an attribute by local name, or "".
func (n *node) attr(local string) string { return n.attrs[local] }

// buildTree reads the document into a tree. Sniff accepted it before, so a decoder error here is
// not expected; it is reported as ErrNotEForms rather than trusted.
func buildTree(data []byte) (*node, error) {
	data = bytes.TrimPrefix(data, utf8BOM)
	decoder := xml.NewDecoder(bytes.NewReader(data))
	decoder.Strict = true

	var root *node
	var open []*node
	for {
		token, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, ErrNotEForms
		}
		switch t := token.(type) {
		case xml.StartElement:
			n := &node{name: t.Name}
			for _, a := range t.Attr {
				if n.attrs == nil {
					n.attrs = make(map[string]string, len(t.Attr))
				}
				n.attrs[a.Name.Local] = a.Value
			}
			if len(open) == 0 {
				if root != nil {
					return nil, ErrNotEForms
				}
				root = n
			} else {
				parent := open[len(open)-1]
				parent.children = append(parent.children, n)
			}
			open = append(open, n)
		case xml.EndElement:
			if len(open) == 0 {
				return nil, ErrNotEForms
			}
			open = open[:len(open)-1]
		case xml.CharData:
			if len(open) > 0 {
				top := open[len(open)-1]
				top.text = append(top.text, t...)
			}
		}
	}
	if root == nil {
		return nil, ErrNotEForms
	}
	return root, nil
}

// reader walks a tree and remembers every element it has read, so that what it did not read can
// be reported by path.
type reader struct {
	visited map[*node]bool
}

func newReader(root *node) *reader {
	return &reader{visited: map[*node]bool{root: true}}
}

// walk follows the steps as child elements from n and returns the elements reached, in document
// order. Every element on the way is marked as read.
func (r *reader) walk(n *node, steps ...xml.Name) []*node {
	level := []*node{n}
	for _, step := range steps {
		var next []*node
		for _, p := range level {
			for _, c := range p.children {
				if c.name == step {
					r.visited[c] = true
					next = append(next, c)
				}
			}
		}
		level = next
	}
	return level
}

// consume marks n and everything under it as read: the value is taken whole.
func (r *reader) consume(n *node) {
	r.visited[n] = true
	for _, c := range n.children {
		r.consume(c)
	}
}

// countDescendants counts the elements called name anywhere under n, and reads them.
func (r *reader) countDescendants(n *node, name xml.Name) int {
	count := 0
	for _, c := range n.children {
		if c.name == name {
			count++
			r.consume(c)
			continue
		}
		count += r.countDescendants(c, name)
	}
	return count
}

func firstNode(nodes []*node) *node {
	if len(nodes) == 0 {
		return nil
	}
	return nodes[0]
}

func firstText(nodes []*node) string {
	if n := firstNode(nodes); n != nil {
		return n.textValue()
	}
	return ""
}

// unmapped lists the elements that no field map read, as paths of local names from the root.
// Each path appears once, in sorted order, and carries no value. An element that was not read is
// listed once, and what lies under it is not listed separately.
func unmapped(root *node, visited map[*node]bool) []string {
	seen := map[string]bool{}
	var out []string
	var visit func(n *node, path string)
	visit = func(n *node, path string) {
		for _, c := range n.children {
			p := path + "/" + c.name.Local
			if !visited[c] {
				if !seen[p] && len(out) < maxUnmapped {
					seen[p] = true
					out = append(out, p)
				}
				continue
			}
			visit(c, p)
		}
	}
	visit(root, root.name.Local)
	sort.Strings(out)
	return out
}
