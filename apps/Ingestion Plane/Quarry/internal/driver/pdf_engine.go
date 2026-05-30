package driver

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/quarry/internal/transform"
)

// PDFEngine is a specialised PageDriver that fetches PDF/DOCX/XLSX documents
// and returns their text content as HTML-like output. It is selected by the
// waterfall when the URL extension signals a document, so FetchFormats can
// process it through the same pipeline as regular HTML pages.
type PDFEngine struct {
	userAgent     string
	parserMode    string
	aiCoreBaseURL string // empty disables OCR; non-empty enables /api/v1/analyze fallback
	mu            sync.RWMutex
	last          *FetchResult
}

// NewPDFEngine creates a document-oriented driver.
// parserMode: "auto" (default), "native", or "ocr".
// aiCoreBaseURL: when non-empty, enables AI-powered OCR via Model Plane v2.
func NewPDFEngine(userAgent, parserMode, aiCoreBaseURL string) *PDFEngine {
	if parserMode == "" {
		parserMode = "auto"
	}
	return &PDFEngine{userAgent: userAgent, parserMode: parserMode, aiCoreBaseURL: aiCoreBaseURL}
}

func (d *PDFEngine) Name() string { return "pdf" }

func (d *PDFEngine) Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error) {
	// Only handle document URLs.
	if !isDocumentURL(targetURL) {
		return nil, fmt.Errorf("pdf engine: url is not a document (%s)", targetURL)
	}

	headers := map[string]string{}
	if opts != nil {
		for k, v := range opts.Headers {
			headers[k] = v
		}
	}

	media, err := transform.ParseMediaURLWithOCR(ctx, targetURL, headers, d.parserMode, d.aiCoreBaseURL)
	if err != nil {
		return nil, fmt.Errorf("pdf engine: parse failed: %w", err)
	}

	// Wrap the extracted text in minimal HTML so downstream markdown/HTML
	// transformers work without special-casing.
	htmlContent := fmt.Sprintf(`<!DOCTYPE html>
<html><head><title>%s</title></head>
<body><article>
<pre>%s</pre>
</article></body></html>`, escapeHTMLAttr(targetURL), escapeHTMLContent(media.Text))

	result := &FetchResult{
		URL:         targetURL,
		Status:      http.StatusOK,
		ContentType: "text/html",
		HTML:        htmlContent,
		RawHTML:     htmlContent,
		Links:       nil,
		Rendered:    false,
	}

	d.mu.Lock()
	d.last = result
	d.mu.Unlock()

	return result, nil
}

func (d *PDFEngine) HTML(_ context.Context) (string, error) {
	d.mu.RLock()
	defer d.mu.RUnlock()
	if d.last == nil {
		return "", fmt.Errorf("no document loaded")
	}
	return d.last.HTML, nil
}

// Interactive methods are not supported by the PDF engine.
func (d *PDFEngine) Click(_ context.Context, _ string) error {
	return fmt.Errorf("pdf engine does not support click")
}
func (d *PDFEngine) Type(_ context.Context, _, _ string) error {
	return fmt.Errorf("pdf engine does not support type")
}
func (d *PDFEngine) Press(_ context.Context, _ string) error {
	return fmt.Errorf("pdf engine does not support press")
}
func (d *PDFEngine) Wait(_ context.Context, _ int) error {
	return fmt.Errorf("pdf engine does not support wait")
}
func (d *PDFEngine) Scroll(_ context.Context, _ string) error {
	return fmt.Errorf("pdf engine does not support scroll")
}
func (d *PDFEngine) Screenshot(_ context.Context, _ bool) ([]byte, error) {
	return nil, fmt.Errorf("pdf engine does not support screenshot")
}
func (d *PDFEngine) EvalJS(_ context.Context, _ string) (interface{}, error) {
	return nil, fmt.Errorf("pdf engine does not support EvalJS")
}
func (d *PDFEngine) GeneratePDF(_ context.Context) ([]byte, error) {
	return nil, fmt.Errorf("pdf engine does not support GeneratePDF")
}
func (d *PDFEngine) Close() error { return nil }

// isDocumentURL checks URL extension for document types.
func isDocumentURL(u string) bool {
	lower := strings.ToLower(u)
	// Strip query/fragment before checking extension.
	if idx := strings.IndexAny(lower, "?#"); idx != -1 {
		lower = lower[:idx]
	}
	for _, ext := range []string{".pdf", ".docx", ".xlsx"} {
		if strings.HasSuffix(lower, ext) {
			return true
		}
	}
	return false
}

func escapeHTMLAttr(s string) string {
	s = strings.ReplaceAll(s, "&", "&amp;")
	s = strings.ReplaceAll(s, `"`, "&quot;")
	s = strings.ReplaceAll(s, "<", "&lt;")
	s = strings.ReplaceAll(s, ">", "&gt;")
	return s
}

func escapeHTMLContent(s string) string {
	s = strings.ReplaceAll(s, "&", "&amp;")
	s = strings.ReplaceAll(s, "<", "&lt;")
	s = strings.ReplaceAll(s, ">", "&gt;")
	return s
}

// DefaultDownloadTimeout limits how long document downloads can take.
const DefaultDownloadTimeout = 60 * time.Second

// DownloadDocument fetches raw document bytes with proper headers.
func DownloadDocument(ctx context.Context, docURL, userAgent string, headers map[string]string) ([]byte, string, error) {
	client := &http.Client{Timeout: DefaultDownloadTimeout}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, docURL, nil)
	if err != nil {
		return nil, "", err
	}
	if userAgent != "" {
		req.Header.Set("User-Agent", userAgent)
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, "", fmt.Errorf("download document: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 50*1024*1024)) // 50 MB limit
	if err != nil {
		return nil, "", fmt.Errorf("read document body: %w", err)
	}
	return body, resp.Header.Get("Content-Type"), nil
}
