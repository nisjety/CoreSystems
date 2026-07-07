package extract

import (
	"strings"
	"testing"
)

func TestClassify(t *testing.T) {
	cases := []struct {
		name string
		mime string
		file string
		want Kind
	}{
		{"pdf by mime", "application/pdf", "x", KindPDF},
		{"pdf by ext", "", "report.PDF", KindPDF},
		{"text plain", "text/plain; charset=utf-8", "a.txt", KindPlainText},
		{"text html mime", "text/html", "", KindPlainText},
		{"json mime", "application/json", "", KindPlainText},
		{"markdown ext", "", "notes.md", KindPlainText},
		{"csv ext", "application/octet-stream", "data.csv", KindPlainText},
		{"png unsupported", "image/png", "logo.png", KindUnsupported},
		{"office binary unsupported", "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "a.docx", KindUnsupported},
		{"empty unsupported", "", "noext", KindUnsupported},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := Classify(tc.mime, tc.file); got != tc.want {
				t.Fatalf("Classify(%q,%q)=%v want %v", tc.mime, tc.file, got, tc.want)
			}
		})
	}
}

func TestExtract_PlainText(t *testing.T) {
	text, supported, err := Extract("text/plain", "a.txt", []byte("hello world"))
	if err != nil || !supported {
		t.Fatalf("supported=%v err=%v", supported, err)
	}
	if text != "hello world" {
		t.Fatalf("got %q", text)
	}
}

func TestExtract_PlainText_InvalidUTF8Sanitized(t *testing.T) {
	// Data Plane rejects non-UTF-8 content; the extractor must sanitize.
	raw := []byte{'o', 'k', 0xff, 0xfe, '!'}
	text, supported, err := Extract("text/plain", "a.txt", raw)
	if err != nil || !supported {
		t.Fatalf("supported=%v err=%v", supported, err)
	}
	if !strings.HasPrefix(text, "ok") || !strings.HasSuffix(text, "!") {
		t.Fatalf("unexpected sanitized text %q", text)
	}
	if strings.ContainsRune(text, 0xfffd) {
		t.Fatalf("invalid bytes should be dropped, not replaced: %q", text)
	}
}

func TestExtract_UnsupportedSkipped(t *testing.T) {
	text, supported, err := Extract("image/png", "logo.png", []byte{0x89, 'P', 'N', 'G'})
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}
	if supported {
		t.Fatal("image/png must be unsupported")
	}
	if text != "" {
		t.Fatalf("expected empty text, got %q", text)
	}
}

func TestExtract_PDFGarbageFailsGracefully(t *testing.T) {
	// A PDF-typed payload that is not a valid PDF must be reported as supported
	// (so the caller knows we tried) with a non-nil error, and must NOT panic.
	text, supported, err := Extract("application/pdf", "broken.pdf", []byte("%PDF-1.7 not really a pdf"))
	if !supported {
		t.Fatal("application/pdf must be supported")
	}
	if err == nil {
		t.Fatal("expected an extraction error for garbage PDF")
	}
	if text != "" {
		t.Fatalf("expected empty text on failure, got %q", text)
	}
}

func TestExtract_EmptyPDFPayload(t *testing.T) {
	_, supported, err := Extract("application/pdf", "empty.pdf", nil)
	if !supported {
		t.Fatal("application/pdf must be supported")
	}
	if err == nil {
		t.Fatal("expected error for empty pdf payload")
	}
}
