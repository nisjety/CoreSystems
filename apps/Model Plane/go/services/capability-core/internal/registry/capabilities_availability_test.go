package registry

import (
	"context"
	"errors"
	"fmt"
	"reflect"
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
	row   pgx.Row
}

type availabilityRow struct {
	values []any
	err    error
}

func (row availabilityRow) Scan(destinations ...any) error {
	if row.err != nil {
		return row.err
	}
	if len(destinations) != len(row.values) {
		return fmt.Errorf("scan destinations = %d, values = %d", len(destinations), len(row.values))
	}
	for index, value := range row.values {
		destination := reflect.ValueOf(destinations[index])
		if destination.Kind() != reflect.Ptr || destination.IsNil() {
			return fmt.Errorf("scan destination %d is not a non-nil pointer", index)
		}
		if value == nil {
			destination.Elem().SetZero()
			continue
		}
		destination.Elem().Set(reflect.ValueOf(value))
	}
	return nil
}

func (database *availabilityDatabase) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	database.query = query
	database.args = append([]any(nil), args...)
	return database.tag, database.err
}

func (*availabilityDatabase) Query(context.Context, string, ...any) (pgx.Rows, error) {
	panic("unexpected Query")
}

func (database *availabilityDatabase) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	database.query = query
	database.args = append([]any(nil), args...)
	if database.row == nil {
		return availabilityRow{err: pgx.ErrNoRows}
	}
	return database.row
}

func availabilityCapabilityValues(id, orgID string) []any {
	createdAt := time.Date(2026, time.July, 16, 10, 0, 0, 0, time.UTC)
	healthCheckedAt := createdAt.Add(-time.Minute)
	return []any{
		id, orgID, "tool", "Retrieval", "1.0.0", "retrieves evidence",
		"low", "org", false, true, "idem-capability",
		[]byte(`{"type":"object"}`), []byte(`{"type":"object"}`), []byte(`{}`), []string{"retrieval"}, []string{"org"},
		0.99, 0.01, 10.0, 0.001,
		0.02, 0, 4.5, "stable",
		"available", "runtime_healthy", "probe succeeded",
		"direct_read", "bounded", &healthCheckedAt,
		"service:registry", createdAt, createdAt,
	}
}

func TestGetGlobalPinsLookupToGlobalCapabilityRow(t *testing.T) {
	t.Parallel()

	database := &availabilityDatabase{row: availabilityRow{values: availabilityCapabilityValues("cap.retrieval.query", "global")}}
	store := &CapabilitiesStore{pool: database}

	row, err := store.GetGlobal(context.Background(), "cap.retrieval.query")
	if err != nil {
		t.Fatalf("GetGlobal error = %v", err)
	}
	if row.ID != "cap.retrieval.query" || row.OrgID != "global" || row.Version != "1.0.0" {
		t.Fatalf("global row = %#v", row)
	}
	if len(database.args) != 1 || database.args[0] != "cap.retrieval.query" {
		t.Fatalf("GetGlobal args = %#v", database.args)
	}
	for _, required := range []string{
		"WHERE id = $1 AND org_id = 'global' AND deleted_at IS NULL",
		"SELECT id, org_id",
	} {
		if !strings.Contains(database.query, required) {
			t.Fatalf("GetGlobal query missing %q: %s", required, database.query)
		}
	}
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

func TestSetAvailabilityGlobalPinsGlobalRowAndWritesDistinctAuditEvent(t *testing.T) {
	t.Parallel()

	database := &availabilityDatabase{tag: pgconn.NewCommandTag("UPDATE 1")}
	store := &CapabilitiesStore{pool: database}
	updated, err := store.AttestAvailabilityGlobal(context.Background(), "cap.retrieval.query", "service:capability-health", AvailabilityUpdate{
		ExpectedVersion: "1.0.0",
		State:           "available",
		ReasonCode:      "runtime_healthy",
		Reason:          "probe succeeded",
		ExecutionMode:   "direct_read",
		CostClass:       "bounded",
		HealthCheckedAt: time.Now(),
	})
	if err != nil || !updated {
		t.Fatalf("updated = %v, err = %v", updated, err)
	}
	if len(database.args) != 11 || database.args[6] != "cap.retrieval.query" || database.args[7] != "1.0.0" || database.args[9] != "service:capability-health" {
		t.Fatalf("global attestation arguments = %#v", database.args)
	}
	for _, required := range []string{
		"org_id = 'global'",
		"global_availability_attested",
		"health_checked_at IS NULL OR health_checked_at < $6",
		"version = $8",
	} {
		if !strings.Contains(database.query, required) {
			t.Fatalf("global attestation missing %q: %s", required, database.query)
		}
	}
}

func TestSetAvailabilityGlobalRejectsIncompleteAttestationBeforeDatabaseWrite(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		id      string
		actor   string
		version string
	}{
		{name: "missing capability", actor: "service:capability-health", version: "1.0.0"},
		{name: "missing actor", id: "cap.retrieval.query", version: "1.0.0"},
		{name: "missing expected version", id: "cap.retrieval.query", actor: "service:capability-health"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			database := &availabilityDatabase{tag: pgconn.NewCommandTag("UPDATE 1")}
			store := &CapabilitiesStore{pool: database}
			updated, err := store.AttestAvailabilityGlobal(context.Background(), test.id, test.actor, AvailabilityUpdate{
				ExpectedVersion: test.version,
				HealthCheckedAt: time.Now(),
			})
			if err == nil || updated {
				t.Fatalf("updated = %v, err = %v", updated, err)
			}
			if database.query != "" {
				t.Fatalf("incomplete attestation reached database: %s", database.query)
			}
		})
	}
}

