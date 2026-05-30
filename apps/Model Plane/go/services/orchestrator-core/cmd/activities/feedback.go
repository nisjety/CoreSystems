package activities

import (
	"sort"
	"sync"
)

// FeedbackStore accumulates operator ratings per skill-promotion candidate.
//
// It is the input signal for the feedback → skill-promotion loop
// (HARNESS_PHASE1 §6): a NATS subscriber records each `mp.v1.feedback.rated`
// envelope here, and the nightly FeedbackPromotionWorkflow's
// AggregateFeedbackActivity reads it to decide which skills have earned
// promotion. In-memory + bounded by the number of distinct skills an org uses;
// durable history lives in the rating store (Convex agentRuns).
type FeedbackStore struct {
	mu    sync.Mutex
	stats map[string]*skillStat
}

type skillStat struct {
	skillID   string
	fromScope string
	toScope   string
	good      int
	total     int
}

// SkillPromotionCandidate is a skill whose feedback crossed the promotion bar.
type SkillPromotionCandidate struct {
	SkillID   string
	FromScope string
	ToScope   string
	Good      int
	Total     int
	Score     float64
}

// NewFeedbackStore creates an empty store.
func NewFeedbackStore() *FeedbackStore {
	return &FeedbackStore{stats: make(map[string]*skillStat)}
}

func feedbackKey(skillID, fromScope, toScope string) string {
	return skillID + "|" + fromScope + "|" + toScope
}

// Record folds one rating into the per-skill counters. `rating` is the operator
// label ("good" | "acceptable" | "poor"); only "good" counts toward the
// promotion score, but every rating counts toward the sample total.
func (s *FeedbackStore) Record(skillID, fromScope, toScope, rating string) {
	if skillID == "" {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	key := feedbackKey(skillID, fromScope, toScope)
	st, ok := s.stats[key]
	if !ok {
		st = &skillStat{skillID: skillID, fromScope: fromScope, toScope: toScope}
		s.stats[key] = st
	}
	st.total++
	if rating == "good" {
		st.good++
	}
}

// Candidates returns skills with at least `minSamples` ratings whose good-ratio
// meets `threshold`, sorted by score descending (deterministic for tests).
func (s *FeedbackStore) Candidates(minSamples int, threshold float64) []SkillPromotionCandidate {
	s.mu.Lock()
	defer s.mu.Unlock()

	var out []SkillPromotionCandidate
	for _, st := range s.stats {
		if st.total < minSamples {
			continue
		}
		score := float64(st.good) / float64(st.total)
		if score < threshold {
			continue
		}
		out = append(out, SkillPromotionCandidate{
			SkillID:   st.skillID,
			FromScope: st.fromScope,
			ToScope:   st.toScope,
			Good:      st.good,
			Total:     st.total,
			Score:     score,
		})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Score != out[j].Score {
			return out[i].Score > out[j].Score
		}
		return out[i].SkillID < out[j].SkillID
	})
	return out
}
