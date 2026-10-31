package registry

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// classFloorDatabase answers the two reads an Upsert makes before it writes: the
// risk floor's prior risk_level and the class floor's prior (class, floor) pair.
// priorClass empty means the row does not exist yet.
type classFloorDatabase struct {
	priorClass, priorFloor string
	readErr                error
	execs                  []string
	execArgs               [][]any
}

func (d *classFloorDatabase) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	d.execs = append(d.execs, query)
	d.execArgs = append(d.execArgs, append([]any(nil), args...))
	return pgconn.CommandTag{}, nil
}

func (*classFloorDatabase) Query(context.Context, string, ...any) (pgx.Rows, error) {
	panic("unexpected Query")
}

func (d *classFloorDatabase) QueryRow(_ context.Context, query string, _ ...any) pgx.Row {
	switch {
	case d.readErr != nil:
		return availabilityRow{err: d.readErr}
	case strings.Contains(query, "SELECT risk_level"):
		return availabilityRow{err: pgx.ErrNoRows}
	case strings.Contains(query, "SELECT side_effect_class"):
		if d.priorClass == "" {
			return availabilityRow{err: pgx.ErrNoRows}
		}
		return availabilityRow{values: []any{d.priorClass, d.priorFloor}}
	default:
		panic("unexpected QueryRow: " + query)
	}
}

func upsertWithClass(db *classFloorDatabase, row CapabilityRow, override bool) error {
	row.ID, row.OrgID, row.Name, row.RiskLevel = "cap.test.tool", "org-a", "test_tool", models.RiskLow
	if row.Kind == "" {
		row.Kind = models.KindTool
	}
	return (&CapabilitiesStore{pool: db}).Upsert(context.Background(), &row, override)
}

func TestUpsertWithoutAClassLeavesTheStoredClassAlone(t *testing.T) {
	t.Parallel()

	db := &classFloorDatabase{priorClass: models.SideEffectDestructive, priorFloor: models.ApprovalFloorAsk}
	if err := upsertWithClass(db, CapabilityRow{}, false); err != nil {
		t.Fatalf("plain upsert: %v", err)
	}
	if len(db.execs) != 1 {
		t.Fatalf("exec calls = %d, want 1", len(db.execs))
	}
	args := db.execArgs[0]
	// The last three arguments are the insert-only defaults and the write flag:
	// a class-less upsert must not be able to move the pair in either direction.
	got := args[len(args)-3:]
	if got[0] != models.SideEffectUnclassified || got[1] != models.ApprovalFloorNone || got[2] != false {
		t.Fatalf("class arguments = %v, want the defaults with the write flag off", got)
	}
	if !strings.Contains(db.execs[0], "CASE WHEN $23 THEN EXCLUDED.side_effect_class ELSE capabilities.side_effect_class END") {
		t.Fatal("ON CONFLICT must keep the stored class unless the write flag is on")
	}
}

func TestPlainWriterCannotLowerAClassOrAFloor(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name                   string
		priorClass, priorFloor string
		class, floor           string
		override, wantOK       bool
	}{
		{"lower destructive to read", models.SideEffectDestructive, models.ApprovalFloorNone, models.SideEffectRead, models.ApprovalFloorNone, false, false},
		{"lower the floor alone", models.SideEffectRead, models.ApprovalFloorAsk, models.SideEffectRead, models.ApprovalFloorNone, false, false},
		{"set a class on a new row", "", "", models.SideEffectRead, models.ApprovalFloorNone, false, false},
		{"unclassified default to context_write", models.SideEffectUnclassified, models.ApprovalFloorNone, models.SideEffectContextWrite, models.ApprovalFloorNone, false, false},
		{"raise read to destructive", models.SideEffectRead, models.ApprovalFloorNone, models.SideEffectDestructive, models.ApprovalFloorNone, false, true},
		{"keep the same pair", models.SideEffectExternalWrite, models.ApprovalFloorAsk, models.SideEffectExternalWrite, models.ApprovalFloorAsk, false, true},
		{"raise the floor alone", models.SideEffectRead, models.ApprovalFloorNone, models.SideEffectRead, models.ApprovalFloorAsk, false, true},
		{"override lowers a class", models.SideEffectDestructive, models.ApprovalFloorAsk, models.SideEffectRead, models.ApprovalFloorNone, true, true},
		{"override classifies a new row", "", "", models.SideEffectSandboxCompute, models.ApprovalFloorNone, true, true},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			db := &classFloorDatabase{priorClass: tc.priorClass, priorFloor: tc.priorFloor}
			err := upsertWithClass(db, CapabilityRow{SideEffectClass: tc.class, ApprovalFloor: tc.floor}, tc.override)
			if tc.wantOK {
				if err != nil {
					t.Fatalf("upsert refused: %v", err)
				}
				args := db.execArgs[0]
				got := args[len(args)-3:]
				if got[0] != tc.class || got[1] != tc.floor || got[2] != true {
					t.Fatalf("class arguments = %v, want the requested pair with the write flag on", got)
				}
				return
			}
			if !errors.Is(err, ErrToolClassFloorViolation) {
				t.Fatalf("err = %v, want ErrToolClassFloorViolation", err)
			}
			if len(db.execs) != 0 {
				t.Fatal("a refused class write must not reach the database")
			}
		})
	}
}

func TestClassWriteMustBeWellFormedAndMCPToolIsReserved(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name     string
		row      CapabilityRow
		override bool
		wantErr  error
	}{
		{"class without a floor", CapabilityRow{SideEffectClass: models.SideEffectRead}, true, nil},
		{"floor without a class", CapabilityRow{ApprovalFloor: models.ApprovalFloorAsk}, true, nil},
		{"owner effect below durable_owner", CapabilityRow{SideEffectClass: models.SideEffectOwnerEffect, ApprovalFloor: models.ApprovalFloorAsk}, true, nil},
		{"unknown class", CapabilityRow{SideEffectClass: "readonly", ApprovalFloor: models.ApprovalFloorNone}, true, nil},
		{"mcp_tool is reserved", CapabilityRow{Kind: models.KindMCPTool}, false, ErrMCPToolKindReserved},
		{"mcp_tool is reserved even with the override scope", CapabilityRow{Kind: models.KindMCPTool, SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}, true, ErrMCPToolKindReserved},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			db := &classFloorDatabase{}
			err := upsertWithClass(db, tc.row, tc.override)
			if err == nil {
				t.Fatal("upsert accepted a class write it must refuse")
			}
			if tc.wantErr != nil && !errors.Is(err, tc.wantErr) {
				t.Fatalf("err = %v, want %v", err, tc.wantErr)
			}
			if len(db.execs) != 0 {
				t.Fatal("a refused write must not reach the database")
			}
		})
	}
}

func TestClassFloorFailsClosedWhenThePriorRowCannotBeRead(t *testing.T) {
	t.Parallel()

	db := &classFloorDatabase{readErr: errors.New("connection reset")}
	err := upsertWithClass(db, CapabilityRow{SideEffectClass: models.SideEffectDestructive, ApprovalFloor: models.ApprovalFloorNone}, false)
	if err == nil || len(db.execs) != 0 {
		t.Fatalf("err = %v, execs = %d: an unreadable prior row must refuse the write", err, len(db.execs))
	}
}
