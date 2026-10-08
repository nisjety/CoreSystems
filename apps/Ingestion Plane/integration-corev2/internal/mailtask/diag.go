package mailtask

import (
	"fmt"
	"sort"
	"strings"
	"sync"
)

// This file makes a silent mass-drop visible without ever touching mail
// content. The receiver answers 200 to a message it drops for sender
// verification (spec §3.2: never leak why to the sender), so the HTTP layer
// cannot tell "one forgery rejected" from "every real message rejected because
// Postmark's headers are not what the rule expects". Two content-free signals
// close that gap:
//
//   - a log line on every sender_not_verified drop (Result.Diagnostic): the
//     failing check, the derived verdict, the deciding rule, a closed-vocabulary
//     detail, and the header NAMES the payload carried;
//   - counters (Handler.RenderMetrics) of every inbound outcome and every
//     sender check, labelled only with closed vocabularies, so a drop RATE can
//     be graphed and alerted on.
//
// Nothing here ever records a header value, a subject, a body, an attachment
// name or an address. Header names are attacker-influenced strings, so they
// are restricted to a conservative alphabet and bounded in length and count.

// Derived auth verdicts. AuthResult carries one of these.
const (
	authVerdictPass = "pass"
	authVerdictFail = "fail"
	authVerdictNone = "none"
)

// Deciding rules (closed vocabulary).
const (
	authRuleSpamTests   = "x-spam-tests"
	authRuleAuthResults = "authentication-results"
	authRuleNone        = "none"
)

// Rule outcomes (closed vocabulary).
const (
	authDetailOK              = "ok"
	authDetailDuplicateHeader = "duplicate_header"
	authDetailSPFNotPass      = "spf_not_pass"
	authDetailDKIMNotAligned  = "dkim_not_aligned"
	authDetailSPFDKIMNotPass  = "spf_and_dkim_not_pass"
	authDetailSPFOrDKIMFailed = "spf_or_dkim_not_pass"
	authDetailNoSignalTokens  = "no_spf_or_dkim_tokens"
	authDetailNoUsableHeader  = "no_usable_header"
	authDetailUnspecified     = "unspecified"
)

// Sender-check causes (closed vocabulary), in the order VerifySender checks.
const (
	senderCauseAuthNotPass     = "auth_not_pass"
	senderCauseNoVerifiedEmail = "no_verified_email"
	senderCauseFromMismatch    = "from_mismatch"
)

// Sender-check outcomes. A verified sender has no failing check, so its cause
// label is senderCauseNone.
const (
	senderOutcomeVerified = "verified"
	senderOutcomeDropped  = "dropped"
	senderCauseNone       = "none"
)

const (
	maxDiagHeaderNames      = 64
	maxDiagHeaderNameLength = 48
)

// AuthDiagnostic explains how a message's AuthResult was derived. Every field
// is either a constant from the closed vocabularies above or a sanitised header
// NAME; no field can carry mail content.
type AuthDiagnostic struct {
	// Verdict is the derived verdict: "pass", "fail" or "none".
	Verdict string
	// Rule is the header rule that decided: "x-spam-tests",
	// "authentication-results", or "none" when no rule found a usable header.
	Rule string
	// Detail is the closed-vocabulary outcome of that rule.
	Detail string
	// HeaderNames are the lower-cased names of the headers the payload carried
	// (deduplicated, sorted, restricted to [a-z0-9-], bounded). Values are
	// never recorded.
	HeaderNames []string
}

// sanitizedHeaderNames reduces the set of header names seen in a payload to the
// bounded, loggable form AuthDiagnostic carries. A name outside the conservative
// alphabet (for example one that embeds an address or whitespace) is dropped
// rather than logged.
func sanitizedHeaderNames(counts map[string]int) []string {
	names := make([]string, 0, len(counts))
	for name := range counts {
		if name == "" || len(name) > maxDiagHeaderNameLength {
			continue
		}
		valid := true
		for i := 0; i < len(name); i++ {
			c := name[i]
			if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
				valid = false
				break
			}
		}
		if valid {
			names = append(names, name)
		}
	}
	sort.Strings(names)
	if len(names) > maxDiagHeaderNames {
		names = names[:maxDiagHeaderNames]
	}
	return names
}

// logLine renders the one content-free line logged when a message is dropped
// for sender verification. cause is one of the senderCause* constants.
func (d AuthDiagnostic) logLine(cause string) string {
	return fmt.Sprintf("cause=%s verdict=%s rule=%s detail=%s header_count=%d headers=%s",
		diagToken(cause), diagToken(d.Verdict), diagToken(d.Rule), diagToken(d.Detail),
		len(d.HeaderNames), strings.Join(d.HeaderNames, ","))
}

