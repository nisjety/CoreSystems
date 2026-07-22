package sync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

// syncSitePages runs one full sync pass for a site_pages-kind source. The
// Graph sitePages API has no delta endpoint, so every pass lists the whole
// site and upserts each page; the delta cursor row is reused purely as the
// last-sync status record (its deltaLink stays empty).
func (e *Engine) syncSitePages(ctx context.Context, source store.Source, started time.Time) (SyncResult, error) {
	result := SyncResult{SourceID: source.ID, StartedAt: started}

	if e.sitePages == nil {
		err := errors.New("site pages sync not configured")
		e.recordFailure(ctx, source.ID, err)
		return result, err
	}

	pages, err := e.sitePages.ListSitePages(ctx, source.OrganizationID, source.SiteID)
	if err != nil {
		e.recordFailure(ctx, source.ID, err)
		return result, fmt.Errorf("list site pages: %w", err)
	}
	result.Pages = 1

	for _, page := range pages {
		if page.ID == "" {
			continue
		}

		item := sitePageDriveItem(page)
		raw, err := json.Marshal(page)
		if err != nil {
			e.recordFailure(ctx, source.ID, err)
			return result, fmt.Errorf("marshal raw site page %s: %w", page.ID, err)
		}

		res, err := e.items.Upsert(ctx, source.ID, source.OrganizationID, item, raw)
		if err != nil {
			e.recordFailure(ctx, source.ID, err)
			return result, fmt.Errorf("upsert site page %s: %w", page.ID, err)
		}

		// The source-object sink (e.sink) is deliberately not called here: its
		// payloads are keyed around a drive id, which site-pages sources do not
		// have. The content-bearing document below is the Data Plane projection
		// of a page.
		if e.pageContent != nil {
			if err := e.pageContent.IngestSitePage(ctx, source, res.Item, page); err != nil {
				e.logger.Warn().Err(err).
					Str("source_id", source.ID.String()).
					Str("page_id", page.ID).
					Msg("site page content ingest failed; continuing")
			}
		}

		result.ItemsUpserted++
		if e.publisher != nil {
			_ = e.publisher.Publish(e.subjects.ItemUpserted(), events.ItemUpserted{
				OrganizationID: source.OrganizationID,
				SourceID:       source.ID.String(),
				ItemPK:         res.Item.ID.String(),
				ItemID:         res.Item.ItemID,
				Path:           res.Item.Path,
				Name:           res.Item.Name,
				MimeType:       res.Item.MimeType,
				IsFolder:       false,
				WebURL:         res.Item.WebURL,
				Inserted:       res.Inserted,
				ObservedAt:     time.Now().UTC(),
			})
		}
	}

	result.CompletedAt = time.Now().UTC()
	if _, err := e.cursors.Save(ctx, source.ID, "", "ok", "", int64(result.ItemsUpserted)); err != nil {
		return result, fmt.Errorf("save cursor: %w", err)
	}

	if e.publisher != nil {
		_ = e.publisher.Publish(e.subjects.SourceSynced(), events.SourceSynced{
			OrganizationID: source.OrganizationID,
			SourceID:       source.ID.String(),
			ItemsUpserted:  result.ItemsUpserted,
			ItemsDeleted:   0,
			StartedAt:      result.StartedAt,
			CompletedAt:    result.CompletedAt,
		})
	}

	e.logger.Info().
		Str("source_id", source.ID.String()).
		Int("pages", len(pages)).
		Int("upserts", result.ItemsUpserted).
		Msg("site pages sync complete")

	return result, nil
}

// sitePageDriveItem projects a SitePage onto the DriveItem shape the items
// store persists, so pages live in the same table as files with an honest
// mime type and no fabricated drive metadata.
func sitePageDriveItem(page sharepoint.SitePage) sharepoint.DriveItem {
	return sharepoint.DriveItem{
		ID:                   page.ID,
		Name:                 page.Name,
		WebURL:               page.WebURL,
		LastModifiedDateTime: page.LastModifiedDateTime,
		File:                 &sharepoint.FileFacet{MimeType: "text/html"},
	}
}
