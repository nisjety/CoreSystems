package transform

import (
	"strings"
	"testing"
)

func TestChunkMarkdown_Empty(t *testing.T) {
	chunks := ChunkMarkdown("", ChunkOptions{})
	if len(chunks) != 0 {
		t.Errorf("expected 0 chunks for empty text, got %d", len(chunks))
	}
}

func TestChunkMarkdown_SingleSmallSection(t *testing.T) {
	text := "Hello world, this is a test paragraph."
	chunks := ChunkMarkdown(text, ChunkOptions{MaxTokens: 512})
	if len(chunks) != 1 {
		t.Errorf("expected 1 chunk, got %d", len(chunks))
	}
	if chunks[0].Content != text {
		t.Errorf("expected content to match input")
	}
	if chunks[0].Index != 0 {
		t.Errorf("expected index 0, got %d", chunks[0].Index)
	}
}

func TestChunkMarkdown_HeadingSplit(t *testing.T) {
	text := `# Introduction

This is the intro paragraph with enough text.

## Methods

This is the methods section with more content.

## Results

These are the results.`

	chunks := ChunkMarkdown(text, ChunkOptions{MaxTokens: 20})
	if len(chunks) < 2 {
		t.Errorf("expected at least 2 chunks for multi-heading doc, got %d", len(chunks))
	}

	// First chunk should contain Introduction
	if !strings.Contains(chunks[0].Content, "intro") {
		t.Errorf("first chunk should contain intro content, got: %s", chunks[0].Content)
	}
}

func TestChunkMarkdown_LargeSection(t *testing.T) {
	// Build a section with many sentences that exceeds MaxTokens.
	var builder strings.Builder
	for i := 0; i < 50; i++ {
		builder.WriteString("This is sentence number one hundred. ")
	}
	text := builder.String()

	chunks := ChunkMarkdown(text, ChunkOptions{MaxTokens: 100, Overlap: 10})
	if len(chunks) < 2 {
		t.Errorf("expected multiple chunks for large text, got %d", len(chunks))
	}

	// Verify indices are sequential.
	for i, c := range chunks {
		if c.Index != i {
			t.Errorf("chunk %d has wrong index %d", i, c.Index)
		}
	}

	// Verify token estimates are reasonable.
	for _, c := range chunks {
		if c.Tokens <= 0 {
			t.Errorf("chunk %d has non-positive token count %d", c.Index, c.Tokens)
		}
	}
}

func TestChunkMarkdown_Overlap(t *testing.T) {
	// Build text with clear sentence boundaries and enough tokens.
	text := "First sentence here. Second sentence here. Third sentence here. Fourth sentence here. Fifth sentence here."
	chunks := ChunkMarkdown(text, ChunkOptions{MaxTokens: 10, Overlap: 2})
	if len(chunks) < 2 {
		t.Errorf("expected multiple chunks, got %d: %+v", len(chunks), chunks)
	}
}

func TestApproxTokens(t *testing.T) {
	tests := []struct {
		input    string
		expected int
	}{
		{"", 0},
		{"Hi", 1},
		{"Hello World", 2},
		{"This is a longer sentence with several words in it.", 12},
	}

	for _, tt := range tests {
		got := approxTokens(tt.input)
		if got != tt.expected {
			t.Errorf("approxTokens(%q) = %d, want %d", tt.input, got, tt.expected)
		}
	}
}
