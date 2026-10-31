package demo

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/rawstore"
)

// Increment on parser/projection changes so held notices are rebuilt once, off the
// interactive request path. Only public list metadata is persisted, never profiles,
// document contents or unverified winner names.
const catalogueProjectionVersion = 2

type projectedNotice struct {
	MatchingIdentity *matchingIdentity    `json:"matching_identity,omitempty"`
	Notice           CatalogueNotice      `json:"notice"`
	Deadlines        []normalize.Deadline `json:"deadlines"`
	UUID             string               `json:"uuid"`
	Version          string               `json:"version"`
	Source           string               `json:"source"`
}
type projectionStore interface {
	CatalogueGeneration(context.Context) (int64, error)
	CatalogueSourceGeneration(context.Context) (int64, error)
	PendingCatalogue(context.Context, int) ([]rawstore.CatalogueDocument, error)
	SaveCatalogueProjection(context.Context, int, []rawstore.CatalogueProjection) error
	MatchingCatalogue(context.Context, int, time.Time) ([]rawstore.CatalogueProjection, error)
	PreparedCatalogue(context.Context, int, int64) ([]rawstore.CatalogueProjection, error)
	PublicationDates(context.Context) (map[string]string, error)
	SetCataloguePublicationDates(context.Context, int, map[string]string) error
}
type catalogueSnapshot struct {
	generation int64
	revision   int64
	scanned    int
	notices    []projectedNotice
}
type cataloguePreparation struct {
	sync.Mutex
	generation int64
	prepared   bool
}

// PrepareCatalogue backfills once and then parses only newly held/changed bodies.
// The worker uses its own context, never an abandoned browser request's context.
func (s *Service) PrepareCatalogue(ctx context.Context) error {
	store, ok := s.notices.(projectionStore)
	if !ok {
		return nil
	}
	s.preparation.Lock()
	defer s.preparation.Unlock()
	generation, err := store.CatalogueSourceGeneration(ctx)
	if err != nil {
		return err
	}
	if s.preparation.prepared && generation == s.preparation.generation {
		return nil
	}
	dates, err := store.PublicationDates(ctx)
	if err != nil {
		return err
	}
	for {
		docs, err := store.PendingCatalogue(ctx, catalogueProjectionVersion)
		if err != nil {
			return err
		}
		if len(docs) == 0 {
			break
		}
		rows := make([]CatalogueNotice, 0, len(docs))
		items := make([]projectedNotice, 0, len(docs))
		awards := map[string][]incumbency.Award{}
		matchRows := make([]json.RawMessage, 0, len(docs))
		activeUntil := make([]*time.Time, 0, len(docs))
		for _, doc := range docs {
			if err := ctx.Err(); err != nil {
				return err
			}
			row, c := catalogueNotice(doc.Document, s.now())
			identity, matching, last := prepareMatching(doc.Document, c)
			matchRows = append(matchRows, matching)
			activeUntil = append(activeUntil, last)
			row.PublishedDate = dates[row.NoticeID]
			rows = append(rows, row)
			items = append(items, projectedNotice{MatchingIdentity: identity, Notice: row, Deadlines: c.Norm.Deadlines, UUID: c.Notice.NoticeID, Version: c.Notice.VersionID, Source: doc.Document.Source})
			awards[row.NoticeID] = incumbency.FromNotice(row.NoticeID, c.Notice)
		}
		s.enrichCatalogueWinners(ctx, rows, awards)
		entries := make([]rawstore.CatalogueProjection, 0, len(rows))
		for i, row := range rows {
			items[i].Notice = row
			payload, err := json.Marshal(items[i])
			if err != nil {
				return err
			}
			entries = append(entries, rawstore.CatalogueProjection{Sequence: docs[i].Sequence, Payload: payload, MatchingPayload: matchRows[i], ActiveUntil: activeUntil[i]})
		}
		if err := store.SaveCatalogueProjection(ctx, catalogueProjectionVersion, entries); err != nil {
			return err
		}
	}
	if err := store.SetCataloguePublicationDates(ctx, catalogueProjectionVersion, dates); err != nil {
		return err
	}
	// Imports racing preparation are detected on the next poll.
	s.preparation.generation = generation
	s.preparation.prepared = true
	return nil
}

