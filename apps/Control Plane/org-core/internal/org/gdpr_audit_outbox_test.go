package org

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

type gdprAuditPublisherStub struct {
	err      error
	eventIDs []string
	subjects []string
	payloads []map[string]any
}

func (p *gdprAuditPublisherStub) PublishAudit(_ context.Context, subject, eventID string, payload map[string]any) error {
	p.eventIDs = append(p.eventIDs, eventID)
	p.subjects = append(p.subjects, subject)
	copied := make(map[string]any, len(payload))
	for key, value := range payload {
		copied[key] = value
	}
	p.payloads = append(p.payloads, copied)
	return p.err
}

func TestGDPRAuditEventIdentityIsStableAndPayloadIsCanonical(t *testing.T) {
	occurredAt := time.Date(2026, 7, 15, 10, 11, 12, 123, time.UTC)
	event, err := newGDPRAuditEvent(
		"org-stable", "organization", "org-stable", "user-owner", "owner", "ok", occurredAt,
	)
	if err != nil {
		t.Fatal(err)
	}
	retry, err := newGDPRAuditEvent(
		"org-stable", "organization", "org-stable", "user-owner", "owner", "ok", occurredAt,
	)
	if err != nil {
		t.Fatal(err)
	}
	if event.EventID != retry.EventID || !strings.HasPrefix(event.EventID, "gdpr:org-core:") {
		t.Fatalf("event IDs are not stable: %q / %q", event.EventID, retry.EventID)
	}
	if event.Subject != GDPRErasureAuditSubject {
		t.Fatalf("subject=%q", event.Subject)
	}
	payload, err := event.Payload()
	if err != nil {
		t.Fatal(err)
	}
	for key, want := range map[string]any{
		"event_id": event.EventID, "org_id": "org-stable", "user_id": "user-owner",
		"actor_role": "owner", "plane": "control", "producer": "org-core",
		"event": "erasure", "subject": "organization:org-stable",
		"resource_id": "org-stable", "outcome": "ok",
	} {
		if got := payload[key]; got != want {
			t.Fatalf("payload[%q]=%v; want %v", key, got, want)
		}
	}
}

func TestGDPRAuditEventAndEncodingBoundariesFailClosed(t *testing.T) {
	now := time.Now().UTC()
	tests := []struct {
		name        string
		orgID       string
		subjectType string
		subjectID   string
		actorID     string
		actorRole   string
		outcome     string
		occurredAt  time.Time
	}{
		{name: "missing org", subjectType: "organization", subjectID: "org", actorID: "user", actorRole: "owner", outcome: "ok", occurredAt: now},
		{name: "missing subject", orgID: "org", subjectType: "organization", actorID: "user", actorRole: "owner", outcome: "ok", occurredAt: now},
		{name: "missing actor", orgID: "org", subjectType: "organization", subjectID: "org", actorRole: "owner", outcome: "ok", occurredAt: now},
		{name: "missing role", orgID: "org", subjectType: "organization", subjectID: "org", actorID: "user", outcome: "ok", occurredAt: now},
		{name: "unsupported subject type", orgID: "org", subjectType: "user", subjectID: "org", actorID: "user", actorRole: "owner", outcome: "ok", occurredAt: now},
		{name: "unsupported outcome", orgID: "org", subjectType: "organization", subjectID: "org", actorID: "user", actorRole: "owner", outcome: "unknown", occurredAt: now},
		{name: "missing occurrence", orgID: "org", subjectType: "organization", subjectID: "org", actorID: "user", actorRole: "owner", outcome: "ok"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := newGDPRAuditEvent(
				tt.orgID, tt.subjectType, tt.subjectID, tt.actorID, tt.actorRole,
				tt.outcome, tt.occurredAt,
			); err == nil {
				t.Fatal("invalid GDPR audit event was accepted")
			}
		})
	}

	if _, err := (GDPRAuditEvent{}).Payload(); err == nil {
		t.Fatal("empty GDPR audit payload was accepted")
	}
	if _, err := (GDPRAuditEvent{
		EventID: strings.Repeat("x", 129), Subject: GDPRErasureAuditSubject,
	}).Payload(); err == nil {
		t.Fatal("oversized GDPR audit event ID was accepted")
	}
	if err := enqueueGDPRAuditEvent(context.Background(), nil, GDPRAuditEvent{}); err == nil {
		t.Fatal("invalid GDPR audit event was enqueued")
	}
	event, err := newGDPRAuditEvent("org", "organization", "org", "user", "owner", "ok", now)
	if err != nil {
		t.Fatal(err)
	}
	invalidPayloads := []GDPRAuditEvent{
		func() GDPRAuditEvent { candidate := event; candidate.OrgID = ""; return candidate }(),
		func() GDPRAuditEvent { candidate := event; candidate.UserID = ""; return candidate }(),
		func() GDPRAuditEvent { candidate := event; candidate.ActorRole = ""; return candidate }(),
		func() GDPRAuditEvent { candidate := event; candidate.SubjectType = "user"; return candidate }(),
		func() GDPRAuditEvent { candidate := event; candidate.Outcome = "unknown"; return candidate }(),
		func() GDPRAuditEvent { candidate := event; candidate.OccurredAt = time.Time{}; return candidate }(),
	}
	for index, candidate := range invalidPayloads {
		if _, err := candidate.Payload(); err == nil {
			t.Fatalf("invalid direct payload %d was accepted", index)
		}
	}
	if err := enqueueGDPRAuditEvent(context.Background(), nil, event); err == nil || !strings.Contains(err.Error(), "store") {
		t.Fatalf("missing GDPR audit store error=%v", err)
	}
	event.Details = map[string]any{"unencodable": func() {}}
	if err := enqueueGDPRAuditEvent(context.Background(), nil, event); err == nil || !strings.Contains(err.Error(), "encode") {
		t.Fatalf("unencodable GDPR audit payload error=%v", err)
	}
}

func TestFlushGDPRAuditOutboxValidatesLimitAndPublisher(t *testing.T) {
	service := NewService(&Repository{}, nil)
	if _, err := service.FlushGDPRAuditOutbox(context.Background(), 0); err == nil || !strings.Contains(err.Error(), "between 1 and 1000") {
		t.Fatalf("invalid limit error=%v", err)
	}
	if _, err := service.FlushGDPRAuditOutbox(context.Background(), 10); err == nil || !strings.Contains(err.Error(), "publisher") {
		t.Fatalf("missing publisher error=%v", err)
	}

	publisher := &gdprAuditPublisherStub{err: errors.New("fixture")}
	service.SetAuditPublisher(publisher)
	if _, err := service.FlushGDPRAuditOutbox(context.Background(), 10); err == nil {
		t.Fatal("repository-less flush unexpectedly succeeded")
	}
	if _, err := service.GDPRAuditOutboxStatus(context.Background()); err == nil || !strings.Contains(err.Error(), "repository") {
		t.Fatalf("repository-less status error=%v", err)
	}
}
