// Package sync drives the per-drive Microsoft Graph delta sync loop.
package sync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

// DeltaFetcher is the narrow surface the engine needs from the delta client —
// extracted so tests can inject a fake without standing up a fake Graph server.
type DeltaFetcher interface {
	InitialDeltaURL(driveID string) string
	Fetch(ctx context.Context, organizationID, deltaURL string) (sharepoint.DeltaPage, error)
}

// SourceStore + ItemStore + CursorStore are the persistence dependencies of
// the engine, exposed as interfaces so tests can substitute fakes.
type SourceStore interface {
	Get(ctx context.Context, id uuid.UUID) (store.Source, error)
}

type ItemStore interface {
	Upsert(ctx context.Context, sourceID uuid.UUID, organizationID string, d sharepoint.DriveItem, rawJSON []byte) (store.UpsertResult, error)
	SoftDelete(ctx context.Context, sourceID uuid.UUID, itemID string) (store.Item, error)
}

type CursorStore interface {
	Get(ctx context.Context, sourceID uuid.UUID) (store.Cursor, error)
	Save(ctx context.Context, sourceID uuid.UUID, deltaLink, status, errMsg string, itemsDelta int64) (store.Cursor, error)
}

// Publisher is the events.Publisher contract the engine uses. We accept it as
// an interface so tests can assert which subjects/payloads were emitted.
type Publisher interface {
	Publish(subject string, payload any) error
}

type SourceObjectSink interface {
	UpsertSourceObject(ctx context.Context, source store.Source, item store.Item) error
	DeleteSourceObject(ctx context.Context, source store.Source, item store.Item) error
}

type PermissionAwareSourceObjectSink interface {
	UpsertSourceObjectWithPermissions(ctx context.Context, source store.Source, item store.Item, permissions []store.Permission) error
}

// ContentSink forwards a synced file's extracted text to Data Plane v2 as a
// content-bearing document. Optional and nil-safe: when unset the engine
// captures metadata only (the historical behavior). Implemented by
// *content.Ingestor. Calls are best-effort — one file's failure is logged and
// the page continues.
type ContentSink interface {
	// permissions is the item's freshly-captured ACL, which the sink uses to
	// decide the forwarded document's visibility. It is empty when permission
	// capture is disabled or failed; sinks must fail closed on that.
	IngestItemContent(ctx context.Context, source store.Source, item store.Item, permissions []store.Permission) error
}

// SitePagesLister is the narrow Graph surface the engine needs to sync a
// site-pages source. Implemented by *sharepoint.PagesClient.
type SitePagesLister interface {
	ListSitePages(ctx context.Context, organizationID, siteID string) ([]sharepoint.SitePage, error)
}

// PageContentSink forwards one site page's extracted text to Data Plane v2 as
// a content-bearing document. Optional and nil-safe like ContentSink: when
// unset the engine captures page metadata only. Implemented by
// *content.PagesIngestor. Calls are best-effort — one page's failure is
// logged and the listing continues.
type PageContentSink interface {
	IngestSitePage(ctx context.Context, source store.Source, item store.Item, page sharepoint.SitePage) error
}

// PermissionsFetcher is the narrow Graph surface the engine needs to capture
// ACLs. Implemented by *sharepoint.PermissionsClient.
type PermissionsFetcher interface {
	ListItemPermissions(ctx context.Context, organizationID, driveID, itemID string) ([]sharepoint.PermissionEntry, error)
}

// PermissionsStore is the narrow persistence surface needed to write ACL
// summaries. Implemented by *store.Permissions.
type PermissionsStore interface {
	ReplaceAll(ctx context.Context, itemPK uuid.UUID, entries []sharepoint.PermissionEntry) ([]store.Permission, error)
}

// Engine ties the Graph delta client to the finspo Postgres tables and the
// event bus. One Engine instance handles every drive across every tenant.
type Engine struct {
	fetcher            DeltaFetcher
	sources            SourceStore
	items              ItemStore
	cursors            CursorStore
	publisher          Publisher
	sink               SourceObjectSink
	permissionsFetcher PermissionsFetcher
	permissionsStore   PermissionsStore
	capturePerms       bool
	content            ContentSink
	sitePages          SitePagesLister
	pageContent        PageContentSink
	subjects           events.Subjects
	logger             zerolog.Logger
	pageLimit          int
}

