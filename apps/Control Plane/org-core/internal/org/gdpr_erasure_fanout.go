package org

import "time"

// GDPRErasureFanoutSubject is the cross-plane erasure fan-out contract.
// Model Plane (run history / conversations) and Data Plane (documents)
// subscribe to this subject to purge their own side of an erased
// organization. Both trigger paths — the explicit, immediate HTTP hard-delete
// (internal/http/gdpr_handlers.go) and the 30-day retention cron
// (PurgeDeletedOrganizations below) — publish this exact subject with an
// identical payload shape so subscribers never need to special-case which
// path triggered the erasure.
const GDPRErasureFanoutSubject = "verevon.gdpr.erasure.requested"

// gdprPurgeCronActorID is the requested_by value used when the retention
// cron (not a human/admin caller) triggers the erasure fan-out for an
// organization it purged automatically.
const gdprPurgeCronActorID = "system:org-core-purge-cron"

// PublishGDPRErasureFanout emits GDPRErasureFanoutSubject for orgID. It is
// the single implementation shared by both trigger paths:
//   - internal/http/gdpr_handlers.go's publishErasureFanout (explicit,
//     immediate HTTP hard-delete) delegates here;
//   - PurgeDeletedOrganizations (30-day retention cron) calls this directly,
//     once per organization the sweep actually purged.
//
// Before this method existed, only the explicit HTTP path published the
// fan-out — an org purged by the cron never told Model Plane / Data Plane to
// clean up their side. A nil shared publisher (verevon-nats disabled) makes
// this a no-op, matching every other SharedPub() call site in this package.
func (s *Service) PublishGDPRErasureFanout(orgID, actorID string) {
	sp := s.SharedPub()
	if sp == nil {
		return
	}
	sp.PublishPlain(GDPRErasureFanoutSubject, map[string]any{
		"subject_type": "organization",
		"subject_id":   orgID,
		"org_id":       orgID,
		"requested_by": actorID,
		"ts":           time.Now().UTC().Format(time.RFC3339Nano),
	})
}
