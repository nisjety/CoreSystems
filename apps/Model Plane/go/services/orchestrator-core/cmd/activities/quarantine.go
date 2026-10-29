package activities

import (
	"context"
	"errors"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/quality"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// QuarantineSweepOutput reports what one demotion sweep did.
//
// Withheld is carried out of the activity rather than only logged: the sweep cap
// exists so a bad deploy cannot empty the catalogue, and a cap that truncates
// silently reads as "nothing more was wrong".
type QuarantineSweepOutput struct {
	// Evaluated is how many skills the policy judged.
	Evaluated int
	// Quarantined are the skill ids actually stopped, worst-first.
	Quarantined []string
	// Failed are skills the policy chose but the registry refused. Reported
	// rather than retried: a sweep that half-applied is information an operator
	// needs, and re-running the whole sweep to catch one failure would re-apply
	// the others.
	Failed []string
	// Withheld is how many further quarantines the per-sweep cap held back.
	Withheld int
	// Summary is human-readable and says which of the above happened.
	Summary string
}

// QuarantineSweepActivity applies the quality policy's demotion decisions.
//
// Quarantine means "stop injecting this", never "delete it": the skill keeps its
// identity and history and can recover once its evidence does. That matters
// because the evidence driving this is partly behavioural inference, and an
// unrecoverable action on a weak label will eventually be wrong about something
// good.
//
// Applied through session-core's `SetAgentSkillEnabled`, which flips only the
// injection switch — `UpsertAgentSkill` is keyed by (org, name) and rewrites the
// whole row, so using it here would blank the content of the skill it was meant
// to pause and make quarantine unrecoverable.
//
// One consequence to know: `enabled` cannot currently distinguish a policy
// quarantine from a human disabling a skill. Recovery is therefore NOT automated
// — re-enabling automatically could undo a deliberate human decision. A distinct
// `quality_state` column is the fix, and until it exists recovery stays manual.
func (a *Activities) QuarantineSweepActivity(ctx context.Context) (QuarantineSweepOutput, error) {
	if a.feedbackStore == nil {
		return QuarantineSweepOutput{}, errors.New("feedback store not configured")
	}
	candidates, err := a.feedbackStore.Quarantine(ctx)
	if err != nil {
		return QuarantineSweepOutput{}, err
	}
	if len(candidates) == 0 {
		return QuarantineSweepOutput{Summary: "no skills below the quality bar"}, nil
	}

	planned := make([]quality.Candidate, 0, len(candidates))
	for _, candidate := range candidates {
		planned = append(planned, quality.Candidate{
			OrgID:   candidate.OrgID,
			SkillID: candidate.SkillID,
			State:   quality.StateActive,
			Score:   candidate.Score,
			Action:  quality.DecisionQuarantine,
		})
	}
	actionable, withheld := quality.PlanSweep(planned)

	out := QuarantineSweepOutput{Evaluated: len(candidates), Withheld: withheld}
	if a.clients == nil || a.clients.SessionCore == nil {
		out.Summary = "session-core unavailable; nothing was quarantined"
		return out, status.Error(codes.Unavailable, out.Summary)
	}
	client := mpv1.NewSessionCoreClient(a.clients.SessionCore)

	for _, candidate := range actionable {
		resp, uerr := client.SetAgentSkillEnabled(ctx, &mpv1.SetAgentSkillEnabledRequest{
			OrgId:   candidate.OrgID,
			SkillId: candidate.SkillID,
			Enabled: false,
			Reason: "quality policy: Wilson lower bound " +
				formatBound(candidate.Score.LowerBound) + " below the demotion bar",
		})
		if uerr == nil && !resp.GetUpdated() {
			// The skill vanished between the sweep's read and this write. A race
			// to report, not a failure to retry.
			a.logger.Info("quarantine target no longer exists",
				"skill_id", candidate.SkillID, "org_id", candidate.OrgID)
			continue
		}
		if uerr != nil {
			a.logger.Error("quarantine failed",
				"skill_id", candidate.SkillID, "org_id", candidate.OrgID,
				"lower_bound", candidate.Score.LowerBound, "err", uerr)
			out.Failed = append(out.Failed, candidate.SkillID)
			continue
		}
		a.logger.Warn("skill quarantined by the quality policy",
			"skill_id", candidate.SkillID, "org_id", candidate.OrgID,
			"lower_bound", candidate.Score.LowerBound,
			"weighted_samples", candidate.Score.WeightedTotal,
			"had_explicit_ratings", candidate.Score.HasExplicit)
		out.Quarantined = append(out.Quarantined, candidate.SkillID)
	}

	out.Summary = summarizeSweep(out)
	if withheld > 0 {
		// Loud on purpose. Hitting the cap means more skills were below the bar
		// than one sweep may stop, which is the signature of a systemic problem
		// rather than a few bad skills.
		a.logger.Warn("quarantine sweep hit its per-sweep cap",
			"withheld", withheld, "cap", quality.MaxQuarantinesPerSweep)
	}
	return out, nil
}

func summarizeSweep(out QuarantineSweepOutput) string {
	summary := "evaluated " + itoa(out.Evaluated) + " skill(s); quarantined " +
		itoa(len(out.Quarantined))
	if len(out.Failed) > 0 {
		summary += "; " + itoa(len(out.Failed)) + " refused by the registry"
	}
	if out.Withheld > 0 {
		summary += "; " + itoa(out.Withheld) + " held back by the per-sweep cap"
	}
	return summary
}

// formatBound renders a 0..1 bound to two decimals without pulling in fmt for
// one string.
func formatBound(value float64) string {
	hundredths := int(value*100 + 0.5)
	if hundredths < 0 {
		hundredths = 0
	}
	if hundredths > 100 {
		hundredths = 100
	}
	whole := hundredths / 100
	frac := hundredths % 100
	tens := frac / 10
	return itoa(whole) + "." + itoa(tens) + itoa(frac%10)
}

// itoa avoids pulling strconv in for one call site.
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	negative := n < 0
	if negative {
		n = -n
	}
	var digits []byte
	for n > 0 {
		digits = append([]byte{byte('0' + n%10)}, digits...)
		n /= 10
	}
	if negative {
		return "-" + string(digits)
	}
	return string(digits)
}
