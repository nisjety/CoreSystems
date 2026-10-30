package rawstore

import (
	"context"
	"encoding/json"
	"time"

	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/boamp"
	"github.com/triodelab/anbud/internal/sources/bzp"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"github.com/triodelab/anbud/internal/sources/fts"
	"github.com/triodelab/anbud/internal/sources/ted"
)

// PublicationDates reads publisher-provided dates from held search evidence, of every source, keyed by
// public id (internal/noticeid). It never substitutes issuance, a requested publication date or the
// local fetch timestamp.
func (s *Store) PublicationDates(ctx context.Context) (map[string]string, error) {
	rows, err := s.pool.Query(ctx, `SELECT source, body FROM raw_search_pages WHERE source IN ($1, $2, $3, $4, $5) ORDER BY id DESC LIMIT 1000`, noticeid.SourceDoffin, noticeid.SourceTED, noticeid.SourceBOAMP, noticeid.SourceFTS, noticeid.SourceBZP)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	dates := map[string]string{}
	for rows.Next() {
		var source string
		var body []byte
		if err := rows.Scan(&source, &body); err != nil {
			return nil, err
		}
		if source == noticeid.SourceTED || source == noticeid.SourceBOAMP || source == noticeid.SourceFTS || source == noticeid.SourceBZP {
			for number, day := range publicationDatesOf(source, body) {
				public := noticeid.Public(source, number)
				if _, ok := dates[public]; !ok {
					dates[public] = day
				}
			}
			continue
		}
		var page doffin.SearchResponse
		if json.Unmarshal(body, &page) != nil {
			continue
		}
		for _, hit := range page.Hits {
			if _, ok := dates[hit.ID]; ok {
				continue
			}
			if _, err := time.Parse("2006-01-02", hit.PublicationDate); err == nil {
				dates[hit.ID] = hit.PublicationDate
			}
		}
	}
	return dates, rows.Err()
}

// publicationDatesOf reads the publication days of a TED, BOAMP or Find a Tender search page, keyed by the
// source's own identifier.
func publicationDatesOf(source string, body []byte) map[string]string {
	switch source {
	case noticeid.SourceBOAMP:
		return boamp.PublicationDates(body)
	case noticeid.SourceFTS:
		return fts.PublicationDates(body)
	case noticeid.SourceBZP:
		return bzp.PublicationDates(body)
	}
	return ted.PublicationDates(body)
}
