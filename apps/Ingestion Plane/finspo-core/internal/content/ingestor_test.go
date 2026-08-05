package content

import (
	"context"
	"errors"
	"regexp"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/dataplane"
	"github.com/triodelab/finspo/internal/store"
)

// fakeFetcher records download calls and returns canned bytes/errors.
type fakeFetcher struct {
	data     []byte
	err      error
	maxBytes int64
	calls    int
}

func (f *fakeFetcher) DownloadContent(ctx context.Context, org, drive, item string) ([]byte, error) {
	f.calls++
	return f.data, f.err
}
func (f *fakeFetcher) MaxBytes() int64 { return f.maxBytes }

// fakeSink records forwarded documents.
type fakeSink struct {
	configured bool
	created    []dataplane.CreateDocumentInput
	orgs       []string
	err        error
}

func (s *fakeSink) Configured() bool { return s.configured }
func (s *fakeSink) CreateDocument(ctx context.Context, org string, in dataplane.CreateDocumentInput) error {
	if s.err != nil {
		return s.err
	}
	s.created = append(s.created, in)
	s.orgs = append(s.orgs, org)
	return nil
}

func newIngestor(f *fakeFetcher, s *fakeSink, class string) *Ingestor {
	return NewIngestor(Config{Fetcher: f, Docs: s, Logger: zerolog.Nop(), ZDRClassification: class})
}

func srcItem(name, mime string, size int64, folder bool) (store.Source, store.Item) {
	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", DriveID: "drive-1", TenantID: "tenant-1", SiteWebURL: "https://sp/site"}
	item := store.Item{ItemID: "item-1", Name: name, MimeType: mime, SizeBytes: size, IsFolder: folder, WebURL: "https://sp/f", Path: "/f/" + name, SHA1Hash: "abc"}
	return src, item
}

func TestIngestor_HappyPathForwardsDocument(t *testing.T) {
	f := &fakeFetcher{data: []byte("real document text"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	ing := newIngestor(f, s, "internal")

	src, item := srcItem("plan.txt", "text/plain", 18, false)
	if err := ing.IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if len(s.created) != 1 {
		t.Fatalf("expected 1 forwarded doc, got %d", len(s.created))
	}
	doc := s.created[0]
	if doc.Content != "real document text" {
		t.Errorf("content = %q", doc.Content)
	}
	if doc.Source != "sharepoint" || doc.Type != "sharepoint_file" {
		t.Errorf("source/type = %q/%q", doc.Source, doc.Type)
	}
	if doc.Title != "plan.txt" {
		t.Errorf("title = %q", doc.Title)
	}
	if doc.ZDRClassification != "internal" {
		t.Errorf("zdr = %q", doc.ZDRClassification)
	}
	// Hashed key: raw Graph drive ids ("b!...") violate documents-api's
	// idempotency_key charset, so the key is sha256(drive:item) — assert
	// determinism + charset safety rather than the raw concatenation.
	if doc.IdempotencyKey != itemIdempotencyKey("drive-1", "item-1") {
		t.Errorf("idempotency = %q", doc.IdempotencyKey)
	}
	if !regexp.MustCompile(`^[A-Za-z0-9._:-]+$`).MatchString(itemIdempotencyKey("b!x7/y+z==", "01ITEM")) {
		t.Error("idempotency key must stay within documents-api's allowed charset even for raw Graph ids")
	}
	if doc.Metadata["content_hash"] != "sha1:abc" {
		t.Errorf("content_hash = %v", doc.Metadata["content_hash"])
	}
	if s.orgs[0] != "org-1" {
		t.Errorf("org = %q", s.orgs[0])
	}
}

// TestIngestor_ForwardsModifiedAt covers P2-3: the drive item's own
// lastModifiedDateTime must reach the forwarded document as ModifiedAt, so
// Data Plane can persist document_date instead of only its own ingest-time
// bookkeeping.
func TestIngestor_ForwardsModifiedAt(t *testing.T) {
	f := &fakeFetcher{data: []byte("real document text"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	ing := newIngestor(f, s, "internal")

	src, item := srcItem("plan.txt", "text/plain", 18, false)
	modified := time.Date(2024, 3, 1, 12, 0, 0, 0, time.UTC)
	item.ModifiedAt = &modified
	if err := ing.IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatalf("ingest: %v", err)
	}
	if len(s.created) != 1 {
		t.Fatalf("expected 1 forwarded doc, got %d", len(s.created))
	}
	if got := s.created[0].ModifiedAt; got == nil || !got.Equal(modified) {
		t.Errorf("ModifiedAt = %v, want %v", got, modified)
	}
}

func TestIngestor_SkipsFolder(t *testing.T) {
	f := &fakeFetcher{data: []byte("x"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	src, item := srcItem("folder", "", 0, true)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if f.calls != 0 || len(s.created) != 0 {
		t.Fatal("folder must not be downloaded or forwarded")
	}
}

func TestIngestor_SkipsUnsupportedType(t *testing.T) {
	f := &fakeFetcher{data: []byte("x"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	src, item := srcItem("logo.png", "image/png", 10, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if f.calls != 0 {
		t.Fatal("unsupported type must be skipped before download")
	}
}

func TestIngestor_SkipsOversizedByMetadata(t *testing.T) {
	f := &fakeFetcher{data: []byte("x"), maxBytes: 100}
	s := &fakeSink{configured: true}
	src, item := srcItem("big.txt", "text/plain", 999, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if f.calls != 0 {
		t.Fatal("oversized item must be skipped before download")
	}
}

func TestIngestor_SkipsEmptyExtractedText(t *testing.T) {
	f := &fakeFetcher{data: []byte("   "), maxBytes: 1000}
	s := &fakeSink{configured: true}
	src, item := srcItem("blank.txt", "text/plain", 3, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if len(s.created) != 0 {
		t.Fatal("empty extracted text must not be forwarded (Data Plane rejects it)")
	}
}

func TestIngestor_DownloadErrorReturnsError(t *testing.T) {
	f := &fakeFetcher{err: errors.New("graph 500"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	src, item := srcItem("plan.txt", "text/plain", 10, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err == nil {
		t.Fatal("download failure must surface as an error for best-effort logging")
	}
}

func TestIngestor_UnconfiguredSinkIsNoOp(t *testing.T) {
	f := &fakeFetcher{data: []byte("text"), maxBytes: 1000}
	s := &fakeSink{configured: false}
	src, item := srcItem("plan.txt", "text/plain", 10, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if f.calls != 0 || len(s.created) != 0 {
		t.Fatal("unconfigured sink must short-circuit before any work")
	}
}

func TestIngestor_DefaultClassificationInternal(t *testing.T) {
	f := &fakeFetcher{data: []byte("body"), maxBytes: 1000}
	s := &fakeSink{configured: true}
	src, item := srcItem("a.txt", "text/plain", 4, false)
	if err := newIngestor(f, s, "").IngestItemContent(context.Background(), src, item); err != nil {
		t.Fatal(err)
	}
	if s.created[0].ZDRClassification != "internal" {
		t.Fatalf("empty classification must default to internal, got %q", s.created[0].ZDRClassification)
	}
}
