package spaces

import (
	"context"
	"testing"
)

// F-f03-01 live proof for BrowserFloor, gated on TEST_DATABASE_URL like the other live
// tests and reusing their fixture (its cleanup removes these rows). The answer is the stored
// bit. A missing effect-policy row, a Space that is not active, an unknown Space and a Space
// asked for under another organisation are all not entitled, and none of them is an error.
func TestBrowserFloorLive(t *testing.T) {
	f := newDenialLiveFixture(t)
	ctx := context.Background()
	repo := f.repo

	active := f.space(t, "browser-active", "room", "active")
	deleting := f.space(t, "browser-deleting", "room", "deleting")

	// No effect-policy row yet: not entitled, and not an error.
	if got, err := repo.BrowserFloor(ctx, f.org, active); err != nil || got {
		t.Fatalf("a Space with no policy row must read not entitled: got=%v err=%v", got, err)
	}

	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')`, f.org)

	// The default is off.
	if got, err := repo.BrowserFloor(ctx, f.org, active); err != nil || got {
		t.Fatalf("the default browser floor must read not entitled: got=%v err=%v", got, err)
	}

	// Turned on, the active Space reads entitled.
	f.exec(t, `UPDATE space_effect_policies SET browser_entitled = TRUE WHERE org_id = $1`, f.org)
	if got, err := repo.BrowserFloor(ctx, f.org, active); err != nil || !got {
		t.Fatalf("an active Space of an entitled organisation must read entitled: got=%v err=%v", got, err)
	}

	// A Space that is not active reads not entitled even with the bit on.
	if got, err := repo.BrowserFloor(ctx, f.org, deleting); err != nil || got {
		t.Fatalf("a Space that is not active must read not entitled: got=%v err=%v", got, err)
	}

	// An unknown Space reads not entitled, and is not an error.
	if got, err := repo.BrowserFloor(ctx, f.org, "ws03s2-browser-missing-"+f.suffix); err != nil || got {
		t.Fatalf("an unknown Space must read not entitled: got=%v err=%v", got, err)
	}

	// The same Space asked for under another organisation reads not entitled.
	if got, err := repo.BrowserFloor(ctx, "ws03s2-org-other-"+f.suffix, active); err != nil || got {
		t.Fatalf("a Space must not be read under another organisation: got=%v err=%v", got, err)
	}
}
