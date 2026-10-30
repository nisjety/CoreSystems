package eforms

// ExternalLinks returns the link of every cbc:URI that sits directly in a cac:ExternalReference of
// the notice, in document order. It reads the same checked document the parser reads. It is a
// measurement of what buyers link, not the parser's model of a notice, so it takes no view of which
// link is a tender document.
func ExternalLinks(data []byte) ([]string, error) {
	if _, err := Sniff(data); err != nil {
		return nil, err
	}
	root, err := buildTree(data)
	if err != nil {
		return nil, err
	}
	var out []string
	var visit func(n *node)
	visit = func(n *node) {
		for _, c := range n.children {
			if c.name == cbc("URI") && n.name == cac("ExternalReference") {
				out = append(out, c.textValue())
			}
			visit(c)
		}
	}
	visit(root)
	return out, nil
}
