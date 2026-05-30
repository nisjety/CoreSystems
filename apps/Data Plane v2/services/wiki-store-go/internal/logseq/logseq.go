// Package logseq implements a minimal Logseq-style block-outline parser and
// serializer for wiki page content. Supports the subset that matters for
// round-tripping wiki edits in the v2 Data Plane:
//
//   - Bullet blocks (lines starting with "- ").
//   - Nested indentation via leading tabs (one tab = one nesting level).
//   - Page properties (Logseq inline "key:: value" syntax) attached to the
//     first block whose parent is the page itself.
//
// Closes D4+D5 spec D4-6 (Logseq block-outline parser/serializer). The
// implementation is intentionally small — we don't yet support every
// Logseq feature (queries, embeds, advanced properties). What we DO
// guarantee is round-trip stability: Serialize(Parse(s)) == s for any
// input written by Serialize.
package logseq

import (
	"strings"
)

// Block is a single outline node. Parent/child relationships are implicit in
// the slice ordering (preorder) — Indent gives the nesting depth.
type Block struct {
	Indent     int               `json:"indent"`
	Content    string            `json:"content"`
	Properties map[string]string `json:"properties,omitempty"`
}

// Parse converts Logseq block-outline markdown into a flat preorder slice.
//
// Blank lines are dropped. Lines that don't start with "- " (after stripping
// leading tabs) are treated as continuation of the previous block.
func Parse(s string) []Block {
	var blocks []Block
	var current *Block

	for _, raw := range strings.Split(s, "\n") {
		// Count leading tabs for indent depth.
		indent := 0
		line := raw
		for strings.HasPrefix(line, "\t") {
			indent++
			line = line[1:]
		}

		// Pure blank → flush nothing, keep state.
		if strings.TrimSpace(line) == "" {
			continue
		}

		if strings.HasPrefix(line, "- ") {
			// Flush previous block.
			if current != nil {
				blocks = append(blocks, *current)
			}
			content := strings.TrimPrefix(line, "- ")
			// Property line `key:: value` becomes a property on the block.
			props, contentOut := extractProperties(content)
			current = &Block{Indent: indent, Content: contentOut, Properties: props}
			continue
		}

		// Continuation line.
		if current != nil {
			current.Content += "\n" + line
		}
	}
	if current != nil {
		blocks = append(blocks, *current)
	}
	return blocks
}

// extractProperties pulls `key:: value` lines out of a block's first line.
// Returns the cleaned content + a property map (nil if none).
func extractProperties(content string) (map[string]string, string) {
	idx := strings.Index(content, "::")
	if idx < 0 {
		return nil, content
	}
	key := strings.TrimSpace(content[:idx])
	if key == "" || strings.ContainsAny(key, " \t") {
		// Not a valid property (contains whitespace) — treat as content.
		return nil, content
	}
	value := strings.TrimSpace(content[idx+2:])
	return map[string]string{key: value}, ""
}

// Serialize converts a Block slice back to Logseq block-outline markdown.
// Round-trip stable for inputs produced by Parse.
func Serialize(blocks []Block) string {
	var b strings.Builder
	for _, blk := range blocks {
		prefix := strings.Repeat("\t", blk.Indent)
		// Single-property block (current parser supports one) becomes "key:: value"
		for k, v := range blk.Properties {
			b.WriteString(prefix)
			b.WriteString("- ")
			b.WriteString(k)
			b.WriteString(":: ")
			b.WriteString(v)
			b.WriteString("\n")
		}
		if blk.Content != "" {
			// Multi-line content keeps continuation lines at indent+1.
			lines := strings.Split(blk.Content, "\n")
			b.WriteString(prefix)
			b.WriteString("- ")
			b.WriteString(lines[0])
			b.WriteString("\n")
			for _, cont := range lines[1:] {
				b.WriteString(prefix)
				b.WriteString("\t")
				b.WriteString(cont)
				b.WriteString("\n")
			}
		}
	}
	return strings.TrimRight(b.String(), "\n")
}