type Config struct {
	Fetcher   DeltaFetcher
	Sources   SourceStore
	Items     ItemStore
	Cursors   CursorStore
	Publisher Publisher
	Sink      SourceObjectSink
	Subjects  events.Subjects
	Logger    zerolog.Logger

	// PermissionsFetcher / PermissionsStore enable ACL capture. Both must be
	// non-nil AND CapturePermissions=true for the engine to fetch permissions
	// per file. Folders are always skipped (permissions on a folder apply
	// transitively via inheritedFrom on its children).
	PermissionsFetcher PermissionsFetcher
	PermissionsStore   PermissionsStore
	CapturePermissions bool

	// Content, when non-nil, forwards each synced file's extracted text to Data
	// Plane v2 as a document. Nil keeps the metadata-only behavior.
	Content ContentSink

	// SitePages lists a site's pages for site_pages-kind sources. Required to
	// sync those sources; drive sources ignore it.
	SitePages SitePagesLister

	// PageContent, when non-nil, forwards each synced site page's text to Data
	// Plane v2 as a document — the site-pages analog of Content.
	PageContent PageContentSink

	// PageLimit caps the number of delta pages followed in a single SyncDrive
	// call. 0 means "no cap" — useful for tests, but production deployments
	// should set a finite value so a runaway initial crawl cannot starve
	// other sources.
	PageLimit int
}

func NewEngine(cfg Config) *Engine {
	return &Engine{
		fetcher:            cfg.Fetcher,
		sources:            cfg.Sources,
		items:              cfg.Items,
		cursors:            cfg.Cursors,
		publisher:          cfg.Publisher,
		sink:               cfg.Sink,
		permissionsFetcher: cfg.PermissionsFetcher,
		permissionsStore:   cfg.PermissionsStore,
		capturePerms:       cfg.CapturePermissions,
		content:            cfg.Content,
		sitePages:          cfg.SitePages,
		pageContent:        cfg.PageContent,
		subjects:           cfg.Subjects,
		logger:             cfg.Logger,
		pageLimit:          cfg.PageLimit,
	}
}

// permissionsEnabled reports whether the engine is configured to fetch and
// persist permissions for upserted items.
func (e *Engine) permissionsEnabled() bool {
	return e.capturePerms && e.permissionsFetcher != nil && e.permissionsStore != nil
}

// SyncResult summarizes one SyncDrive invocation.
type SyncResult struct {
	SourceID      uuid.UUID `json:"source_id"`
	Pages         int       `json:"pages"`
	ItemsUpserted int       `json:"items_upserted"`
	ItemsDeleted  int       `json:"items_deleted"`
	StartedAt     time.Time `json:"started_at"`
	CompletedAt   time.Time `json:"completed_at"`
	DeltaLink     string    `json:"delta_link,omitempty"`
}

// SyncDrive runs one sync pass for one source. Drive sources run the Graph
// delta loop (safe to call repeatedly: on the second call the persisted
// deltaLink resumes from where the prior run stopped); site-pages sources run
// a full sitePages listing (the pages API has no delta). The name predates
// source kinds — it remains the single entry point the scheduler and the API
// sync endpoint call for every source.
func (e *Engine) SyncDrive(ctx context.Context, sourceID uuid.UUID) (SyncResult, error) {
	started := time.Now().UTC()

	source, err := e.sources.Get(ctx, sourceID)
	if err != nil {
		return SyncResult{}, fmt.Errorf("load source: %w", err)
	}

	if store.NormalizeKind(source.Kind) == store.SourceKindSitePages {
		return e.syncSitePages(ctx, source, started)
	}

	// Decide the starting URL: persisted deltaLink wins; otherwise start fresh.
	startURL, err := e.startingURL(ctx, source)
	if err != nil {
		return SyncResult{}, err
	}

	result := SyncResult{SourceID: source.ID, StartedAt: started}
	nextURL := startURL
	for {
		if e.pageLimit > 0 && result.Pages >= e.pageLimit {
			e.logger.Warn().
				Str("source_id", source.ID.String()).
				Int("page_limit", e.pageLimit).
				Msg("delta page limit reached; resume on next sync")
			break
		}

		page, fetchErr := e.fetcher.Fetch(ctx, source.OrganizationID, nextURL)
		if fetchErr != nil {
			e.recordFailure(ctx, source.ID, fetchErr)
			return result, fmt.Errorf("fetch delta: %w", fetchErr)
		}
		result.Pages++

		ups, dels, processErr := e.processPage(ctx, source, page)
		result.ItemsUpserted += ups
		result.ItemsDeleted += dels
		if processErr != nil {
			e.recordFailure(ctx, source.ID, processErr)
			return result, processErr
		}

		switch {
		case page.NextLink != "":
			nextURL = page.NextLink
		case page.DeltaLink != "":
			result.DeltaLink = page.DeltaLink
		}

		if page.NextLink == "" {
			break
		}
	}

	result.CompletedAt = time.Now().UTC()
	if _, err := e.cursors.Save(ctx, source.ID, result.DeltaLink, "ok", "", int64(result.ItemsUpserted+result.ItemsDeleted)); err != nil {
		return result, fmt.Errorf("save cursor: %w", err)
	}

	if e.publisher != nil {
		_ = e.publisher.Publish(e.subjects.SourceSynced(), events.SourceSynced{
			OrganizationID: source.OrganizationID,
			SourceID:       source.ID.String(),
			ItemsUpserted:  result.ItemsUpserted,
			ItemsDeleted:   result.ItemsDeleted,
			StartedAt:      result.StartedAt,
			CompletedAt:    result.CompletedAt,
		})
	}

	e.logger.Info().
		Str("source_id", source.ID.String()).
		Int("pages", result.Pages).
		Int("upserts", result.ItemsUpserted).
		Int("deletes", result.ItemsDeleted).
		Msg("delta sync complete")

	return result, nil
}

