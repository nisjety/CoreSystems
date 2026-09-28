// Package mailtask backs the "forward an email to create a task" surface
// (chat-wave2 mail-task spec, owner ruling 2026-09-28: Postmark inbound,
// self-forward only). It owns exactly one thing on the Control Plane side: the
// per-user, high-entropy inbound-mail alias that maps 1:1 to a live
// (org_id, user_id) — the only address integration-corev2's inbound webhook
// receiver is ever allowed to resolve into a chat thread.
//
// Everything downstream of resolution (sender verification, attachment
// extraction, thread creation, the drafted turn, the notification) lives in
// other services; this package never sees an email's content.
package mailtask

import "time"

// Address is one alias row: an opaque token minted for a single (OrgID,
// UserID), live until RevokedAt is set. A revoked row is never reused or
// deleted — RevokeAlias always mints a fresh row instead — so a leaked or
// decommissioned alias can never start resolving again by accident.
type Address struct {
	Alias     string
	OrgID     string
	UserID    string
	CreatedAt time.Time
	RevokedAt *time.Time
}

// IsLive reports whether this address still resolves.
func (a Address) IsLive() bool {
	return a.RevokedAt == nil
}
