package store

import (
	"context"
	"fmt"
	"time"

	"github.com/google/uuid"
)

// RecommendationDraft is a pre-filled proposal suggestion. It is NEVER
// persisted by the recommender — the operator reviews it and, if they agree,
// POSTs it to /proposals (which is the human gate). Think of it as "the system
// noticed X; here's the proposal it would write if you approve."
type RecommendationDraft struct {
	Kind           string      `json:"kind"`   // 'delete' | 'archive'
	Reason         string      `json:"reason"`
	ItemPKs        []uuid.UUID `json:"item_pks"`
	EstimatedBytes int64       `json:"estimated_bytes"`
	// KeepItemPK is set on duplicate-delete drafts: the copy the recommender
	// suggests keeping (newest). Informational only.
	KeepItemPK *uuid.UUID `json:"keep_item_pk,omitempty"`
}

type Recommendations struct {
	analytics *Analytics
}

// Duplicates turns duplicate groups into delete drafts: keep the newest copy,
// propose deleting the rest. EstimatedBytes is the space reclaimed (sum of the
// deleted copies' sizes).
func (r *Recommendations) Duplicates(ctx context.Context, organizationID string, minSizeBytes int64, maxGroups int) ([]RecommendationDraft, error) {
	groups, err := r.analytics.Duplicates(ctx, organizationID, 2, minSizeBytes, maxGroups)
	if err != nil {
		return nil, fmt.Errorf("recommend duplicates: %w", err)
	}

	var drafts []RecommendationDraft
	for _, g := range groups {
		if len(g.Members) < 2 {
			continue
		}
		keepIdx := newestMemberIndex(g.Members)
		keep := g.Members[keepIdx]

		var (
			toDelete []uuid.UUID
			reclaim  int64
		)
		for i, m := range g.Members {
			if i == keepIdx {
				continue
			}
			toDelete = append(toDelete, m.ItemPK)
			reclaim += m.SizeBytes
		}
		if len(toDelete) == 0 {
			continue
		}
		keepPK := keep.ItemPK
		drafts = append(drafts, RecommendationDraft{
			Kind:           ProposalKindDelete,
			Reason:         fmt.Sprintf("duplicate of %s (keeping newest copy)", keep.Path),
			ItemPKs:        toDelete,
			EstimatedBytes: reclaim,
			KeepItemPK:     &keepPK,
		})
	}
	return drafts, nil
}

// Inactive turns stale files into a single archive draft. Bundling them into
// one draft keeps the operator's review surface small; they can split it
// before approving if they want finer granularity.
func (r *Recommendations) Inactive(ctx context.Context, organizationID string, olderThan time.Duration, limit int) (RecommendationDraft, error) {
	items, err := r.analytics.Inactive(ctx, organizationID, olderThan, limit)
	if err != nil {
		return RecommendationDraft{}, fmt.Errorf("recommend inactive: %w", err)
	}
	if len(items) == 0 {
		return RecommendationDraft{}, ErrNotFound
	}

	var (
		pks   []uuid.UUID
		bytes int64
	)
	for _, it := range items {
		pks = append(pks, it.ItemPK)
		bytes += it.SizeBytes
	}

	days := int64(olderThan.Hours() / 24)
	return RecommendationDraft{
		Kind:           ProposalKindArchive,
		Reason:         fmt.Sprintf("inactive for more than %d days (%d files)", days, len(items)),
		ItemPKs:        pks,
		EstimatedBytes: bytes,
	}, nil
}

// newestMemberIndex returns the index of the member with the most recent
// ModifiedAt. A nil ModifiedAt is treated as the oldest possible time so a
// member with a known timestamp always wins over one without.
func newestMemberIndex(members []DuplicateMember) int {
	best := 0
	var bestTime time.Time
	if members[0].ModifiedAt != nil {
		bestTime = *members[0].ModifiedAt
	}
	for i := 1; i < len(members); i++ {
		var t time.Time
		if members[i].ModifiedAt != nil {
			t = *members[i].ModifiedAt
		}
		if t.After(bestTime) {
			best = i
			bestTime = t
		}
	}
	return best
}
