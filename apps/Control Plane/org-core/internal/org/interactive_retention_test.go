package org

import (
	"context"
	"strings"
	"testing"
)

func TestPlanAllowsZeroDataRetention(t *testing.T) {
	cases := []struct {
		plan string
		want bool
	}{
		{"pro", true},
		{"enterprise", true},
		{"Enterprise", true},   // case-insensitive
		{"  pro  ", true},      // trimmed
		{"free", false},
		{"trial", false},
		{"hobby", false},
		{"standard", false},
		{"", false},
		{"unknown", false},
	}
	for _, c := range cases {
		if got := PlanAllowsZeroDataRetention(c.plan); got != c.want {
			t.Errorf("PlanAllowsZeroDataRetention(%q) = %v, want %v", c.plan, got, c.want)
		}
	}
}

// TestFlushInteractiveRetentionOutboxValidatesLimitAndPublisher exercises the
// guard clauses reachable without a database: limit bounds, then a missing
// shared publisher, then a repository with no pool. See
// TestControlLifecycleInteractiveRetentionOutboxDrainsOnTransition (postgres,
// gated) for the full claim/publish/acknowledge cycle.
func TestFlushInteractiveRetentionOutboxValidatesLimitAndPublisher(t *testing.T) {
	service := NewService(&Repository{}, nil)
	if _, err := service.FlushInteractiveRetentionOutbox(context.Background(), 0); err == nil || !strings.Contains(err.Error(), "between 1 and 1000") {
		t.Fatalf("invalid limit error=%v", err)
	}
	if _, err := service.FlushInteractiveRetentionOutbox(context.Background(), 10); err == nil || !strings.Contains(err.Error(), "publisher") {
		t.Fatalf("missing publisher error=%v", err)
	}

	service.SetSharedPublisher(&sharedPlanChangeTestPublisher{})
	if _, err := service.FlushInteractiveRetentionOutbox(context.Background(), 10); err == nil || !strings.Contains(err.Error(), "repository") {
		t.Fatalf("repository-less flush error=%v", err)
	}
}

func TestSetInteractiveRetentionRequiresOrgID(t *testing.T) {
	service := NewService(&Repository{}, nil)
	if _, err := service.SetInteractiveRetention(context.Background(), "  ", true, "user-1"); err == nil || !strings.Contains(err.Error(), "organization id is required") {
		t.Fatalf("empty org id error=%v", err)
	}
}

func TestSetSupportAIModeValidatesInput(t *testing.T) {
	service := NewService(&Repository{}, nil)
	if _, err := service.SetSupportAIMode(context.Background(), "", "assist", "user-1"); err == nil || !strings.Contains(err.Error(), "organization id is required") {
		t.Fatalf("empty org id error=%v", err)
	}
	if _, err := service.SetSupportAIMode(context.Background(), "org-1", "bogus", "user-1"); err == nil || !strings.Contains(err.Error(), "invalid support AI mode") {
		t.Fatalf("invalid mode error=%v", err)
	}
}
