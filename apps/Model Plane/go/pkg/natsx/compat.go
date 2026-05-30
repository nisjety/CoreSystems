// Package natsx — compat.go provides legacy-to-new subject translation.
//
// Legacy subjects (from Model Plane v2):
//
//	velion.agent.run.{run_id}.event       -> mp.v1.run.{run_id}.event
//	velion.session.{id}.command           -> mp.v1.session.{id}.command
//	aqencia.reasoning.reasoning.started   -> mp.v1.run.{id}.event (RUN_STARTED)
//	aqencia.reasoning.reasoning.completed -> mp.v1.run.{id}.event (RUN_COMPLETED)
//	aqencia.reasoning.usage.recorded      -> mp.v1.ingress.usage
//	aqencia.reasoning.decision.made       -> mp.v1.ingress.decision
//	aqencia.reasoning.quota.exceeded      -> mp.v1.ingress.quota_exceeded
package natsx

import "strings"

// LegacyMapping describes how a legacy subject maps to a new subject.
type LegacyMapping struct {
	LegacyPattern string
	NewSubjectFn  func(legacySubject string) string
	EventTypeMap  string // new event type if applicable
}

// LegacyMappings is the complete mapping table from v2 to new subjects.
var LegacyMappings = []LegacyMapping{
	{
		LegacyPattern: "velion.agent.run.*.event",
		NewSubjectFn: func(s string) string {
			// velion.agent.run.<run_id>.event -> mp.v1.run.<run_id>.event
			parts := strings.Split(s, ".")
			if len(parts) >= 5 {
				return RunEventSubject(parts[3])
			}
			return s
		},
	},
	{
		LegacyPattern: "velion.session.*.command",
		NewSubjectFn: func(s string) string {
			// velion.session.<id>.command -> mp.v1.session.<id>.command
			parts := strings.Split(s, ".")
			if len(parts) >= 4 {
				return SessionCommandSubject(parts[2])
			}
			return s
		},
	},
	{
		LegacyPattern: "aqencia.reasoning.reasoning.started",
		NewSubjectFn: func(_ string) string {
			return IngressSubject("run_started_compat")
		},
		EventTypeMap: "RUN_STARTED",
	},
	{
		LegacyPattern: "aqencia.reasoning.reasoning.completed",
		NewSubjectFn: func(_ string) string {
			return IngressSubject("run_completed_compat")
		},
		EventTypeMap: "RUN_COMPLETED",
	},
	{
		LegacyPattern: "aqencia.reasoning.usage.recorded",
		NewSubjectFn: func(_ string) string {
			return IngressSubject("usage")
		},
	},
	{
		LegacyPattern: "aqencia.reasoning.decision.made",
		NewSubjectFn: func(_ string) string {
			return IngressSubject("decision")
		},
	},
	{
		LegacyPattern: "aqencia.reasoning.quota.exceeded",
		NewSubjectFn: func(_ string) string {
			return IngressSubject("quota_exceeded")
		},
	},
}

// TranslateLegacySubject translates a legacy subject to the new mp.v1 namespace.
// Returns the original subject unchanged if no mapping matches.
func TranslateLegacySubject(legacySubject string) string {
	for _, m := range LegacyMappings {
		if matchesPattern(legacySubject, m.LegacyPattern) {
			return m.NewSubjectFn(legacySubject)
		}
	}
	return legacySubject
}

// TranslateNewToLegacy maps an mp.v1.* subject back to its legacy v2 equivalent.
// Returns "" if no unambiguous reverse mapping exists (e.g., aqencia family is lossy).
func TranslateNewToLegacy(v1Subject string) string {
	parts := strings.Split(v1Subject, ".")
	if matchesPattern(v1Subject, RunEventsWildcard) && len(parts) >= 4 {
		return LegacyRunEventSubject(parts[3])
	}
	if matchesPattern(v1Subject, SessionCommandWildcard) && len(parts) >= 4 {
		return LegacySessionCommandSubject(parts[3])
	}
	switch v1Subject {
	case IngressSubject("usage"):
		return LegacyAqenciaUsageRecorded
	case IngressSubject("decision"):
		return LegacyAqenciaDecisionMade
	case IngressSubject("quota_exceeded"):
		return LegacyAqenciaQuotaExceeded
	}
	return ""
}

// matchesPattern checks if a subject matches a NATS-style wildcard pattern.
// Supports `*` (single token) and trailing `>` (one or more tokens).
func matchesPattern(subject, pattern string) bool {
	subParts := strings.Split(subject, ".")
	patParts := strings.Split(pattern, ".")

	if len(patParts) > 0 && patParts[len(patParts)-1] == ">" {
		if len(subParts) < len(patParts) {
			return false
		}
		for i := 0; i < len(patParts)-1; i++ {
			if patParts[i] == "*" {
				continue
			}
			if patParts[i] != subParts[i] {
				return false
			}
		}
		return true
	}

	if len(subParts) != len(patParts) {
		return false
	}

	for i, pat := range patParts {
		if pat == "*" {
			continue
		}
		if pat != subParts[i] {
			return false
		}
	}
	return true
}
