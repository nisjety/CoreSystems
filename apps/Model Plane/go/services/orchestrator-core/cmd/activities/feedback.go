package activities

import (
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/feedback"
)

// SkillPromotionCandidate is a skill whose feedback crossed the promotion bar.
//
// The counters behind it now live in a durable, Postgres-backed store
// (internal/feedback) rather than the in-memory map they used to (which was
// wiped on every restart — AGENT_QUALITY_PLAN_2026-07-29 §1.2).
type SkillPromotionCandidate struct {
	// OrgID is the tenant whose ratings produced this candidate. Ratings are
	// never pooled across tenants, so a candidate always names one.
	OrgID     string
	SkillID   string
	FromScope string
	ToScope   string
	Good      int
	Total     int
	Score     float64
}

// promotionCandidates adapts the store's read model to the workflow's input type.
func promotionCandidates(in []feedback.Candidate) []SkillPromotionCandidate {
	if len(in) == 0 {
		return nil
	}
	out := make([]SkillPromotionCandidate, 0, len(in))
	for _, c := range in {
		out = append(out, SkillPromotionCandidate{
			OrgID:     c.OrgID,
			SkillID:   c.SkillID,
			FromScope: c.FromScope,
			ToScope:   c.ToScope,
			Good:      c.Good,
			Total:     c.Total,
			Score:     c.Score,
		})
	}
	return out
}
