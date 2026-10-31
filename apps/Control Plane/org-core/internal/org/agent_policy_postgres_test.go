package org

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

func TestNormalizeAgentLearningPolicy(t *testing.T) {
	cases := []struct {
		name string
		raw  string
		want AgentLearningPolicy
	}{
		{"absent is the defaults", "", AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}},
		{"null is the defaults", "null", AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}},
		{"empty object is the defaults", "{}", AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}},
		{"stored policy", `{"memory":"disabled","learningReview":"proposals","revision":4,"updatedAt":"2026-10-24T09:00:00Z","updatedBy":"u1"}`,
			AgentLearningPolicy{Memory: "disabled", LearningReview: "proposals", Revision: 4, UpdatedAt: "2026-10-24T09:00:00Z"}},
		{"unknown memory value reads as disabled", `{"memory":"yes"}`, AgentLearningPolicy{Memory: "disabled", LearningReview: "disabled"}},
		{"non-string memory reads as disabled", `{"memory":true}`, AgentLearningPolicy{Memory: "disabled", LearningReview: "disabled"}},
		{"unknown learning value reads as disabled", `{"learningReview":"auto"}`, AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}},
		{"damaged object reads as memory disabled", `"nonsense"`, AgentLearningPolicy{Memory: "disabled", LearningReview: "disabled"}},
		{"negative revision is ignored", `{"revision":-2}`, AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if got := normalizeAgentLearningPolicy([]byte(test.raw)); got != test.want {
				t.Fatalf("policy=%+v want=%+v", got, test.want)
			}
		})
	}
}

// The service refuses before it touches the repository, so a zero repository
// proves nothing was written for a value outside the documented spellings.
func TestSetAgentLearningPolicyRefusesEverythingButTheDocumentedValues(t *testing.T) {
	service := NewService(&Repository{}, nil)
	wrongCase, unknown, allow := "Disabled", "auto", "allow"
	cases := map[string][2]*string{
		"no field":                   {nil, nil},
		"wrong case memory":          {&wrongCase, nil},
		"unknown learning mode":      {nil, &unknown},
		"memory in the learning row": {nil, &allow},
	}
	for name, args := range cases {
		if _, err := service.SetAgentLearningPolicy(context.Background(), "org-1", args[0], args[1], "u1"); !errors.Is(err, ErrInvalidAgentLearningPolicy) {
			t.Fatalf("%s: err=%v want ErrInvalidAgentLearningPolicy", name, err)
		}
	}
}

// TestControlLifecycleAgentLearningPolicyRoundTripsThroughPostgres exercises
// the jsonb_set writer and the reader against a real Postgres: defaults when
// nothing is stored, a one-row change keeps the other row and bumps the
// revision, a malformed stored value reads restrictive, and a soft-deleted or
// unknown org is not found. Gated like the other lifecycle tests.
func TestControlLifecycleAgentLearningPolicyRoundTripsThroughPostgres(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	service := NewService(repo, nil)
	orgID := "org-agent-policy-roundtrip"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Agent Policy Roundtrip", Slug: "agent-policy-roundtrip", Plan: "enterprise", Status: "active",
	}, "owner-agent-policy-roundtrip"); err != nil {
		t.Fatalf("seed organization: %v", err)
	}

	read := func(label string) AgentLearningPolicy {
		t.Helper()
		got, err := service.GetAgentLearningPolicy(ctx, orgID)
		if err != nil {
			t.Fatalf("%s: read policy: %v", label, err)
		}
		return *got
	}

	if got := read("nothing stored"); got.Memory != "allow" || got.LearningReview != "disabled" || got.Revision != 0 {
		t.Fatalf("defaults: %+v", got)
	}

	disabled, proposals := "disabled", "proposals"
	if _, err := service.SetAgentLearningPolicy(ctx, orgID, &disabled, nil, "admin-1"); err != nil {
		t.Fatalf("disable memory: %v", err)
	}
	if got := read("memory disabled"); got.Memory != "disabled" || got.LearningReview != "disabled" || got.Revision != 1 || got.UpdatedAt == "" {
		t.Fatalf("after disabling memory: %+v", got)
	}
	if _, err := service.SetAgentLearningPolicy(ctx, orgID, nil, &proposals, "admin-1"); err != nil {
		t.Fatalf("enable review: %v", err)
	}
	if got := read("review enabled"); got.Memory != "disabled" || got.LearningReview != "proposals" || got.Revision != 2 {
		t.Fatalf("a one-row change must keep the other row: %+v", got)
	}

	if _, err := db.Pool.Exec(ctx, `
UPDATE organizations
SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), '{agentLearning}', '{"memory":"maybe","learningReview":"auto"}'::jsonb, true)
WHERE id = $1`, orgID); err != nil {
		t.Fatalf("write malformed policy: %v", err)
	}
	if got := read("malformed"); got.Memory != "disabled" || got.LearningReview != "disabled" {
		t.Fatalf("a malformed policy must read restrictive: %+v", got)
	}

	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET deleted_at = NOW() WHERE id = $1`, orgID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}
	if _, err := service.GetAgentLearningPolicy(ctx, orgID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("soft-deleted org error=%v want ErrNotFound", err)
	}
	if _, err := service.SetAgentLearningPolicy(ctx, orgID, &disabled, nil, "admin-1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("write to a soft-deleted org error=%v want ErrNotFound", err)
	}
	if _, err := service.GetAgentLearningPolicy(ctx, "org-agent-policy-missing"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown org error=%v want ErrNotFound", err)
	}
}
