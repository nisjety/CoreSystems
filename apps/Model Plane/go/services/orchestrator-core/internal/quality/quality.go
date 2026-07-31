// Package quality decides whether a skill has earned promotion or lost trust.
//
// It exists because a signal nothing reads is worthless, and a signal read
// naively is worse than worthless: it silently teaches the system that good work
// is bad. This package is the one place that judgement is made, so the policy can
// be reviewed as a policy rather than inferred from a SQL predicate.
//
// # Why a Wilson lower bound and not a ratio
//
// The promotion path used `good / total` with a minimum-sample floor. That is two
// problems in one expression. A raw ratio treats 1-of-1 as a perfect score, so the
// floor exists only to paper over it — and any floor is arbitrary: at 5 samples a
// 5-of-5 skill and a 50-of-50 skill both score 1.0, though one is barely evidence.
//
// The Wilson score interval's LOWER bound answers the question actually being
// asked: given what we have observed, what is the worst plausible true quality at
// 95% confidence? It rises with agreement AND with volume, so it needs no separate
// sample floor and cannot be gamed by a single rating. This is the standard
// approach for ranking under sparse binary feedback.
//
// # Why implicit evidence is weighted far below explicit
//
// An explicit rating is someone stating a judgement. An implicit signal is an
// inference from behaviour, and every such inference has an innocent reading: a
// regenerate can mean "give me another style", a re-asked question can mean the
// user thought of a better phrasing. So implicit evidence contributes a fraction
// of a sample rather than a whole one ([ImplicitWeight]), scaled again by how
// confident the detector was. A stated correction still lands well below a
// deliberate thumbs-down.
//
// # Why demotion quarantines and never retires
//
// A quarantined skill stops being injected but keeps its identity, its history and
// its ability to recover. Deleting on a signal is unrecoverable and, given weak
// labels, will eventually delete something good. Retirement stays a human
// decision. The sweep is also bounded ([MaxQuarantinesPerSweep]) so a bad deploy
// or one frustrated user cannot empty the catalogue between two cron ticks.
package quality

import "math"

// Wilson95Z is the z-score for a 95% confidence interval.
//
// 95% rather than 99%: at 99% the bound is so conservative that a genuinely good
// skill needs a large sample before it can be promoted, which makes the loop feel
// broken. 95% is the conventional choice for this kind of ranking.
const Wilson95Z = 1.96

// ImplicitWeight is how much one implicit signal counts against an explicit
// rating's 1.0.
//
// 0.25 means four implicit negatives are needed to equal one deliberate
// thumbs-down, and that is before the detector's own confidence scales it down
// further. The number is deliberately conservative: the cost of over-trusting
// behavioural inference is teaching the loop that correct answers are wrong, and
// that failure is silent.
const ImplicitWeight = 0.25

// PromoteBound is the Wilson lower bound a skill must reach to be promoted.
const PromoteBound = 0.70

// DemoteBound is the Wilson lower bound below which a skill is quarantined.
//
// The gap between this and [RecoverBound] is hysteresis, and it is the difference
// between a working control loop and one that flaps: with a single threshold, a
// skill sitting near the line would be quarantined and un-quarantined on
// alternating sweeps, and every flip would change what users are served.
const DemoteBound = 0.45

// RecoverBound is the Wilson lower bound a quarantined skill must reach to become
// active again — higher than [DemoteBound] on purpose.
const RecoverBound = 0.65

// MinEvidenceForDemotion is the minimum WEIGHTED sample count before a skill may
// be quarantined at all.
//
// Weighted, so implicit-only evidence must accumulate a lot of it: at
// [ImplicitWeight] 0.25 and a regenerate's 0.3 confidence, one regenerate is
// 0.075 of a sample, so roughly 40 of them are needed to clear this bar alone.
// Someone repeatedly clicking regenerate for variety therefore cannot demote
// anything, while a handful of real thumbs-down can.
const MinEvidenceForDemotion = 3.0

// MaxQuarantinesPerSweep bounds how many skills one sweep may quarantine.
//
// A cap rather than trust: the worst realistic case is a bad deploy making every
// answer poor at once, and the correct response to that is a handful of
// quarantines and a loud log, not an emptied catalogue. Skipped candidates are
// reported so the truncation is never silent.
const MaxQuarantinesPerSweep = 5

// State is a skill's lifecycle position.
type State string

const (
	// StateActive means the skill is injected normally.
	StateActive State = "active"
	// StateQuarantined means the skill is not injected but is fully retained and
	// can recover. It is NEVER a terminal state.
	StateQuarantined State = "quarantined"
	// StateRetired is terminal and is only ever set by a human. This package
	// never returns it.
	StateRetired State = "retired"
)

// Evidence is what has been observed about one skill in one org.
//
// Explicit and implicit counts are kept apart rather than pre-summed so the
// weighting decision lives in this package and a caller cannot accidentally
// launder an implicit signal into an explicit one.
type Evidence struct {
	// ExplicitGood / ExplicitBad are deliberate operator ratings. `acceptable`
	// counts as bad for the promotion bar (it is "not good"), matching the
	// existing scoreWeight contract.
	ExplicitGood int
	ExplicitBad  int
	// ImplicitBadWeight is the SUM of detector strengths for negative implicit
	// signals — already scaled by each signal's own confidence, not yet by
	// [ImplicitWeight].
	ImplicitBadWeight float64
	// ImplicitGoodWeight exists for symmetry and is normally 0: there is no
	// reliable behavioural signal for satisfaction. Accepting silence as
	// approval would make every unanswered turn a positive vote.
	ImplicitGoodWeight float64
}

// WeightedGood is the numerator: explicit positives plus discounted implicit ones.
func (e Evidence) WeightedGood() float64 {
	return float64(e.ExplicitGood) + e.ImplicitGoodWeight*ImplicitWeight
}