func (e *Engine) startingURL(ctx context.Context, source store.Source) (string, error) {
	cur, err := e.cursors.Get(ctx, source.ID)
	if err != nil && !errors.Is(err, store.ErrNotFound) {
		return "", fmt.Errorf("load cursor: %w", err)
	}
	if cur.DeltaLink != "" {
		return cur.DeltaLink, nil
	}
	return e.fetcher.InitialDeltaURL(source.DriveID), nil
}

func (e *Engine) processPage(ctx context.Context, source store.Source, page sharepoint.DeltaPage) (int, int, error) {
	var upserts, deletes int

	for _, item := range page.Items {
		if item.ID == "" {
			continue
		}
		if item.IsDeleted() {
			res, err := e.items.SoftDelete(ctx, source.ID, item.ID)
			if errors.Is(err, store.ErrNotFound) {
				// Tombstone for an item we never saw — Graph emits these for
				// items created and deleted between two delta windows. Not
				// an error, but worth logging at debug level.
				e.logger.Debug().
					Str("source_id", source.ID.String()).
					Str("item_id", item.ID).
					Msg("delta tombstone for unknown item; ignoring")
				continue
			}
			if err != nil {
				return upserts, deletes, fmt.Errorf("soft-delete %s: %w", item.ID, err)
			}
			if e.sink != nil {
				if err := e.sink.DeleteSourceObject(ctx, source, res); err != nil {
					return upserts, deletes, fmt.Errorf("delete data-plane source object %s: %w", item.ID, err)
				}
			}
			deletes++
			if e.publisher != nil {
				_ = e.publisher.Publish(e.subjects.ItemDeleted(), events.ItemDeleted{
					OrganizationID: source.OrganizationID,
					SourceID:       source.ID.String(),
					ItemPK:         res.ID.String(),
					ItemID:         res.ItemID,
					Path:           res.Path,
					ObservedAt:     time.Now().UTC(),
				})
			}
			continue
		}

		// Folder-scoped source: the delta feed still enumerates the WHOLE
		// drive, so items outside the scoped subtree are dropped here rather
		// than persisted. Deletes above are unaffected — out-of-scope items
		// were never upserted, so their tombstones fall through SoftDelete's
		// ErrNotFound path.
		if !itemInScope(source, item) {
			e.logger.Debug().
				Str("source_id", source.ID.String()).
				Str("item_id", item.ID).
				Str("path", item.FullPath()).
				Str("folder_path", source.FolderPath).
				Msg("delta item outside scoped folder; skipping")
			continue
		}

		raw, err := json.Marshal(item)
		if err != nil {
			return upserts, deletes, fmt.Errorf("marshal raw item %s: %w", item.ID, err)
		}

		res, err := e.items.Upsert(ctx, source.ID, source.OrganizationID, item, raw)
		if err != nil {
			return upserts, deletes, fmt.Errorf("upsert %s: %w", item.ID, err)
		}
		var permissions []store.Permission
		if e.permissionsEnabled() && !item.IsFolder() {
			captured, err := e.capturePermissionsForItem(ctx, source, res.Item)
			if err != nil {
				// ACL capture is best-effort: the item itself is already
				// persisted and the event has not yet been emitted, so we
				// must not abort the whole page. Log and move on.
				e.logger.Warn().Err(err).
					Str("source_id", source.ID.String()).
					Str("item_id", item.ID).
					Msg("permission capture failed; continuing")
			} else {
				permissions = captured
			}
		}
		if e.sink != nil {
			if permissionAware, ok := e.sink.(PermissionAwareSourceObjectSink); ok {
				if err := permissionAware.UpsertSourceObjectWithPermissions(ctx, source, res.Item, permissions); err != nil {
					return upserts, deletes, fmt.Errorf("upsert data-plane source object %s: %w", item.ID, err)
				}
			} else if err := e.sink.UpsertSourceObject(ctx, source, res.Item); err != nil {
				return upserts, deletes, fmt.Errorf("upsert data-plane source object %s: %w", item.ID, err)
			}
		}
		// Content ingest is best-effort and gated on a configured ContentSink:
		// a single unreadable/oversized file must not abort the page or fail the
		// metadata sync it rides alongside. Folders are skipped inside the sink.
		if e.content != nil && !item.IsFolder() {
			if err := e.content.IngestItemContent(ctx, source, res.Item, permissions); err != nil {
				e.logger.Warn().Err(err).
					Str("source_id", source.ID.String()).
					Str("item_id", item.ID).
					Msg("content ingest failed; continuing")
			}
		}
		upserts++
		if e.publisher != nil {
			_ = e.publisher.Publish(e.subjects.ItemUpserted(), events.ItemUpserted{
				OrganizationID: source.OrganizationID,
				SourceID:       source.ID.String(),
				ItemPK:         res.Item.ID.String(),
				ItemID:         res.Item.ItemID,
				ParentItemID:   res.Item.ParentItemID,
				Path:           res.Item.Path,
				Name:           res.Item.Name,
				MimeType:       res.Item.MimeType,
				SizeBytes:      res.Item.SizeBytes,
				IsFolder:       res.Item.IsFolder,
				WebURL:         res.Item.WebURL,
				QuickXorHash:   res.Item.QuickXorHash,
				SHA1Hash:       res.Item.SHA1Hash,
				Inserted:       res.Inserted,
				ObservedAt:     time.Now().UTC(),
			})
		}
	}

	return upserts, deletes, nil
}

