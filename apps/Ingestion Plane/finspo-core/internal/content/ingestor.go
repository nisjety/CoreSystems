// Package content closes the SharePoint content gap: it downloads a synced
// DriveItem's bytes from Microsoft Graph, extracts plain text, and forwards a
// real content-bearing document to Data Plane v2. Before this existed the sync
// captured file/folder METADATA only, so the Data Plane forwarding hook was
// deliberately left inert (there was nothing honest to send). Now there is.
package content

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"

	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/dataplane"
	"github.com/triodelab/finspo/internal/extract"
	"github.com/triodelab/finspo/internal/store"
)

// ContentFetcher downloads a DriveItem's raw bytes. Implemented by
// *sharepoint.ContentClient.
type ContentFetcher interface {
	DownloadContent(ctx context.Context, organizationID, driveID, itemID string) ([]byte, error)
	MaxBytes() int64
}

// DocumentSink forwards a document to Data Plane v2. Implemented by
// *dataplane.DocumentsClient.
type DocumentSink interface {
	Configured() bool
	CreateDocument(ctx context.Context, orgID string, input dataplane.CreateDocumentInput) error
}

// Ingestor turns one synced file item into a Data Plane document.
type Ingestor struct {
	fetcher        ContentFetcher
	docs           DocumentSink
	logger         zerolog.Logger
	classification string
}

type Config struct {
	Fetcher ContentFetcher
	Docs    DocumentSink
	Logger  zerolog.Logger
	// ZDRClassification stamped on every forwarded SharePoint document. Empty
	// defaults to "internal" — enterprise SharePoint content is org-private, and
	// "internal" is Data Plane v2's customer-private tier. Set "restricted" for
	// libraries that must be excluded from reject-mode retrieval.
	ZDRClassification string
}

func NewIngestor(cfg Config) *Ingestor {
	classification := strings.TrimSpace(cfg.ZDRClassification)
	if classification == "" {
		classification = "internal"
	}
	return &Ingestor{
		fetcher:        cfg.Fetcher,
		docs:           cfg.Docs,
		logger:         cfg.Logger,
		classification: classification,
	}
}

// IngestItemContent downloads, extracts, and forwards one item's content.
//
// It is best-effort at the call site: the returned error is logged by the sync
// engine but never aborts the page, so one unreadable file cannot stall a whole
// drive's sync. Folders, oversized files, unsupported formats, and files whose
// extracted text is empty are all skipped cleanly (return nil, no error).
func (i *Ingestor) IngestItemContent(ctx context.Context, source store.Source, item store.Item) error {
	if i == nil || i.fetcher == nil || i.docs == nil || !i.docs.Configured() {
		return nil
	}
	if item.IsFolder {
		return nil
	}
	// Skip formats we cannot extract BEFORE spending a download.
	if extract.Classify(item.MimeType, item.Name) == extract.KindUnsupported {
		return nil
	}
	// Skip files we already know exceed the download ceiling from metadata.
	if max := i.fetcher.MaxBytes(); max > 0 && item.SizeBytes > max {
		i.logger.Debug().
			Str("item_id", item.ItemID).
			Int64("size_bytes", item.SizeBytes).
			Int64("max_bytes", max).
			Msg("skipping oversized item for content ingest")
		return nil
	}

	data, err := i.fetcher.DownloadContent(ctx, source.OrganizationID, source.DriveID, item.ItemID)
	if err != nil {
		return fmt.Errorf("download content for item %s: %w", item.ItemID, err)
	}

	text, supported, err := extract.Extract(item.MimeType, item.Name, data)
	if err != nil {
		return fmt.Errorf("extract content for item %s: %w", item.ItemID, err)
	}
	if !supported || strings.TrimSpace(text) == "" {
		// Nothing extractable (e.g. a scanned image-only PDF). Data Plane rejects
		// empty content, so skip rather than forward a body-less document.
		return nil
	}

	input := dataplane.CreateDocumentInput{
		Source:            "sharepoint",
		Type:              "sharepoint_file",
		Title:             documentTitle(item),
		Content:           text,
		ZDRClassification: i.classification,
		Metadata: map[string]any{
			"connector":    "sharepoint",
			"source_id":    source.ID.String(),
			"drive_id":     source.DriveID,
			"item_id":      item.ItemID,
			"path":         item.Path,
			"web_url":      item.WebURL,
			"mime_type":    item.MimeType,
			"size_bytes":   item.SizeBytes,
			"tenant_id":    source.TenantID,
			"site_web_url": source.SiteWebURL,
			"content_hash": contentHash(item),
		},
		// Stable per-item key so re-sync of a changed file UPDATEs the same Data
		// Plane document in place (re-index) rather than inserting a duplicate.
		// Hashed because documents-api restricts idempotency_key to
		// [alphanumerics - _ . :] and raw Graph drive ids violate that (they
		// start with "b!"), which 400'd every single content forward.
		IdempotencyKey: itemIdempotencyKey(source.DriveID, item.ItemID),
		// P2-3: the file's own SharePoint lastModifiedDateTime, so Data Plane
		// can rank on content freshness instead of ingestion time.
		ModifiedAt: item.ModifiedAt,
	}
	return i.docs.CreateDocument(ctx, source.OrganizationID, input)
}

func documentTitle(item store.Item) string {
	if strings.TrimSpace(item.Name) != "" {
		return item.Name
	}
	if strings.TrimSpace(item.Path) != "" {
		return item.Path
	}
	return item.ItemID
}

// itemIdempotencyKey derives a documents-api-safe stable key for a drive
// item. sha256 keeps it deterministic per (drive, item) while guaranteeing
// the [A-Za-z0-9._:-] charset the API enforces.
func itemIdempotencyKey(driveID, itemID string) string {
	sum := sha256.Sum256([]byte(driveID + ":" + itemID))
	return "finspo-sp:" + hex.EncodeToString(sum[:])
}

func contentHash(item store.Item) string {
	switch {
	case item.SHA1Hash != "":
		return "sha1:" + item.SHA1Hash
	case item.QuickXorHash != "":
		return "quickxor:" + item.QuickXorHash
	default:
		return ""
	}
}
