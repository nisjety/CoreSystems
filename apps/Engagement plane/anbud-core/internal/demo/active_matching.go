package demo

import (
	"context"
	"encoding/json"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/rawstore"
)

type matchingIdentity struct {
	ID        string
	Source    string
	FetchedAt time.Time
	Header    eforms.Header
	IssuedAt  *time.Time
	Deadlines []normalize.Deadline
}

func prepareMatching(doc rawstore.Document, c candidate) (*matchingIdentity, json.RawMessage, *time.Time) {
	if c.Notice.Root != "ContractNotice" {
		return nil, nil, nil
	}
	identity := &matchingIdentity{ID: doc.ID, Source: doc.Source, FetchedAt: doc.FetchedAt, Header: c.Notice.Header, IssuedAt: c.Notice.IssuedAt, Deadlines: c.Norm.Deadlines}
	n := c.Notice
	// Winner/contact data is irrelevant to opportunity matching and stays out of this projection.
	n.Results = nil
	n.ContactPoints = 0
	n.Unmapped = nil
	n.Issues = nil
	norm := c.Norm
	norm.WinnerOrgNumbers = nil
	body, _ := json.Marshal(match.Candidate{Notice: n, Norm: norm})
	var latest *time.Time
	for _, d := range norm.Deadlines {
		if latest == nil || d.At.After(*latest) {
			at := d.At
			latest = &at
		}
	}
	return identity, body, latest
}

// activeContracts traverses every held notice for fake/legacy sources too. The
// runtime uses prepared facts and reads full text only for currently open notices.
func (s *Service) activeContracts(ctx context.Context, now time.Time) ([]candidate, Funnel, error) {
	var cs []candidate
	var copies []twin
	funnel := Funnel{Gate: map[string]int{"pass": 0, "conditional": 0, "fail": 0}}
	if store, ok := s.notices.(projectionStore); ok {
		rows, err := store.MatchingCatalogue(ctx, catalogueProjectionVersion, now)
		if err != nil {
			return nil, funnel, err
		}
		for _, row := range rows {
			if err := ctx.Err(); err != nil {
				return nil, funnel, err
			}
			var identity matchingIdentity
			if err := json.Unmarshal(row.Payload, &identity); err != nil {
				return nil, funnel, err
			}
			c := candidate{id: identity.ID, fetched: identity.FetchedAt, Candidate: match.Candidate{Notice: eforms.Notice{Header: identity.Header, IssuedAt: identity.IssuedAt}, Norm: normalize.Result{Deadlines: identity.Deadlines}}}
			if len(row.MatchingPayload) > 0 {
				if err := json.Unmarshal(row.MatchingPayload, &c.Candidate); err != nil {
					return nil, funnel, err
				}
			}
			cs = append(cs, c)
			copies = append(copies, twin{noticeID: identity.Header.NoticeID, version: identity.Header.VersionID, source: identity.Source})
			funnel.Scanned++
		}
	} else if source, ok := s.notices.(catalogueSource); ok {
		before := int64(0)
		for {
			rows, err := source.CataloguePage(ctx, before, 100)
			if err != nil {
				return nil, funnel, err
			}
			for _, row := range rows {
				if err := ctx.Err(); err != nil {
					return nil, funnel, err
				}
				before = row.Sequence
				funnel.Scanned++
				c, kind, _ := read(row.Document)
				if kind != kindOpportunity {
					switch kind {
					case kindAward:
						funnel.Awards++
					case kindPlanning:
						funnel.Planning++
					case kindUnsupported:
						funnel.Unsupported++
					case kindUnreadable:
						funnel.Unreadable++
					}
					continue
				}
				c.Norm = normalize.Notice(c.Notice)
				cs = append(cs, c)
				copies = append(copies, twin{noticeID: c.Notice.NoticeID, version: c.Notice.VersionID, source: row.Document.Source})
			}
			if len(rows) < 100 {
				break
			}
		}
	} else {
		// Sources that cannot page are test/embedded adapters. No newest-N cap is used.
		err := s.notices.RecentNotices(ctx, int(^uint(0)>>1), func(doc rawstore.Document) error {
			if err := ctx.Err(); err != nil {
				return err
			}
			c, kind, _ := read(doc)
			funnel.Scanned++
			switch kind {
			case kindAward:
				funnel.Awards++
			case kindPlanning:
				funnel.Planning++
			case kindUnsupported:
				funnel.Unsupported++
			case kindUnreadable:
				funnel.Unreadable++
			}
			if kind == kindOpportunity {
				c.Norm = normalize.Notice(c.Notice)
				cs = append(cs, c)
				copies = append(copies, twin{noticeID: c.Notice.NoticeID, version: c.Notice.VersionID, source: doc.Source})
			}
			return nil
		})
		if err != nil {
			return nil, funnel, err
		}
	}
	keep := keepOnePerNotice(copies)
	dedup := make([]candidate, 0, len(cs))
	for i, c := range cs {
		if keep[i] {
			dedup = append(dedup, c)
		}
	}
	return dedup, funnel, nil
}
