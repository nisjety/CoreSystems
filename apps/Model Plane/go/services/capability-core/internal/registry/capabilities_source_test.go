package registry

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// sourceRows is a fake pgx.Rows over canned value rows. Only the methods the
// source calls are implemented; the embedded nil interface makes any other call
// fail loudly instead of silently succeeding.
type sourceRows struct {
	pgx.Rows
	rows    [][]any
	current int
	scanErr error
}

func (r *sourceRows) Next() bool {
	r.current++
	return r.current <= len(r.rows)
}

func (r *sourceRows) Scan(destinations ...any) error {
	if r.scanErr != nil {
		return r.scanErr
	}
	return availabilityRow{values: r.rows[r.current-1]}.Scan(destinations...)
}

func (*sourceRows) Close() {}

func (*sourceRows) Err() error { return nil }

// sourceQuerier records the query a Load makes and answers it with canned rows.
type sourceQuerier struct {
	rows    [][]any
	scanErr error
	err     error
	queries []string
	args    [][]any
}

func (q *sourceQuerier) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	q.queries = append(q.queries, query)
	q.args = append(q.args, append([]any(nil), args...))
	if q.err != nil {
		return nil, q.err
	}
	return &sourceRows{rows: q.rows, scanErr: q.scanErr}, nil
}

// sourceCapabilityValues is one row in the order of sourceCapabilityColumns.
func sourceCapabilityValues(id, orgID, class, floor string) []any {
	healthCheckedAt := time.Date(2026, time.July, 16, 9, 59, 0, 0, time.UTC)
	return []any{
		id, orgID, "tool", "Retrieval " + id, "1.0.0", "retrieves evidence",
		"low", "global", false, true,
		"idem-" + id, []string{"org"}, "stable",
		"available", "runtime_healthy",
		"probe succeeded", "direct_read", "bounded",
		&healthCheckedAt, class, floor,
	}
}

func TestSourceLoadReadsTheStoredClassAndFloorOnBothQueries(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name  string
		orgID string
	}{
		{"every org", ""},
		{"one org and the global catalog", "org-a"},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			querier := &sourceQuerier{rows: [][]any{
				sourceCapabilityValues("cap.a", "global", models.SideEffectExternalWrite, models.ApprovalFloorAsk),
				sourceCapabilityValues("cap.b", "global", models.SideEffectRead, models.ApprovalFloorNone),
			}}
			caps, err := (&CapabilitiesSource{db: querier, orgID: tc.orgID}).Load()
			if err != nil {
				t.Fatalf("Load: %v", err)
			}
			if len(querier.queries) != 1 || !strings.Contains(querier.queries[0], "side_effect_class, approval_floor") {
				t.Fatalf("Load must select the class and floor columns: %v", querier.queries)
			}
			if len(caps) != 2 {
				t.Fatalf("capabilities = %d, want 2", len(caps))
			}
			if caps[0].SideEffectClass != models.SideEffectExternalWrite || caps[0].ApprovalFloor != models.ApprovalFloorAsk {
				t.Errorf("cap.a class/floor = %q/%q, want the stored external_write/ask", caps[0].SideEffectClass, caps[0].ApprovalFloor)
			}
			if caps[1].SideEffectClass != models.SideEffectRead || caps[1].ApprovalFloor != models.ApprovalFloorNone {
				t.Errorf("cap.b class/floor = %q/%q, want the stored read/none", caps[1].SideEffectClass, caps[1].ApprovalFloor)
			}
			if caps[0].EnabledForScopes[0] != "org" || caps[0].HealthCheckedAt == nil {
				t.Errorf("the other columns must still scan: %#v", caps[0])
			}
		})
	}
}

// The registry the gRPC read surface serves from is built from this source, so
// a stored pair has to survive into the in-memory capability. Without it every
// capability would carry an empty floor, which the wire reads as the strictest.
func TestRegistryBuiltFromTheSourceCarriesTheStoredClassAndFloor(t *testing.T) {
	t.Parallel()

	querier := &sourceQuerier{rows: [][]any{
		sourceCapabilityValues("cap.retrieval.query", "global", models.SideEffectRead, models.ApprovalFloorNone),
		sourceCapabilityValues("cap.tool.ticket.create", "global", models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner),
	}}
	reg, err := NewFromSource(&CapabilitiesSource{db: querier})
	if err != nil {
		t.Fatalf("NewFromSource: %v", err)
	}
	for id, want := range map[string][2]string{
		"cap.retrieval.query":    {models.SideEffectRead, models.ApprovalFloorNone},
		"cap.tool.ticket.create": {models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner},
	} {
		capability, err := reg.GetForOrg(id, "", "org-a")
		if err != nil {
			t.Fatalf("%s: %v", id, err)
		}
		if capability.SideEffectClass != want[0] || capability.ApprovalFloor != want[1] {
			t.Errorf("%s: class/floor = %q/%q, want %q/%q", id, capability.SideEffectClass, capability.ApprovalFloor, want[0], want[1])
		}
		// What the wire would report for the loaded value is the stored pair, not
		// the strictest floor an empty value would read as.
		if got := models.EffectiveApprovalFloor(capability.SideEffectClass, capability.ApprovalFloor); got != want[1] {
			t.Errorf("%s: effective floor = %q, want %q", id, got, want[1])
		}
	}
}

func TestSourceLoadSurfacesQueryAndScanFailures(t *testing.T) {
	t.Parallel()

	if _, err := (&CapabilitiesSource{db: &sourceQuerier{err: errors.New("down")}}).Load(); err == nil {
		t.Fatal("a failed query must fail the load")
	}
	querier := &sourceQuerier{rows: [][]any{sourceCapabilityValues("cap.a", "global", "read", "none")}, scanErr: errors.New("bad row")}
	if _, err := (&CapabilitiesSource{db: querier}).Load(); err == nil {
		t.Fatal("a failed scan must fail the load")
	}
	// A value list that no longer matches the column list is a failed scan, not
	// a silently shifted one.
	short := sourceCapabilityValues("cap.a", "global", "read", "none")
	querier = &sourceQuerier{rows: [][]any{short[:len(short)-2]}}
	if _, err := (&CapabilitiesSource{db: querier}).Load(); err == nil {
		t.Fatal("a row without the class and floor columns must not scan")
	}
}
