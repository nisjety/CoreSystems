//go:build integration

package registry

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

func readClass(t *testing.T, store *CapabilitiesStore, id, orgID string) (string, string) {
	t.Helper()
	row, err := store.GetForOrg(context.Background(), id, orgID)
	if err != nil {
		t.Fatalf("read %s: %v", id, err)
	}
	return row.SideEffectClass, row.ApprovalFloor
}

// A plain capability:write caller must not be able to lower a class or a floor,
// and an ordinary metadata edit must not move them either way. Only the
// override scope lowers, and kind=mcp_tool is never written by a generic upsert.
func TestPlainWriterCannotLowerAClassAgainstPostgres(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()
	store, err := NewCapabilitiesStore(pool)
	if err != nil {
		t.Fatalf("store: %v", err)
	}
	base := CapabilityRow{
		ID: "cap.tenant.deploy", OrgID: "org-a", Kind: models.KindTool, Name: "deploy",
		Version: "1.0.0", RiskLevel: models.RiskMedium, Scope: "org", Enabled: true,
	}

	// A new tenant row is born unclassified, the strictest class.
	if err := store.Upsert(ctx, &base, false); err != nil {
		t.Fatalf("create: %v", err)
	}
	if class, floor := readClass(t, store, base.ID, "org-a"); class != models.SideEffectUnclassified || floor != models.ApprovalFloorNone {
		t.Fatalf("new row class = %s/%s, want unclassified/none", class, floor)
	}

	// Classifying it is a lowering from unclassified, so it needs the override.
	classified := base
	classified.SideEffectClass, classified.ApprovalFloor = models.SideEffectDestructive, models.ApprovalFloorAsk
	if err := store.Upsert(ctx, &classified, false); !errors.Is(err, ErrToolClassFloorViolation) {
		t.Fatalf("plain classification err = %v, want ErrToolClassFloorViolation", err)
	}
	if err := store.Upsert(ctx, &classified, true); err != nil {
		t.Fatalf("override classification: %v", err)
	}

	// A plain writer's metadata edit leaves the pair alone.
	edit := base
	edit.Description = "now with a description"
	if err := store.Upsert(ctx, &edit, false); err != nil {
		t.Fatalf("metadata edit: %v", err)
	}
	if class, floor := readClass(t, store, base.ID, "org-a"); class != models.SideEffectDestructive || floor != models.ApprovalFloorAsk {
		t.Fatalf("metadata edit moved the class to %s/%s", class, floor)
	}

	// Lowering the class, or the floor alone, is refused and changes nothing.
	for name, pair := range map[string][2]string{
		"lower the class": {models.SideEffectRead, models.ApprovalFloorAsk},
		"lower the floor": {models.SideEffectDestructive, models.ApprovalFloorNone},
	} {
		lowering := base
		lowering.SideEffectClass, lowering.ApprovalFloor = pair[0], pair[1]
		if err := store.Upsert(ctx, &lowering, false); !errors.Is(err, ErrToolClassFloorViolation) {
			t.Fatalf("%s: err = %v, want ErrToolClassFloorViolation", name, err)
		}
	}
	if class, floor := readClass(t, store, base.ID, "org-a"); class != models.SideEffectDestructive || floor != models.ApprovalFloorAsk {
		t.Fatalf("a refused write moved the class to %s/%s", class, floor)
	}

	// Raising is allowed to a plain writer; lowering is the override's.
	raised := base
	raised.SideEffectClass, raised.ApprovalFloor = models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner
	if err := store.Upsert(ctx, &raised, false); err != nil {
		t.Fatalf("raise: %v", err)
	}
	lowered := base
	lowered.SideEffectClass, lowered.ApprovalFloor = models.SideEffectRead, models.ApprovalFloorNone
	if err := store.Upsert(ctx, &lowered, true); err != nil {
		t.Fatalf("override lowering: %v", err)
	}
	if class, floor := readClass(t, store, base.ID, "org-a"); class != models.SideEffectRead || floor != models.ApprovalFloorNone {
		t.Fatalf("override lowering left %s/%s", class, floor)
	}

	// Neither scope may create an mcp_tool row through the generic upsert.
	mcp := base
	mcp.ID, mcp.Name, mcp.Kind = "cap.tenant.mcp", "mcp", models.KindMCPTool
	for _, override := range []bool{false, true} {
		if err := store.Upsert(ctx, &mcp, override); !errors.Is(err, ErrMCPToolKindReserved) {
			t.Fatalf("mcp_tool upsert (override=%v) err = %v, want ErrMCPToolKindReserved", override, err)
		}
	}
}