// itemInScope reports whether a delta item falls inside the source's scoped
// folder. Sources without a folder scope accept everything.
func itemInScope(source store.Source, item sharepoint.DriveItem) bool {
	if source.FolderPath == "" {
		return true
	}
	return pathWithinFolder(source.FolderPath, item.FullPath())
}

// pathWithinFolder reports whether itemPath is the scoped folder itself or
// anything beneath it. SharePoint paths are case-insensitive, so the
// comparison folds case.
func pathWithinFolder(folderPath, itemPath string) bool {
	scope := strings.ToLower(strings.TrimSuffix(folderPath, "/"))
	if scope == "" {
		return true
	}
	p := strings.ToLower(itemPath)
	return p == scope || strings.HasPrefix(p, scope+"/")
}

func (e *Engine) capturePermissionsForItem(ctx context.Context, source store.Source, item store.Item) ([]store.Permission, error) {
	perms, err := e.permissionsFetcher.ListItemPermissions(ctx, source.OrganizationID, source.DriveID, item.ItemID)
	if err != nil {
		return nil, fmt.Errorf("fetch permissions: %w", err)
	}
	saved, err := e.permissionsStore.ReplaceAll(ctx, item.ID, perms)
	if err != nil {
		return nil, fmt.Errorf("persist permissions: %w", err)
	}
	return saved, nil
}

func (e *Engine) recordFailure(ctx context.Context, sourceID uuid.UUID, cause error) {
	// Best-effort: we cannot let a cursor write failure overwrite the real
	// error. Log and move on.
	if _, err := e.cursors.Save(ctx, sourceID, "", "error", truncate(cause.Error(), 1024), 0); err != nil {
		e.logger.Error().Err(err).Str("source_id", sourceID.String()).Msg("cursor save (failure path) failed")
	}
}

func truncate(s string, max int) string {
	if len(s) <= max {
		return s
	}
	return s[:max]
}