// diagToken keeps an unexpected empty value from producing an ambiguous log or
// metric field.
func diagToken(value string) string {
	if value == "" {
		return authDetailUnspecified
	}
	return value
}

// mailStats holds the content-free counters. The zero value is ready to use and
// safe for concurrent use. Its cardinality is bounded by the closed
// vocabularies: Reason, and the verdict/rule/detail/cause/outcome constants.
type mailStats struct {
	mu           sync.Mutex
	inbound      map[Reason]uint64
	senderChecks map[senderCheckKey]uint64
}

type senderCheckKey struct {
	outcome, cause, verdict, rule, detail string
}

func (s *mailStats) recordInbound(reason Reason) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.inbound == nil {
		s.inbound = make(map[Reason]uint64)
	}
	s.inbound[reason]++
}

func (s *mailStats) recordSenderCheck(outcome, cause string, diag AuthDiagnostic) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.senderChecks == nil {
		s.senderChecks = make(map[senderCheckKey]uint64)
	}
	s.senderChecks[senderCheckKey{
		outcome: diagToken(outcome),
		cause:   diagToken(cause),
		verdict: diagToken(diag.Verdict),
		rule:    diagToken(diag.Rule),
		detail:  diagToken(diag.Detail),
	}]++
}

// recordInbound counts one terminal outcome of HandleInboundPostmark. Safe on a
// nil Handler.
func (h *Handler) recordInbound(reason Reason) {
	if h == nil {
		return
	}
	h.stats.recordInbound(reason)
}

// RenderMetrics returns the mail-task counters in Prometheus text format, for
// the service's /metrics endpoint. It is empty until the first inbound request.
//
//	integration_mailtask_inbound_total{reason}
//	    every outcome of the inbound webhook, by closed Reason. The ratio of
//	    sender_not_verified to accepted is the mass-drop signal.
//	integration_mailtask_sender_check_total{outcome,cause,verdict,rule,detail}
//	    every sender-verification decision, with the derived auth verdict, the
//	    rule that produced it and why. A rising verdict="none" detail=
//	    "no_usable_header" means Postmark is not sending the header the rule
//	    reads.
func (h *Handler) RenderMetrics() string {
	if h == nil {
		return ""
	}
	return h.stats.render()
}

func (s *mailStats) render() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.inbound) == 0 && len(s.senderChecks) == 0 {
		return ""
	}
	var out strings.Builder

	out.WriteString("# HELP integration_mailtask_inbound_total Inbound mail-task webhook outcomes by closed reason (content-free).\n")
	out.WriteString("# TYPE integration_mailtask_inbound_total counter\n")
	byReason := make(map[string]uint64, len(s.inbound))
	for reason, count := range s.inbound {
		byReason[metricLabelValue(string(reason))] += count
	}
	reasons := make([]string, 0, len(byReason))
	for label := range byReason {
		reasons = append(reasons, label)
	}
	sort.Strings(reasons)
	for _, label := range reasons {
		fmt.Fprintf(&out, "integration_mailtask_inbound_total{reason=\"%s\"} %d\n", label, byReason[label])
	}

	out.WriteString("# HELP integration_mailtask_sender_check_total Mail-task sender verification decisions by outcome, failing check, derived auth verdict, deciding header rule and rule detail (content-free).\n")
	out.WriteString("# TYPE integration_mailtask_sender_check_total counter\n")
	lines := make([]string, 0, len(s.senderChecks))
	for key, count := range s.senderChecks {
		lines = append(lines, fmt.Sprintf(
			"integration_mailtask_sender_check_total{outcome=\"%s\",cause=\"%s\",verdict=\"%s\",rule=\"%s\",detail=\"%s\"} %d\n",
			metricLabelValue(key.outcome), metricLabelValue(key.cause), metricLabelValue(key.verdict),
			metricLabelValue(key.rule), metricLabelValue(key.detail), count))
	}
	sort.Strings(lines)
	for _, line := range lines {
		out.WriteString(line)
	}
	return out.String()
}

// metricLabelValue is belt and braces: every label value is already drawn from
// a closed vocabulary, but anything outside [a-z0-9_-] is flattened so a future
// vocabulary slip can never inject a quote, backslash or newline into the
// exposition format.
func metricLabelValue(value string) string {
	if value == "" {
		return authDetailUnspecified
	}
	var b strings.Builder
	b.Grow(len(value))
	for i := 0; i < len(value); i++ {
		c := value[i]
		if c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-' {
			b.WriteByte(c)
		} else {
			b.WriteByte('_')
		}
	}
	return b.String()
}
