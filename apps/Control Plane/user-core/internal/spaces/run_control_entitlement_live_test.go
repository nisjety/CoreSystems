package spaces

import (
	"context"
	"testing"
)

// WS09-S7 live: migration 038 defaults the run-control bit to FALSE for an org
// policy row written without naming it, and the catalogue reads it back.
func TestMigration038DefaultsRunControlOffLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	org := "ws09s7-default-" + f.suffix
	t.Cleanup(func() {
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id=$1`, org)
	})
	f.exec(t, `
		INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		    retention_class, residency, deletion_scope)
		VALUES ($1, 'privacy:'||$1||':1', 'assistant_collaboration', 'contract', 'internal',
		    'standard', 'swedencentral', 'space')`, org)

	var entitled bool
	if err := f.pool.QueryRow(ctx, `SELECT run_control_entitled FROM space_effect_policies WHERE org_id=$1`, org).Scan(&entitled); err != nil {
		t.Fatalf("read run_control_entitled: %v", err)
	}
	if entitled {
		t.Fatal("run_control_entitled must default to FALSE")
	}
	policy, err := f.repo.ResolveOrgEffectPolicy(ctx, org)
	if err != nil {
		t.Fatalf("read the policy back: %v", err)
	}
	if policy.RunControlEntitled || EntitlementValues(policy)["run_control"] {
		t.Fatal("the catalogue must read run_control as off")
	}
}
