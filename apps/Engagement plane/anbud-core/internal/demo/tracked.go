package demo

import (
	"context"
	"errors"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
	"time"
)

func (s *Service) trackedResults(ctx context.Context, r Request, p match.Profile, now time.Time) ([]Result, error) {
	results := []Result{}
	if s.tracking == nil || r.Org == "" {
		return results, nil
	}
	ctx, cancel := context.WithTimeout(ctx, s.scanTimeout)
	defer cancel()
	ids, err := s.tracking.TrackedNoticeIDs(ctx, r.Org)
	if err != nil {
		return nil, err
	}
	ordered := []string{}
	byID := map[string]candidate{}
	bases := map[string]CatalogueNotice{}
	candidates := []candidate{}
	seen := map[string]bool{}
	for _, id := range ids {
		if seen[id] {
			continue
		}
		seen[id] = true
		ordered = append(ordered, id)
		doc, err := s.notices.Notice(ctx, id)
		if errors.Is(err, rawstore.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, err
		}
		base, c := catalogueNotice(doc, now)
		byID[id] = c
		bases[id] = base
		candidates = append(candidates, c)
	}
	var evidence map[string]match.Evidence
	if !r.TrackedOnly {
		evidence, _ = s.documents(ctx, r.Org, candidates)
	}
	for _, id := range ordered {
		c, held := byID[id]
		if !held {
			results = append(results, Result{NoticeID: id, Title: id, Verdict: "unknown", Band: "none", CPV: []string{}, NUTS: []string{}, BuyerOrgNumbers: []string{}, Gate: []CheckView{}, Fit: []CheckView{}, Retained: true, RetentionReason: "source_unavailable"})
			continue
		}
		base := bases[id]
		a := match.Assessment{Verdict: match.Unknown, Band: match.None}
		if !r.TrackedOnly && base.MetadataAvailable {
			a = match.AssessWith(c.Notice, c.Norm, p, r.Strictness, now, evidence[id])
		}
		result := newResult(id, c, a, evidence[id], now)
		if result.Deadline == nil {
			result.Deadline = base.Deadline
		}
		if !base.MetadataAvailable {
			result = base.Result
		}
		result.Retained = true
		result.RetentionReason = "tracked_stage"
		results = append(results, result)
	}
	return results, nil
}