// WeightedTotal is the denominator over the same weighting.
func (e Evidence) WeightedTotal() float64 {
	return float64(e.ExplicitGood+e.ExplicitBad) +
		(e.ImplicitGoodWeight+e.ImplicitBadWeight)*ImplicitWeight
}

// HasExplicit reports whether a human ever stated a judgement about this skill.
func (e Evidence) HasExplicit() bool { return e.ExplicitGood+e.ExplicitBad > 0 }

// WilsonLowerBound is the lower bound of the Wilson score interval for
// `good` successes out of `total` trials at the given z.
//
// Accepts fractional counts so weighted evidence flows through the same maths.
// Returns 0 for a non-positive total: no observations means no defensible claim
// about quality, and 0 is the value that neither promotes nor demotes.
func WilsonLowerBound(good, total, z float64) float64 {
	if total <= 0 || good < 0 {
		return 0
	}
	if good > total {
		good = total
	}
	phat := good / total
	z2 := z * z
	denominator := 1 + z2/total
	center := phat + z2/(2*total)
	margin := z * math.Sqrt((phat*(1-phat)+z2/(4*total))/total)
	lower := (center - margin) / denominator
	return math.Max(0, math.Min(1, lower))
}

// Score is the quality verdict for one skill.
type Score struct {
	// LowerBound is the Wilson 95% lower bound over weighted evidence.
	LowerBound float64
	// WeightedGood / WeightedTotal are what the bound was computed from,
	// reported so an operator can see why a decision was made.
	WeightedGood  float64
	WeightedTotal float64
	// HasExplicit is whether any human judgement is present.
	HasExplicit bool
}

// Evaluate scores evidence.
func Evaluate(e Evidence) Score {
	good := e.WeightedGood()
	total := e.WeightedTotal()
	return Score{
		LowerBound:    WilsonLowerBound(good, total, Wilson95Z),
		WeightedGood:  good,
		WeightedTotal: total,
		HasExplicit:   e.HasExplicit(),
	}
}

// Decision is what the sweep should do with a skill.
type Decision string

const (
	// DecisionHold means leave the skill where it is.
	DecisionHold Decision = "hold"
	// DecisionPromote means the skill has earned a wider scope.
	DecisionPromote Decision = "promote"
	// DecisionQuarantine means stop injecting it, reversibly.
	DecisionQuarantine Decision = "quarantine"
	// DecisionRecover means return a quarantined skill to active.
	DecisionRecover Decision = "recover"
)

// Decide applies the policy to one skill's current state and score.
//
// The order of the checks is the policy:
//   - A quarantined skill is only ever considered for recovery. It cannot be
//     promoted straight out of quarantine, because the evidence that redeemed it
//     is by definition thin.
//   - A retired skill is untouched. Retirement is a human decision and this
//     package must not undo one.
//   - Demotion additionally requires enough weighted evidence, so a single bad
//     turn cannot quarantine anything however low the bound.
func Decide(state State, score Score) Decision {
	switch state {
	case StateRetired:
		return DecisionHold
	case StateQuarantined:
		if score.LowerBound >= RecoverBound {
			return DecisionRecover
		}
		return DecisionHold
	case StateActive:
		if score.WeightedTotal >= MinEvidenceForDemotion && score.LowerBound < DemoteBound {
			return DecisionQuarantine
		}
		if score.LowerBound >= PromoteBound {
			return DecisionPromote
		}
		return DecisionHold
	default:
		// An unknown state is not something to act on. Failing closed here means
		// a future state cannot be silently treated as active.
		return DecisionHold
	}
}

// Candidate pairs a skill with the decision reached about it.
type Candidate struct {
	OrgID   string
	SkillID string
	State   State
	Score   Score
	Action  Decision
}

// PlanSweep applies [MaxQuarantinesPerSweep] to a set of decisions.
//
// Returns the candidates to act on and how many quarantines were withheld. The
// withheld count is returned rather than logged here so the caller can surface it
// — a cap that truncates silently reads as "nothing more was wrong".
//
// Quarantines are taken worst-first, so if the cap bites, the most clearly broken
// skills are the ones actually stopped.
func PlanSweep(candidates []Candidate) (actionable []Candidate, withheldQuarantines int) {
	quarantines := make([]Candidate, 0, len(candidates))
	others := make([]Candidate, 0, len(candidates))
	for _, candidate := range candidates {
		switch candidate.Action {
		case DecisionQuarantine:
			quarantines = append(quarantines, candidate)
		case DecisionHold:
			// Nothing to do; drop it rather than carry a no-op through the sweep.
		default:
			others = append(others, candidate)
		}
	}
	// Worst bound first; ties broken by id so a sweep is deterministic and two
	// runs over the same data act on the same skills.
	sortByBoundAscending(quarantines)

	if len(quarantines) > MaxQuarantinesPerSweep {
		withheldQuarantines = len(quarantines) - MaxQuarantinesPerSweep
		quarantines = quarantines[:MaxQuarantinesPerSweep]
	}
	actionable = append(actionable, quarantines...)
	actionable = append(actionable, others...)
	return actionable, withheldQuarantines
}

func sortByBoundAscending(items []Candidate) {
	for i := 1; i < len(items); i++ {
		for j := i; j > 0; j-- {
			prev, cur := items[j-1], items[j]
			if prev.Score.LowerBound < cur.Score.LowerBound {
				break
			}
			if prev.Score.LowerBound == cur.Score.LowerBound &&
				prev.OrgID+prev.SkillID <= cur.OrgID+cur.SkillID {
				break
			}
			items[j-1], items[j] = items[j], items[j-1]
		}
	}
}