func TestSetAvailabilityGlobalFailsClosedWithoutMatchingGlobalRowOrOnDatabaseError(t *testing.T) {
	t.Parallel()

	databaseUnavailable := errors.New("database unavailable")
	update := AvailabilityUpdate{
		ExpectedVersion: "1.0.0",
		State:           "available",
		ReasonCode:      "runtime_healthy",
		Reason:          "probe succeeded",
		ExecutionMode:   "direct_read",
		CostClass:       "bounded",
		HealthCheckedAt: time.Now(),
	}
	tests := []struct {
		name        string
		database    *availabilityDatabase
		wantErr     error
		wantUpdated bool
	}{
		{
			name:        "stale version or non-global row does not update or audit",
			database:    &availabilityDatabase{tag: pgconn.NewCommandTag("UPDATE 0")},
			wantUpdated: false,
		},
		{
			name:        "database error fails closed",
			database:    &availabilityDatabase{err: databaseUnavailable},
			wantErr:     databaseUnavailable,
			wantUpdated: false,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			store := &CapabilitiesStore{pool: test.database}
			updated, err := store.AttestAvailabilityGlobal(context.Background(), "cap.retrieval.query", "service:capability-health", update)
			if updated != test.wantUpdated {
				t.Fatalf("updated = %v, want %v", updated, test.wantUpdated)
			}
			if test.wantErr == nil {
				if err != nil {
					t.Fatalf("error = %v", err)
				}
			} else if !errors.Is(err, test.wantErr) {
				t.Fatalf("error = %v, want %v", err, test.wantErr)
			}
			for _, required := range []string{
				"org_id = 'global'",
				"version = $8",
				"FROM updated",
				"global_availability_attested",
			} {
				if !strings.Contains(test.database.query, required) {
					t.Fatalf("global attestation query missing %q: %s", required, test.database.query)
				}
			}
		})
	}
}

func TestCapabilityUpsertInvalidatesHealthWhenExecutableContractChanges(t *testing.T) {
	t.Parallel()

	database := &availabilityDatabase{tag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &CapabilitiesStore{pool: database}
	err := store.Upsert(context.Background(), &CapabilityRow{
		ID: "cap.read", OrgID: "org-a", Kind: "tool", Name: "Read",
		Version: "2", Enabled: true, RiskLevel: "low", CreatedBy: "service:registry",
	}, false)
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
	}, false)
	if err == nil || !strings.Contains(err.Error(), "risk") {
		t.Fatalf("Upsert error = %v, want invalid risk", err)
	}
	if database.query != "" {
		t.Fatalf("invalid risk reached database: %s", database.query)
	}
}

