package mailtask

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// fakeExtractor is a deterministic AttachmentExtractor: it "extracts"
// anything except filenames containing "unreadable" (simulating an extractor
// that understood the type but found no text) and "erroring" (simulating a
// transport failure).
type fakeExtractor struct {
	calls []string
}

func (f *fakeExtractor) Extract(_ context.Context, filename, _ string, content []byte) (string, bool, error) {
	f.calls = append(f.calls, filename)
	switch {
	case strings.Contains(filename, "erroring"):
		return "", false, errors.New("boom")
	case strings.Contains(filename, "unreadable"):
		return "", true, nil
	default:
		return "extracted text for " + filename, true, nil
	}
}

func TestExtractAttachmentsAllowListedTypeUnderLimitIsExtracted(t *testing.T) {
	extractor := &fakeExtractor{}
	results := ExtractAttachments(context.Background(), extractor, []InboundAttachment{
		{Filename: "report.pdf", ContentType: "application/pdf", Content: []byte("small pdf bytes")},
	})
	if len(results) != 1 {
		t.Fatalf("results = %d, want 1", len(results))
	}
	if !results[0].Extracted || results[0].Text == "" || results[0].Filename != "report.pdf" {
		t.Fatalf("result = %+v, want extracted with text", results[0])
	}
}

func TestExtractAttachmentsDisallowedTypeIsNotExtractedButFilenameIsKept(t *testing.T) {
	extractor := &fakeExtractor{}
	results := ExtractAttachments(context.Background(), extractor, []InboundAttachment{
		{Filename: "sheet.xlsx", ContentType: "application/vnd.ms-excel", Content: []byte("some bytes")},
	})
	if len(results) != 1 {
		t.Fatalf("results = %d, want 1", len(results))
	}
	if results[0].Extracted || results[0].Filename != "sheet.xlsx" || results[0].Text != "" {
		t.Fatalf("result = %+v, want extracted=false with filename kept", results[0])
	}
	if len(extractor.calls) != 0 {
		t.Fatal("extractor must never be called for a disallowed extension")
	}
}

func TestExtractAttachmentsOversizedAttachmentIsNotExtractedAndNeverErrors(t *testing.T) {
	oversized := make([]byte, maxAttachmentBytes+1)
	extractor := &fakeExtractor{}
	results := ExtractAttachments(context.Background(), extractor, []InboundAttachment{
		{Filename: "big.txt", ContentType: "text/plain", Content: oversized},
	})
	if len(results) != 1 || results[0].Extracted {
		t.Fatalf("result = %+v, want extracted=false for an oversized attachment", results[0])
	}
	if len(extractor.calls) != 0 {
		t.Fatal("extractor must never be called for an oversized attachment")
	}
}

func TestExtractAttachmentsEmptyAttachmentIsNotExtracted(t *testing.T) {
	results := ExtractAttachments(context.Background(), &fakeExtractor{}, []InboundAttachment{
		{Filename: "empty.txt", ContentType: "text/plain", Content: nil},
	})
	if len(results) != 1 || results[0].Extracted {
		t.Fatalf("result = %+v, want extracted=false for an empty attachment", results[0])
	}
}

func TestExtractAttachmentsExtractorErrorDoesNotFailTheWholeMessage(t *testing.T) {
	results := ExtractAttachments(context.Background(), &fakeExtractor{}, []InboundAttachment{
		{Filename: "erroring.txt", ContentType: "text/plain", Content: []byte("bytes")},
		{Filename: "fine.txt", ContentType: "text/plain", Content: []byte("bytes")},
	})
	if len(results) != 2 {
		t.Fatalf("results = %d, want 2 (one failure must not drop the other attachment)", len(results))
	}
	if results[0].Extracted {
		t.Fatalf("erroring attachment result = %+v, want extracted=false", results[0])
	}
	if !results[1].Extracted {
		t.Fatalf("fine attachment result = %+v, want extracted=true", results[1])
	}
}

func TestExtractAttachmentsUnreadableResultIsNotExtracted(t *testing.T) {
	results := ExtractAttachments(context.Background(), &fakeExtractor{}, []InboundAttachment{
		{Filename: "unreadable.pdf", ContentType: "application/pdf", Content: []byte("scanned image bytes")},
	})
	if len(results) != 1 || results[0].Extracted {
		t.Fatalf("result = %+v, want extracted=false when the extractor found no text", results[0])
	}
}

func TestExtractAttachmentsNilExtractorDegradesEveryAttachmentWithoutPanicking(t *testing.T) {
	results := ExtractAttachments(context.Background(), nil, []InboundAttachment{
		{Filename: "report.pdf", ContentType: "application/pdf", Content: []byte("bytes")},
	})
	if len(results) != 1 || results[0].Extracted {
		t.Fatalf("result = %+v, want extracted=false with a nil extractor", results[0])
	}
}

func TestExtractAttachmentsCaseInsensitiveExtension(t *testing.T) {
	extractor := &fakeExtractor{}
	results := ExtractAttachments(context.Background(), extractor, []InboundAttachment{
		{Filename: "REPORT.PDF", ContentType: "application/pdf", Content: []byte("bytes")},
	})
	if len(results) != 1 || !results[0].Extracted {
		t.Fatalf("result = %+v, want extracted=true for an uppercase extension", results[0])
	}
}
