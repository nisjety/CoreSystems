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
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/quality"
)

// Canonical rating vocabulary. model-gateway normalises every client
// vocabulary (a chat thumbs-up/down included) into exactly these three tokens
// before publishing, so nothing downstream needs to know about thumbs.
const (
	RatingGood       = "good"
	RatingAcceptable = "acceptable"
	RatingPoor       = "poor"
)

// Rating sources. A stated judgement and a behavioural inference are different
// kinds of evidence, and conflating them is how a regenerate ends up counting as
// a thumbs-down.
const (
	// SourceExplicit is an operator or chat user stating a judgement.
	SourceExplicit = "explicit"
	// SourceImplicit is inferred from behaviour — a regenerate, a re-ask, a
	// stated correction in the next message. Weighted far below explicit by
	// the quality policy, and never treated as truth.
	SourceImplicit = "implicit"
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
	// Source is [SourceExplicit] or [SourceImplicit]. An empty value normalises
	// to explicit, so every rating recorded before this field existed keeps its
	// full weight rather than being silently discounted.
	Source string
	// SignalKind is which detector fired for an implicit rating (regenerate,
	// near_duplicate, …). Empty for explicit ratings. Part of the store's key, so
	// two different kinds on one turn are two samples while the same kind twice
	// stays one.
	SignalKind string
	// SignalStrength is the detector's confidence for an implicit rating, 0..=1.
	// Ignored for explicit ratings, which are always a full sample. A missing or
	// out-of-range value normalises to 1.0 — an implicit signal that forgot to
	// say how sure it was is still discounted by the source weight, and guessing
	// lower would silently hide evidence.
	SignalStrength float64
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
	// Quarantine returns skills whose evidence has fallen below the quality
	// policy's demotion bar, worst-first.
	//
	// Separate from Candidates rather than one method returning both: promotion
	// and demotion have different callers, different blast-radius rules, and
	// different consequences for getting it wrong. Fusing them would make it
	// easy to apply a promotion cap to a demotion or vice versa.
	Quarantine(ctx context.Context) ([]QuarantineCandidate, error)
	// Durable reports whether ratings survive a restart. Used to log honestly
	// at startup instead of pretending an in-memory store is a store.
	Durable() bool
}

// QuarantineCandidate is a skill the policy says should stop being injected.
type QuarantineCandidate struct {
	OrgID   string
	SkillID string
	// Score is the full verdict, carried so an operator can see the evidence
	// behind the decision rather than just its outcome.
	Score quality.Score
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
	r.Source = strings.ToLower(strings.TrimSpace(r.Source))
	r.SignalKind = strings.ToLower(strings.TrimSpace(r.SignalKind))
	if r.Source == "" {
		r.Source = SourceExplicit
	}
	if r.Source != SourceExplicit && r.Source != SourceImplicit {
		return Rating{}, fmt.Errorf("feedback: unknown rating source %q", r.Source)
	}
	if r.Source == SourceExplicit {
		// An explicit rating has no detector, so it can carry no kind. Clearing
		// rather than rejecting: a caller that sets both is confused, not hostile,
		// and the kind is what would corrupt the key.
		r.SignalKind = ""
		r.SignalStrength = 1
	} else if r.SignalStrength <= 0 || r.SignalStrength > 1 {
		r.SignalStrength = 1
	}
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
	fromScope, toScope, rating, source string
	strength                           float64
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
		source:    clean.Source,
		strength:  clean.SignalStrength,
	}
	return nil
}

// groupKey identifies one aggregation bucket. Package-scoped because both the
// promotion and the demotion read model group the same way, and two copies would
// be two chances to group differently.
type groupKey struct {
	orgID, skillID, fromScope, toScope string
}

// aggregate accumulates one skill's evidence, keeping explicit and implicit
// apart so the quality policy — not this file — decides how to weigh them.
type aggregate struct {
	evidence quality.Evidence
}

// add folds one recorded rating in.
func (a *aggregate) add(rating string, source string, strength float64) {
	good, err := scoreWeight(rating)
	if err != nil {
		return
	}
	if source == SourceImplicit {
		// Implicit evidence is only ever negative: there is no behavioural
		// signal for satisfaction, and counting one would make silence a vote.
		a.evidence.ImplicitBadWeight += strength
		return
	}
	if good {
		a.evidence.ExplicitGood++
		return
	}
	a.evidence.ExplicitBad++
}

// Candidates aggregates skill-attached ratings. Run-only ratings (SkillID "")
// are recorded but are never promotion candidates — there is no skill to promote.
func (s *MemoryStore) Candidates(_ context.Context, minSamples int, threshold float64) ([]Candidate, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

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
		agg.add(value.rating, value.source, value.strength)
	}

	out := make([]Candidate, 0, len(groups))
	for gk, agg := range groups {
		score := quality.Evaluate(agg.evidence)
		// The caller's threshold still governs promotion — an operator's
		// configured bar is not this package's to override. What changed is WHAT
		// it is compared against: the Wilson lower bound rather than the raw
		// ratio, so a 1-of-1 skill can no longer present as perfect. The
		// consequence is deliberate and worth knowing: promotion now needs
		// volume as well as agreement, so a 9-of-10 skill is not promotable at
		// 0.8 until it has roughly thirty samples.
		if score.WeightedTotal < float64(minSamples) {
			continue
		}
		if score.LowerBound < threshold {
			continue
		}
		out = append(out, Candidate{
			OrgID:     gk.orgID,
			SkillID:   gk.skillID,
			FromScope: gk.fromScope,
			ToScope:   gk.toScope,
			Good:      agg.evidence.ExplicitGood,
			Total:     agg.evidence.ExplicitGood + agg.evidence.ExplicitBad,
			Score:     score.LowerBound,
		})
	}
	sortCandidates(out)
	return out, nil
}

// Quarantine applies the quality policy's demotion rule to every skill-attached
// group.
//
// Run-level rows (skill_id "") are skipped: quarantining is an action on a skill,
// and a run's rating with no matched skill names nothing to act on.
func (s *MemoryStore) Quarantine(_ context.Context) ([]QuarantineCandidate, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

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
		agg.add(value.rating, value.source, value.strength)
	}

	out := make([]QuarantineCandidate, 0)
	for gk, agg := range groups {
		score := quality.Evaluate(agg.evidence)
		if quality.Decide(quality.StateActive, score) != quality.DecisionQuarantine {
			continue
		}
		out = append(out, QuarantineCandidate{
			OrgID:   gk.orgID,
			SkillID: gk.skillID,
			Score:   score,
		})
	}
	sortQuarantine(out)
	return out, nil
}

// sortQuarantine orders worst-first, ties by org/skill, so the sweep cap always
// stops the most clearly broken skills and two runs agree.
func sortQuarantine(out []QuarantineCandidate) {
	sort.Slice(out, func(i, j int) bool {
		if out[i].Score.LowerBound != out[j].Score.LowerBound {
			return out[i].Score.LowerBound < out[j].Score.LowerBound
		}
		if out[i].OrgID != out[j].OrgID {
			return out[i].OrgID < out[j].OrgID
		}
		return out[i].SkillID < out[j].SkillID
	})
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
