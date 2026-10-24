package browsersites

import "time"

// Decide answers for one canonical host. It is pure: the set, the caller's live
// choices and the instant are its only inputs, so the golden matrix
// (browser-site-policy-matrix.json) pins every row without a database.
//
//   - Block wins over every other input and is the only answer that cannot be
//     overridden by a member.
//   - The ceiling is the admin's read or act rule for the host. With no rule,
//     open mode allows act and allowlist mode allows nothing. Allowlist is any mode
//     other than open, so an unknown mode narrows access instead of opening it.
//   - A member's live choice applies within the ceiling. Choosing none is a refusal
//     kept until it expires. A choice of act over a read ceiling is read, decided
//     by the org cap.
//   - Without a live choice the answer is ask, and the member is consented to
//     before anything runs.
//
// A choice whose expires_at is not after now is treated as absent. Nothing here
// reads the clock: the caller passes now.
func Decide(set PolicySet, prefs map[string]Pref, host string, now time.Time) Decision {
	decision := Decision{Host: host, Effective: EffectiveDeny, Cap: CapNone, Source: SourceDefault}

	switch rule := ruleFor(set, host); rule {
	case RuleBlock:
		decision.Source = SourceOrgBlock
		return decision
	case RuleRead:
		decision.Cap = RuleRead
	case RuleAct:
		decision.Cap = RuleAct
	default:
		if set.Mode == ModeOpen {
			decision.Cap = RuleAct
		}
	}
	if decision.Cap == CapNone {
		decision.Source = SourceOrgCap
		return decision
	}

	pref, ok := prefs[host]
	if !ok || !pref.ExpiresAt.After(now) {
		decision.Effective = EffectiveAsk
		return decision
	}
	switch pref.Level {
	case LevelRead:
		decision.Effective = EffectiveRead
	case LevelAct:
		decision.Effective = EffectiveAct
		if decision.Cap == RuleRead {
			decision.Effective = EffectiveRead
			decision.Source = SourceOrgCap
			return decision
		}
	default:
		decision.Effective = EffectiveDeny
	}
	if decision.Source == SourceDefault {
		decision.Source = SourceUserPref
	}
	return decision
}

// SourceSpaceFloor names a refusal made because the Space a call is made for is not
// entitled to the agent browser (DEC-19 D9).
const SourceSpaceFloor = "space_floor"

// ApplySpaceFloor is the Space floor. A call made for a Space must clear that
// Space's browser_entitled answer before any host is reached: an unentitled Space is
// refused every host, whatever the admin allows or the member chose. The floor only
// narrows. An unscoped call and an entitled Space are returned unchanged, and a
// refusal already decided keeps its reason. A caller that cannot read the floor
// passes entitled=false, so an unknown floor refuses.
func ApplySpaceFloor(resolution Resolution, scoped, entitled bool) Resolution {
	if !scoped || entitled {
		return resolution
	}
	decisions := make([]Decision, 0, len(resolution.Decisions))
	for _, decision := range resolution.Decisions {
		if decision.Effective != EffectiveDeny {
			decision.Effective = EffectiveDeny
			decision.Source = SourceSpaceFloor
		}
		decisions = append(decisions, decision)
	}
	resolution.Decisions = decisions
	return resolution
}

// Resolve answers for each host in order. prefs is nil for a service call, which
// then gets the admin's answer alone and never a member's choice.
func Resolve(set PolicySet, prefs map[string]Pref, hosts []string, now time.Time) Resolution {
	resolution := Resolution{Revision: set.Revision, Decisions: make([]Decision, 0, len(hosts))}
	for _, host := range hosts {
		resolution.Decisions = append(resolution.Decisions, Decide(set, prefs, host, now))
	}
	return resolution
}

// ruleFor returns the admin's rule for host, or "" when there is none.
func ruleFor(set PolicySet, host string) string {
	for _, rule := range set.Rules {
		if rule.Host == host {
			return rule.Rule
		}
	}
	return ""
}
