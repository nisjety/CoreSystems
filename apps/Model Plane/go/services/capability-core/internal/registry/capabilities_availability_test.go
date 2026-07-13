package registry

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

type availabilityDatabase struct {
	tag   pgconn.CommandTag
	err   error
	query string
	args  []any
}

func (database *availabilityDatabase) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	database.query = query
	database.args = append([]any(nil), args...)
	return database.tag, database.err
}

func (*availabilityDatabase) Query(context.Context, string, ...any) (pgx.Rows, error) {
	panic("unexpected Query")
}

func (*availabilityDatabase) QueryRow(context.Context, string, ...any) pgx.Row {
	panic("unexpected QueryRow")
}

func TestSetAvailabilityForOrgPinsTenantAndReportsAtomicOutcome(t *testing.T) {
	t.Parallel()

	checkedAt := time.Date(2026, time.July, 13, 15, 0, 0, 0, time.FixedZone("offset", 2*60*60))
	database := &availabilityDatabase{tag: pgconn.NewCommandTag("UPDATE 1")}
	store := &CapabilitiesStore{pool: database}

	updated, err := store.AttestAvailabilityForOrg(context.Background(), "cap.read", "org-a", "service:health", AvailabilityUpdate{
		ExpectedVersion: "1",
		State:           "available",
		ReasonCode:      "runtime_healthy",
		Reason:          "probe succeeded",
		ExecutionMode:   "direct_read",
		CostClass:       "bounded",
		HealthCheckedAt: checkedAt,
	})
	if err != nil || !updated {
		t.Fatalf("updated = %v, err = %v", updated, err)
	}
	if len(database.args) != 12 || database.args[6] != "cap.read" || database.args[7] != "org-a" || database.args[8] != "1" || database.args[10] != "service:health" {
		t.Fatalf("query args do not pin capability and tenant: %#v", database.args)
	}
	if !strings.Contains(database.query, "INSERT INTO registry_audit_log") || !strings.Contains(database.query, "WITH updated AS") {
		t.Fatalf("availability and audit must be one statement: %s", database.query)
	}
	if !strings.Contains(database.query, "health_checked_at IS NULL OR health_checked_at < $6") {
		t.Fatalf("availability update must reject stale in-flight attestations: %s", database.query)
	}
	if !strings.Contains(database.query, "version = $9") {
		t.Fatalf("availability must be bound to the probed capability version: %s", database.query)
	}
	gotTime, ok := database.args[5].(time.Time)
	if !ok || gotTime.Location() != time.UTC {
		t.Fatalf("health timestamp = %#v, want UTC", database.args[5])
	}
}

func TestSetAvailabilityForOrgFailsClosedOnMissingRowOrDatabaseError(t *testing.T) {
	t.Parallel()

	update := AvailabilityUpdate{ExpectedVersion: "1", HealthCheckedAt: time.Now()}

	missing := &CapabilitiesStore{pool: &availabilityDatabase{tag: pgconn.NewCommandTag("UPDATE 0")}}
	updated, err := missing.AttestAvailabilityForOrg(context.Background(), "cap.read", "org-a", "service:health", update)
	if err != nil || updated {
		t.Fatalf("missing updated = %v, err = %v", updated, err)
	}

	databaseFailure := errors.New("database unavailable")
	failing := &CapabilitiesStore{pool: &availabilityDatabase{err: databaseFailure}}
	updated, err = failing.AttestAvailabilityForOrg(context.Background(), "cap.read", "org-a", "service:health", update)
	if updated || !errors.Is(err, databaseFailure) {
		t.Fatalf("failure updated = %v, err = %v", updated, err)
	}
}

func TestCapabilityUpsertInvalidatesHealthWhenExecutableContractChanges(t *testing.T) {
	t.Parallel()

	database := &availabilityDatabase{tag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &CapabilitiesStore{pool: database}
	err := store.Upsert(context.Background(), &CapabilityRow{
		ID: "cap.read", OrgID: "org-a", Kind: "tool", Name: "Read",
		Version: "2", Enabled: true, RiskLevel: "low", CreatedBy: "service:registry",
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{
		"IS DISTINCT FROM (EXCLUDED.version",
		"THEN 'unavailable' ELSE capabilities.availability_state",
		"THEN 'unknown' ELSE capabilities.cost_class",
		"THEN NULL ELSE capabilities.health_checked_at",
	} {
		if !strings.Contains(database.query, required) {
			t.Fatalf("upsert does not invalidate stale availability (%q): %s", required, database.query)
		}
	}
}

func TestCapabilityUpsertRejectsUnknownRiskBeforeDatabaseWrite(t *testing.T) {
	t.Parallel()

	database := &availabilityDatabase{tag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &CapabilitiesStore{pool: database}
	err := store.Upsert(context.Background(), &CapabilityRow{
		ID: "cap.invalid", OrgID: "org-a", Kind: "tool", Name: "Invalid",
		Version: "1", Enabled: true, RiskLevel: "critical-ish", CreatedBy: "service:registry",
	})
	if err == nil || !strings.Contains(err.Error(), "risk") {
		t.Fatalf("Upsert error = %v, want invalid risk", err)
	}
	if database.query != "" {
		t.Fatalf("invalid risk reached database: %s", database.query)
	}
}
