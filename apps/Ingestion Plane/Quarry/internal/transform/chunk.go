package transform

import (
	"strings"
	"unicode/utf8"
)

// Chunk represents a segment of a document with metadata for retrieval.
type Chunk struct {
	Index   int    `json:"index"`
	Content string `json:"content"`
	Tokens  int    `json:"tokens"` // approximate token count
}

// ChunkOptions controls how text is split into chunks.
type ChunkOptions struct {
	// MaxTokens is the target token count per chunk (default 512).
	MaxTokens int `json:"maxTokens,omitempty"`
	// Overlap is the number of tokens to overlap between chunks (default 50).
	Overlap int `json:"overlap,omitempty"`
}

// ChunkMarkdown splits markdown text into semantically meaningful chunks,
// respecting heading boundaries, paragraph breaks, and the configured
// maximum token size. Uses a simple word-based tokenization approximation.
func ChunkMarkdown(text string, opts ChunkOptions) []Chunk {
	if opts.MaxTokens <= 0 {
		opts.MaxTokens = 512
	}
	if opts.Overlap < 0 {
		opts.Overlap = 0
	}
	if opts.Overlap >= opts.MaxTokens {
		opts.Overlap = opts.MaxTokens / 4
	}

	text = strings.TrimSpace(text)
	if text == "" {
		return nil
	}

	// Split on heading boundaries and double newlines (paragraph breaks).
	sections := splitOnHeadings(text)

	var chunks []Chunk
	var buffer strings.Builder
	bufferTokens := 0

	flush := func() {
		content := strings.TrimSpace(buffer.String())
		if content == "" {
			return
		}
		chunks = append(chunks, Chunk{
			Index:   len(chunks),
			Content: content,
			Tokens:  approxTokens(content),
		})

		// Compute overlap: keep the last N tokens.
		if opts.Overlap > 0 {
			words := strings.Fields(content)
			overlapStart := len(words) - opts.Overlap
			if overlapStart < 0 {
				overlapStart = 0
			}
			overlap := strings.Join(words[overlapStart:], " ")
			buffer.Reset()
			buffer.WriteString(overlap)
			bufferTokens = opts.Overlap
		} else {
			buffer.Reset()
			bufferTokens = 0
		}
	}

	for _, section := range sections {
		section = strings.TrimSpace(section)
		if section == "" {
			continue
		}

		sectionTokens := approxTokens(section)

		// If adding this section would exceed the limit, flush first.
		if bufferTokens > 0 && bufferTokens+sectionTokens > opts.MaxTokens {
			flush()
		}

		// If a single section is larger than MaxTokens, split it by sentences.
		if sectionTokens > opts.MaxTokens {
			sentences := splitOnSentences(section)
			for _, sent := range sentences {
				sent = strings.TrimSpace(sent)
				if sent == "" {
					continue
				}
				sentTokens := approxTokens(sent)
				if bufferTokens+sentTokens > opts.MaxTokens && bufferTokens > 0 {
					flush()
				}
				if buffer.Len() > 0 {
					buffer.WriteString("\n")
				}
				buffer.WriteString(sent)
				bufferTokens += sentTokens
			}
		} else {
			if buffer.Len() > 0 {
				buffer.WriteString("\n\n")
			}
			buffer.WriteString(section)
			bufferTokens += sectionTokens
		}
	}

	// Flush remaining content.
	if buffer.Len() > 0 {
		content := strings.TrimSpace(buffer.String())
		if content != "" {
			chunks = append(chunks, Chunk{
				Index:   len(chunks),
				Content: content,
				Tokens:  approxTokens(content),
			})
		}
	}

	return chunks
}

// splitOnHeadings splits markdown text at heading boundaries (# ... ##).
func splitOnHeadings(text string) []string {
	lines := strings.Split(text, "\n")
	var sections []string
	var current strings.Builder

	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "#") && len(trimmed) > 1 && (trimmed[1] == ' ' || trimmed[1] == '#') {
			// Heading boundary: flush current section.
			if current.Len() > 0 {
				sections = append(sections, current.String())
				current.Reset()
			}
		} else if trimmed == "" && current.Len() > 0 {
			// Double newline: paragraph boundary.
			content := strings.TrimSpace(current.String())
			if content != "" {
				sections = append(sections, content)
				current.Reset()
			}
			continue
		}
		if current.Len() > 0 {
			current.WriteString("\n")
		}
		current.WriteString(line)
	}

	if current.Len() > 0 {
		sections = append(sections, current.String())
	}
	return sections
}

// splitOnSentences does a rough split on sentence boundaries.
func splitOnSentences(text string) []string {
	var sentences []string
	var current strings.Builder

	runes := []rune(text)
	for i := 0; i < len(runes); i++ {
		current.WriteRune(runes[i])

		// Sentence boundary heuristic: period/question/exclamation followed by space or EOL.
		if (runes[i] == '.' || runes[i] == '?' || runes[i] == '!') &&
			(i+1 >= len(runes) || runes[i+1] == ' ' || runes[i+1] == '\n') {
			sentences = append(sentences, current.String())
			current.Reset()
		}
	}
	if current.Len() > 0 {
		sentences = append(sentences, current.String())
	}
	return sentences
}

// approxTokens estimates the number of tokens using a ~4 chars per token
// heuristic (common for English text with GPT-class tokenizers).
func approxTokens(text string) int {
	charCount := utf8.RuneCountInString(text)
	tokens := charCount / 4
	if tokens == 0 && charCount > 0 {
		tokens = 1
	}
	return tokens
}
