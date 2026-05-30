package transform

import (
	"archive/zip"
	"bytes"
	"context"
	"fmt"
	"io"
	"strings"
	"testing"
)

func TestExtractXLSXMarkdown(t *testing.T) {
	t.Parallel()

	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)

	files := map[string]string{
		"xl/workbook.xml": `<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheets>
    <sheet name="Inventory" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>`,
		"xl/sharedStrings.xml": `<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4" uniqueCount="4">
  <si><t>Name</t></si>
  <si><t>Price</t></si>
  <si><t>Widget</t></si>
  <si><t>12.50</t></si>
</sst>`,
		"xl/worksheets/sheet1.xml": `<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="s"><v>0</v></c>
      <c r="B1" t="s"><v>1</v></c>
    </row>
    <row r="2">
      <c r="A2" t="s"><v>2</v></c>
      <c r="B2" t="s"><v>3</v></c>
    </row>
  </sheetData>
</worksheet>`,
	}

	for name, content := range files {
		entry, err := writer.Create(name)
		if err != nil {
			t.Fatalf("Create(%q) error = %v", name, err)
		}
		if _, err := entry.Write([]byte(content)); err != nil {
			t.Fatalf("Write(%q) error = %v", name, err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("Close() error = %v", err)
	}

	markdown, err := extractXLSXMarkdown(buffer.Bytes())
	if err != nil {
		t.Fatalf("extractXLSXMarkdown() error = %v", err)
	}

	for _, want := range []string{"## Inventory", "| Name | Price |", "| Widget | 12.50 |"} {
		if !strings.Contains(markdown, want) {
			t.Fatalf("markdown = %q, want substring %q", markdown, want)
		}
	}
}

func TestNormalizeParserMode(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"":      "auto",
		"fast":  "fast",
		"ocr":   "ocr",
		"other": "auto",
	}

	for input, want := range cases {
		if got := normalizeParserMode(input); got != want {
			t.Fatalf("normalizeParserMode(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestParsePDFDocumentUsesNativeTextInAutoMode(t *testing.T) {
	t.Parallel()

	ocrCalls := 0
	result, err := parsePDFDocument(
		context.Background(),
		"https://example.com/sample.pdf",
		[]byte("fake-pdf"),
		"auto",
		func(_ io.ReaderAt, _ int64) (string, int, error) {
			return "native text", 2, nil
		},
		func(_ context.Context, _ string, _ []byte) (string, int, error) {
			ocrCalls++
			return "ocr text", 2, nil
		},
	)
	if err != nil {
		t.Fatalf("parsePDFDocument() error = %v", err)
	}
	if result.Text != "native text" {
		t.Fatalf("result.Text = %q, want native text", result.Text)
	}
	if got := result.Metadata["parserEngine"]; got != "native_text" {
		t.Fatalf("parserEngine = %q, want native_text", got)
	}
	if ocrCalls != 0 {
		t.Fatalf("ocrCalls = %d, want 0", ocrCalls)
	}
}

func TestParsePDFDocumentFallsBackToOCRInAutoMode(t *testing.T) {
	t.Parallel()

	result, err := parsePDFDocument(
		context.Background(),
		"https://example.com/scanned.pdf",
		[]byte("fake-pdf"),
		"auto",
		func(_ io.ReaderAt, _ int64) (string, int, error) {
			return "", 1, nil
		},
		func(_ context.Context, fileName string, _ []byte) (string, int, error) {
			if fileName != "scanned.pdf" {
				t.Fatalf("fileName = %q, want scanned.pdf", fileName)
			}
			return "ocr text", 3, nil
		},
	)
	if err != nil {
		t.Fatalf("parsePDFDocument() error = %v", err)
	}
	if result.Text != "ocr text" {
		t.Fatalf("result.Text = %q, want ocr text", result.Text)
	}
	if got := result.Metadata["parserEngine"]; got != "ai_core_read" {
		t.Fatalf("parserEngine = %q, want ai_core_read", got)
	}
	if got := result.Metadata["ocrAvailable"]; got != "true" {
		t.Fatalf("ocrAvailable = %q, want true", got)
	}
	if got := result.Metadata["pageCount"]; got != "3" {
		t.Fatalf("pageCount = %q, want 3", got)
	}
}

func TestParsePDFDocumentRequiresOCRWhenExplicit(t *testing.T) {
	t.Parallel()

	_, err := parsePDFDocument(
		context.Background(),
		"https://example.com/scanned.pdf",
		[]byte("fake-pdf"),
		"ocr",
		func(_ io.ReaderAt, _ int64) (string, int, error) {
			return "", 1, nil
		},
		func(_ context.Context, _ string, _ []byte) (string, int, error) {
			return "", 0, fmt.Errorf("ocr unavailable")
		},
	)
	if err == nil || !strings.Contains(err.Error(), "ocr parse pdf failed") {
		t.Fatalf("err = %v, want ocr parse pdf failure", err)
	}
}
