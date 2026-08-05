package org

import (
	"context"
	"log"
)

// ListOrgsNeeding7DayReminder returns organizations pending deletion whose
// 30-day deadline is 7 (or fewer) days out and have not yet had their 7-day
// reminder sent. Thin pass-through to the repository so cmd/server/main.go's
// reminder sweep only talks to *Service, matching every other cron in this
// package.
func (s *Service) ListOrgsNeeding7DayReminder(ctx context.Context) ([]OrgPendingDeletionReminder, error) {
	return s.repo.ListOrgsNeeding7DayReminder(ctx)
}

// ListOrgsNeeding1DayReminder is ListOrgsNeeding7DayReminder's 1-day-out
// counterpart.
func (s *Service) ListOrgsNeeding1DayReminder(ctx context.Context) ([]OrgPendingDeletionReminder, error) {
	return s.repo.ListOrgsNeeding1DayReminder(ctx)
}

// MarkReminderSent stamps the organization's deletion_reminder_<which>_sent_at
// column so the sweep never re-fires the same reminder. which must be "7d" or
// "1d". This is what makes the reminder sweep idempotent under at-least-once
// ticker execution: a sweep that runs twice for the same org only publishes
// twice if the first MarkReminderSent call itself failed.
func (s *Service) MarkReminderSent(ctx context.Context, orgID, which string) error {
	return s.repo.MarkReminderSent(ctx, orgID, which)
}

// PublishDeletionReminder emits verevon.org.deletion.reminder for orgID. The
// member cohort reminded is exactly the org_deletion_members ledger recorded
// at soft-delete time (not a fresh membership lookup) — the same members who
// received the original pending notice, even if org membership has changed
// since. A nil shared publisher (verevon-nats disabled) makes this a no-op.
func (s *Service) PublishDeletionReminder(ctx context.Context, orgID, orgName string, daysRemaining int) {
	var memberIDs []string
	if entries, err := s.repo.ListDeletionLedger(ctx, orgID); err != nil {
		log.Printf("org-core: deletion reminder: list ledger for org=%s failed: %v", orgID, err)
	} else {
		memberIDs = make([]string, 0, len(entries))
		for _, e := range entries {
			memberIDs = append(memberIDs, e.UserID)
		}
	}

	sp := s.SharedPub()
	if sp == nil {
		return
	}
	sp.PublishPlain("verevon.org.deletion.reminder", map[string]any{
		"org_id":          orgID,
		"org_name":        orgName,
		"days_remaining":  daysRemaining,
		"member_user_ids": memberIDs,
	})
}
