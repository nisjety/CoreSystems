// Package feedback holds the operator-rating signal that drives the feedback →
// skill-promotion loop (AGENT_QUALITY_PLAN_2026-07-29 §1.2).
//
// A rating arrives on `mp.v1.feedback.rated` and attaches to two things:
//
//   - the **run** — answer quality for that turn (SkillID == "");
//   - the **skills injected into that turn** — so a thumbs-down demotes the
//     skill that steered a bad answer.
//
// Ratings must survive a restart, so the production implementation is
// Postgres-backed ([PostgresStore]). [MemoryStore] exists only for tests and for
// a deployment with no DATABASE_URL configured, and says so out loud.
package feedback

import (
	"context"
	"errors"
	"sort"
	"strings"
	"sync"
	"time"
)

// Canonical rating vocabulary. model-gateway normalises every client
// vocabulary (a chat thumbs-up/down included) into exactly these three tokens
// before publishing, so nothing downstream needs to know about thumbs.
const (
	RatingGood       = "good"
	RatingAcceptable = "acceptable"
	RatingPoor       = "poor"
)

// ErrUnknownRating is returned for a rating outside the canonical vocabulary.
// Deliberately an error and not a silent skip-or-default: the bug this package
// was rewritten to kill was a non-canonical rating ("positive") counting toward
// the sample total while matching no "good" branch — which made a thumbs-UP
// *lower* the skill's promotion score.
var ErrUnknownRating = errors.New("feedback: rating outside the canonical vocabulary")

// scoreWeight is the ONE place a canonical rating becomes counters.
//
// Returns (good, err): every canonical rating counts toward the sample total,
// and only RatingGood counts toward the numerator. A rating that is not
// canonical is rejected rather than counted, because counting it would move the
// denominator without ever being able to move the numerator — i.e. it would
// silently penalise the skill.
func scoreWeight(rating string) (good bool, err error) {
	switch strings.ToLower(strings.TrimSpace(rating)) {
	case RatingGood:
		return true, nil
	case RatingAcceptable, RatingPoor:
		return false, nil
	default:
		return false, ErrUnknownRating
	}
}

// Rating is one recorded operator rating.
type Rating struct {
	// OrgID is the tenant. Aggregation never pools ratings across tenants.
	OrgID string
	// UserID is the rater. A user re-rating the same target REPLACES their
	// earlier rating rather than adding a second sample.
	UserID string
	// RunID is the durable run the rating scores. Required.
	RunID string
	// SkillID is the injected skill this rating attaches to. Empty = the
	// run-level rating, which is recorded but is not a promotion candidate.
	SkillID   string
	FromScope string
	ToScope   string
	// Rating must be one of the canonical tokens.
	Rating string
	// Note is optional free text from the rating UI.
	Note      string
	CreatedAt time.Time
}

// Candidate is a skill whose feedback crossed the promotion bar.
type Candidate struct {
	OrgID     string
	SkillID   string
	FromScope string
	ToScope   string
	Good      int
	Total     int
	Score     float64
}

// Store is the durable rating sink + promotion-candidate read model.
type Store interface {
	// Record persists one rating. Re-rating the same (org, run, user, skill)
	// replaces the previous value.
	Record(ctx context.Context, r Rating) error
	// Candidates returns skill-attached ratings with at least minSamples
	// distinct raters whose good-ratio meets threshold, score descending.
	Candidates(ctx context.Context, minSamples int, threshold float64) ([]Candidate, error)
	// Durable reports whether ratings survive a restart. Used to log honestly
	// at startup instead of pretending an in-memory store is a store.
	Durable() bool
}

// Normalize validates and fills in a rating, returning the value to persist.
func Normalize(r Rating) (Rating, error) {
	r.OrgID = strings.TrimSpace(r.OrgID)
	r.UserID = strings.TrimSpace(r.UserID)
	r.RunID = strings.TrimSpace(r.RunID)
	r.SkillID = strings.TrimSpace(r.SkillID)
	r.FromScope = strings.TrimSpace(r.FromScope)
	r.ToScope = strings.TrimSpace(r.ToScope)
	r.Rating = strings.ToLower(strings.TrimSpace(r.Rating))
	if r.OrgID == "" {
		return Rating{}, errors.New("feedback: org_id is required")
	}
	if r.RunID == "" {
		return Rating{}, errors.New("feedback: run_id is required")
	}
	if _, err := scoreWeight(r.Rating); err != nil {
		return Rating{}, err
	}
	if r.CreatedAt.IsZero() {
		r.CreatedAt = time.Now().UTC()
	} else {
		r.CreatedAt = r.CreatedAt.UTC()
	}
	return r, nil
}

// ── In-memory store (tests / no DATABASE_URL) ────────────────────────────────

type memoryKey struct {
	orgID, runID, userID, skillID string
}

type memoryValue struct {
	fromScope, toScope, rating string
}

// MemoryStore is a non-durable Store. Ratings are lost on restart, so it must
// never be the production sink — [Store.Durable] returns false so startup can
// say so.
type MemoryStore struct {
	mu      sync.Mutex
	ratings map[memoryKey]memoryValue
}

// NewMemoryStore creates an empty in-memory store.
func NewMemoryStore() *MemoryStore {
	return &MemoryStore{ratings: make(map[memoryKey]memoryValue)}
}

// Durable reports false: this store does not survive a restart.
func (s *MemoryStore) Durable() bool { return false }

// Record folds one rating in, replacing this rater's previous value for the
// same target.
func (s *MemoryStore) Record(_ context.Context, r Rating) error {
	clean, err := Normalize(r)
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.ratings[memoryKey{clean.OrgID, clean.RunID, clean.UserID, clean.SkillID}] = memoryValue{
		fromScope: clean.FromScope,
		toScope:   clean.ToScope,
		rating:    clean.Rating,
	}
	return nil
}

type aggregate struct {
	good, total int
}

// Candidates aggregates skill-attached ratings. Run-only ratings (SkillID "")
// are recorded but are never promotion candidates — there is no skill to promote.
func (s *MemoryStore) Candidates(_ context.Context, minSamples int, threshold float64) ([]Candidate, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	type groupKey struct {
		orgID, skillID, fromScope, toScope string
	}
	groups := make(map[groupKey]*aggregate)
	for key, value := range s.ratings {
		if key.skillID == "" {
			continue
		}
		gk := groupKey{key.orgID, key.skillID, value.fromScope, value.toScope}
		agg, ok := groups[gk]
		if !ok {
			agg = &aggregate{}
			groups[gk] = agg
		}
		agg.total++
		if good, err := scoreWeight(value.rating); err == nil && good {
			agg.good++
		}
	}

	out := make([]Candidate, 0, len(groups))
	for gk, agg := range groups {
		if agg.total < minSamples {
			continue
		}
		score := float64(agg.good) / float64(agg.total)
		if score < threshold {
			continue
		}
		out = append(out, Candidate{
			OrgID:     gk.orgID,
			SkillID:   gk.skillID,
			FromScope: gk.fromScope,
			ToScope:   gk.toScope,
			Good:      agg.good,
			Total:     agg.total,
			Score:     score,
		})
	}
	sortCandidates(out)
	return out, nil
}

// sortCandidates orders by score descending, then org/skill for determinism.
func sortCandidates(out []Candidate) {
	sort.Slice(out, func(i, j int) bool {
		if out[i].Score != out[j].Score {
			return out[i].Score > out[j].Score
		}
		if out[i].OrgID != out[j].OrgID {
			return out[i].OrgID < out[j].OrgID
		}
		return out[i].SkillID < out[j].SkillID
	})
}

var _ Store = (*MemoryStore)(nil)