// TestCapabilityUpsertEnforcesRiskFloor covers POL-1: a plain capability:write
// caller must never be able to silently lower a high-risk capability's
// risk_level (the shape of the flip that would disable policy/engine.go's
// High -> Ask human-approval gate), while ordinary, non-floored risk
// management keeps working exactly as before.
func TestCapabilityUpsertEnforcesRiskFloor(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name               string
		id                 string
		priorRow           pgx.Row // nil => pgx.ErrNoRows (ON CONFLICT target has no existing row)
		requestRisk        string
		hasRiskOverride    bool
		wantErr            bool
		wantFloorViolation bool // when wantErr: is it specifically ErrRiskFloorViolation?
		wantWrite          bool // whether the INSERT/upsert Exec should have been reached
	}{
		{
			name:               "downgrade with plain write scope is rejected",
			id:                 "cap.command.shell",
			priorRow:           availabilityRow{values: []any{"high"}},
			requestRisk:        "low",
			hasRiskOverride:    false,
			wantErr:            true,
			wantFloorViolation: true,
			wantWrite:          false,
		},
		{
			name:            "downgrade with risk override scope is allowed",
			id:              "cap.command.shell",
			priorRow:        availabilityRow{values: []any{"high"}},
			requestRisk:     "low",
			hasRiskOverride: true,
			wantErr:         false,
			wantWrite:       true,
		},
		{
			name:            "upgrade from low to high is always allowed",
			id:              "cap.tenant.custom",
			priorRow:        availabilityRow{values: []any{"low"}},
			requestRisk:     "high",
			hasRiskOverride: false,
			wantErr:         false,
			wantWrite:       true,
		},
		{
			name:            "non-floored medium to low change is unaffected",
			id:              "cap.tenant.custom",
			priorRow:        availabilityRow{values: []any{"medium"}},
			requestRisk:     "low",
			hasRiskOverride: false,
			wantErr:         false,
			wantWrite:       true,
		},
		{
			name:               "unknown seeded-high capability with no readable prior state is refused",
			id:                 "cap.browser.open",
			priorRow:           availabilityRow{err: pgx.ErrNoRows},
			requestRisk:        "low",
			hasRiskOverride:    false,
			wantErr:            true,
			wantFloorViolation: true,
			wantWrite:          false,
		},
		{
			name:            "genuinely new, unseeded capability is unaffected",
			id:              "cap.tenant.brand-new",
			priorRow:        availabilityRow{err: pgx.ErrNoRows},
			requestRisk:     "low",
			hasRiskOverride: false,
			wantErr:         false,
			wantWrite:       true,
		},
		{
			// A real lookup failure (not "no rows") is a distinct, generic
			// error, not ErrRiskFloorViolation: the HTTP handler must surface
			// it as a 500 (infrastructure failure), not a 403 (policy
			// decision). Either way, the write must never proceed on it.
			name:            "a floor-check database error fails closed",
			id:              "cap.tenant.custom",
			priorRow:        availabilityRow{err: errors.New("connection reset")},
			requestRisk:     "low",
			hasRiskOverride: false,
			wantErr:         true,
			wantWrite:       false,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			database := &availabilityDatabase{tag: pgconn.NewCommandTag("INSERT 0 1"), row: test.priorRow}
			store := &CapabilitiesStore{pool: database}
			err := store.Upsert(context.Background(), &CapabilityRow{
				ID: test.id, OrgID: "org-a", Kind: "tool", Name: "thing",
				Version: "1", Enabled: true, RiskLevel: test.requestRisk, CreatedBy: "service:registry",
			}, test.hasRiskOverride)

			switch {
			case test.wantErr && test.wantFloorViolation && !errors.Is(err, ErrRiskFloorViolation):
				t.Fatalf("Upsert error = %v, want ErrRiskFloorViolation", err)
			case test.wantErr && err == nil:
				t.Fatalf("Upsert error = nil, want an error")
			case !test.wantErr && err != nil:
				t.Fatalf("Upsert error = %v, want nil", err)
			}

			wroteRow := strings.Contains(database.query, "INSERT INTO capabilities")
			if wroteRow != test.wantWrite {
				t.Fatalf("wrote row = %v, want %v (query: %s)", wroteRow, test.wantWrite, database.query)
			}
		})
	}
}
