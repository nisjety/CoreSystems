// Package mailtask implements the Ingestion Plane half of "forward an email
// to create a task" (chat-wave2 mail-task spec, owner ruling 2026-09-28:
// Postmark inbound, self-forward only): the inbound webhook receiver, sender
// verification, attachment extraction, and forwarding the normalized event
// on to the Frontend Plane gateway's ingest endpoint.
//
// Everything here is off by default -- Config.MailTaskProvider or
// Config.MailTaskWebhookSecret empty means Handler refuses every request
// without inspecting it, because the owner has not created a Postmark
// account yet (spec §7.1).
package mailtask

import "time"

// InboundMailTaskEvent is the common shape every provider parser normalizes
// into. Only Postmark is implemented for v1 (owner ruling), but the shape is
// kept provider-neutral so a future SendGrid/Mailgun parser (spec §3.2, out
// of scope for this ruling) slots in without changing anything downstream.
type InboundMailTaskEvent struct {
	Provider          string
	ProviderMessageID string
	// Alias is the local-part of the To: address this mail was sent to --
	// the opaque token user-core's mail-task alias resolves.
	Alias string
	// ToDomain is the domain part of the To: address, checked against
	// Config.MailTaskReceivingDomain as defense in depth (the provider's own
	// inbound routing is already domain-scoped; this is a second check, not
	// the only one).
	ToDomain    string
	FromEmail   string
	FromName    string
	AuthResult  string // "pass" | "fail" | "none", derived from SPF/DKIM
	Subject     string
	TextBody    string
	HTMLBody    string
	Attachments []InboundAttachment
}

// InboundAttachment is one attachment exactly as the provider delivered it --
// unfiltered by size or type. attachments.go applies the allow-list and size
// cap before anything is sent to imports-core.
type InboundAttachment struct {
	Filename    string
	ContentType string
	Content     []byte
}

// ExtractedAttachment is one attachment after attachments.go has tried to
// extract it. Extracted=false covers every reason (unsupported type, over
// size, extractor failure, empty result) -- the filename is always kept so
// the drafted turn can say "attachment not extracted: report.xlsx" instead of
// silently dropping it (spec §3.2, §6).
type ExtractedAttachment struct {
	Filename  string
	Extracted bool
	Text      string
}

// OccurredAt is a small time-source seam so tests can pin "now" without
// touching a real clock.
type Clock func() time.Time
