// Package natsx provides NATS subject constants, helpers, and compatibility
// adapters for the Model Plane v1 subject namespace.
package natsx

import "fmt"

// New Model Plane v1 subject constants.
const (
	Prefix = "mp.v1"

	RunEventsWildcard      = "mp.v1.run.*.event"
	SessionCommandWildcard = "mp.v1.session.*.command"
	IngressWildcard        = "mp.v1.ingress.*"
)

// RunEventSubject returns the subject for a specific run's events.
func RunEventSubject(runID string) string {
	return fmt.Sprintf("%s.run.%s.event", Prefix, runID)
}

// SessionCommandSubject returns the subject for a session's commands.
func SessionCommandSubject(sessionKey string) string {
	return fmt.Sprintf("%s.session.%s.command", Prefix, sessionKey)
}

// IngressSubject returns the subject for an ingress event kind.
func IngressSubject(kind string) string {
	return fmt.Sprintf("%s.ingress.%s", Prefix, kind)
}

// EventSubject returns a subject for a given event type and resource.
func EventSubject(eventType, resourceID string) string {
	return fmt.Sprintf("%s.%s.%s", Prefix, eventType, resourceID)
}

// Legacy (v2) subject constants and builders for dual-write compatibility.
const (
	LegacyPrefix                 = "velion"
	LegacyRunEventsWildcard      = "velion.agent.run.*.event"
	LegacySessionCommandWildcard = "velion.session.*.command"
	LegacyAqenciaWildcard        = "aqencia.reasoning.>"
)

// LegacyRunEventSubject returns the legacy v2 subject for a run's events.
func LegacyRunEventSubject(runID string) string {
	return fmt.Sprintf("velion.agent.run.%s.event", runID)
}

// LegacySessionCommandSubject returns the legacy v2 subject for a session's commands.
func LegacySessionCommandSubject(sessionKey string) string {
	return fmt.Sprintf("velion.session.%s.command", sessionKey)
}

// New Model Plane v1 usage and stream subjects.
const (
	UsageWildcard  = "mp.v1.usage.*"
	StreamWildcard = "mp.v1.stream.*"
)

// UsageSubject returns the subject for a specific organization's usage events.
func UsageSubject(orgID string) string {
	return fmt.Sprintf("%s.usage.%s", Prefix, orgID)
}

// StreamSubject returns the subject for a specific stream kind.
func StreamSubject(kind string) string {
	return fmt.Sprintf("%s.stream.%s", Prefix, kind)
}

// Legacy Aqencia reasoning subject constants for dual-write compatibility.
const (
	LegacyAqenciaReasoningStarted   = "aqencia.reasoning.reasoning.started"
	LegacyAqenciaReasoningCompleted = "aqencia.reasoning.reasoning.completed"
	LegacyAqenciaUsageRecorded      = "aqencia.reasoning.usage.recorded"
	LegacyAqenciaDecisionMade       = "aqencia.reasoning.decision.made"
	LegacyAqenciaQuotaExceeded      = "aqencia.reasoning.quota.exceeded"
)