func (s *Service) RunCataloguePreparation(ctx context.Context) {
	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			refresh, cancel := context.WithTimeout(ctx, 2*time.Minute)
			err := s.PrepareCatalogue(refresh)
			cancel()
			if err != nil && ctx.Err() == nil && s.log != nil {
				s.log.Error().Err(err).Msg("catalogue preparation failed")
			}
		}
	}
}

// preparedCatalogue uses one bounded, immutable corpus snapshot. Each request
// checks the database generation: erasure and imports invalidate the snapshot.
// Filtering/sorting copy list structs, so concurrent requests cannot mutate it.
func (s *Service) preparedCatalogue(ctx context.Context, r CatalogueRequest) ([]CatalogueNotice, int64, int, error) {
	store := s.notices.(projectionStore)
	generation, err := store.CatalogueGeneration(ctx)
	if err != nil {
		return nil, 0, 0, err
	}
	requested := int64(0)
	if r.Cursor != "" {
		c, err := readCatalogueCursor(r)
		if err != nil {
			return nil, 0, 0, err
		}
		if c.Revision <= 0 || c.Revision == int64(^uint64(0)>>1) {
			return nil, 0, 0, ErrCatalogueRequest
		}
		requested = c.Revision
	}
	s.catalogueMu.Lock()
	defer s.catalogueMu.Unlock()
	snapshot := s.catalogueSnapshot
	if snapshot == nil || snapshot.generation != generation || (requested != 0 && snapshot.revision != requested) {
		entries, err := store.PreparedCatalogue(ctx, catalogueProjectionVersion, requested)
		if err != nil {
			return nil, 0, 0, err
		}
		snapshot = &catalogueSnapshot{generation: generation, scanned: len(entries)}
		copies := make([]twin, 0, len(entries))
		items := make([]projectedNotice, 0, len(entries))
		for _, entry := range entries {
			var item projectedNotice
			if err := json.Unmarshal(entry.Payload, &item); err != nil {
				return nil, 0, 0, err
			}
			if entry.Sequence > snapshot.revision {
				snapshot.revision = entry.Sequence
			}
			copies = append(copies, twin{noticeID: item.UUID, version: item.Version, source: item.Source})
			items = append(items, item)
		}
		keep := keepOnePerNotice(copies)
		for i, item := range items {
			if keep[i] {
				snapshot.notices = append(snapshot.notices, item)
			}
		}
		// If an import/redaction raced the read, do not publish a cache entry under
		// its old generation; the next request must reread live rows.
		current, err := store.CatalogueGeneration(ctx)
		if err != nil {
			return nil, 0, 0, err
		}
		if current == generation {
			s.catalogueSnapshot = snapshot
		}
	}
	now := s.now()
	out := make([]CatalogueNotice, 0, len(snapshot.notices))
	for _, item := range snapshot.notices {
		row := item.Notice
		row.Deadline = nil
		norm := normalize.Result{Deadlines: item.Deadlines}
		d, open := match.NextOpenDeadline(norm, now)
		if !open && len(item.Deadlines) > 0 {
			d = item.Deadlines[0]
			open = true
		}
		if open {
			row.Deadline = &DeadlineView{At: d.At.UTC(), OsloDay: normalize.OsloDay(d.At), DaysLeft: normalize.DaysLeft(d.At, now), Kind: string(d.Kind), Lot: d.Lot}
		}
		if row.Status == "active" || row.Status == "expired" {
			if row.Deadline != nil && row.Deadline.DaysLeft < 0 {
				row.Status = "expired"
			} else {
				row.Status = "active"
			}
		}
		out = append(out, row)
	}
	return out, snapshot.revision, snapshot.scanned, nil
}
