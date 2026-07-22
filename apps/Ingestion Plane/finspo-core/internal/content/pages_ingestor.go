package content

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"

	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/dataplane"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

// PageTextFetcher pulls one site page's plain text (canvasLayout text web
// parts). Implemented by *sharepoint.PagesClient.
type PageTextFetcher interface {
	FetchPageText(ctx context.Context, organizationID, siteID, pageID string) (string, error)
}

// PagesIngestor turns one synced SharePoint site page into a Data Plane
// document — the site-pages analog of Ingestor, with the download+extract
// step replaced by the Graph canvasLayout fetch.
type PagesIngestor struct {
	pages          PageTextFetcher
	docs           DocumentSink
	logger         zerolog.Logger
	classification string
}

type PagesConfig struct {
	Pages  PageTextFetcher
	Docs   DocumentSink
	Logger zerolog.Logger
	// ZDRClassification stamped on every forwarded page document; empty
	// defaults to "internal" exactly like the file ingestor.
	ZDRClassification string
}

func NewPagesIngestor(cfg PagesConfig) *PagesIngestor {
	classification := strings.TrimSpace(cfg.ZDRClassification)
	if classification == "" {
		classification = "internal"
	}
	return &PagesIngestor{
		pages:          cfg.Pages,
		docs:           cfg.Docs,
		logger:         cfg.Logger,
		classification: classification,
	}
}

// IngestSitePage fetches a page's text and forwards it as a document.
//
// Best-effort at the call site: the sync engine logs the returned error but
// never aborts the listing, so one unreadable page cannot stall a site's
// sync. Pages with no extractable text are skipped cleanly (return nil).
func (i *PagesIngestor) IngestSitePage(ctx context.Context, source store.Source, item store.Item, page sharepoint.SitePage) error {
	if i == nil || i.pages == nil || i.docs == nil || !i.docs.Configured() {
		return nil
	}

	text, err := i.pages.FetchPageText(ctx, source.OrganizationID, source.SiteID, page.ID)
	if err != nil {
		return fmt.Errorf("fetch text for site page %s: %w", page.ID, err)
	}
	if strings.TrimSpace(text) == "" {
		// A page whose canvas holds no text web parts (image/hero-only pages).
		// Data Plane rejects empty content, so skip rather than forward a
		// body-less document.
		return nil
	}

	input := dataplane.CreateDocumentInput{
		Source:            "sharepoint",
		Type:              "sharepoint_page",
		Title:             pageTitle(page),
		Content:           text,
		ZDRClassification: i.classification,
		Metadata: map[string]any{
			"connector":    "sharepoint",
			"source_id":    source.ID.String(),
			"site_id":      source.SiteID,
			"page_id":      page.ID,
			"name":         page.Name,
			"page_layout":  page.PageLayout,
			"path":         item.Path,
			"web_url":      page.WebURL,
			"mime_type":    "text/html",
			"tenant_id":    source.TenantID,
			"site_web_url": source.SiteWebURL,
		},
		// Stable per-page key so a re-sync of an edited page UPDATEs the same
		// Data Plane document in place rather than inserting a duplicate.
		// Hashed for the same reason as file items: raw Graph site ids contain
		// characters ("," etc.) outside documents-api's idempotency-key charset.
		IdempotencyKey: pageIdempotencyKey(source.SiteID, page.ID),
	}
	return i.docs.CreateDocument(ctx, source.OrganizationID, input)
}

func pageTitle(page sharepoint.SitePage) string {
	if strings.TrimSpace(page.Title) != "" {
		return page.Title
	}
	if strings.TrimSpace(page.Name) != "" {
		return page.Name
	}
	return page.ID
}

// pageIdempotencyKey derives a documents-api-safe stable key for a site page.
func pageIdempotencyKey(siteID, pageID string) string {
	sum := sha256.Sum256([]byte(siteID + ":" + pageID))
	return "finspo-sp-page:" + hex.EncodeToString(sum[:])
}
