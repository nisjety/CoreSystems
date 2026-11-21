package browsersites

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"
)

// errFloorStoreUnused marks the Store methods ResolveForSpace never calls, so a test
// that reaches one fails loudly.
var errFloorStoreUnused = errors.New("floor test store: method not used by ResolveForSpace")

// floorStore answers one policy set and one member's choices. ResolveForSpace reads only
// Snapshot.
type floorStore struct {
	set   PolicySet
	prefs map[string]Pref
}

func (f *floorStore) Get(context.Context, string) (*PolicySet, error) {
	return nil, errFloorStoreUnused
}

func (f *floorStore) Put(context.Context, string, string, int64, PutRequest) (*PolicySet, error) {
	return nil, errFloorStoreUnused
}

func (f *floorStore) Snapshot(_ context.Context, _, _ string, _ []string) (*PolicySet, map[string]Pref, error) {
	set := f.set
	return &set, f.prefs, nil
}

func (f *floorStore) ListPrefs(context.Context, string, string) (PrefSet, error) {
	return PrefSet{}, errFloorStoreUnused
}

func (f *floorStore) PutPref(context.Context, string, string, int64, Pref) (Pref, error) {
	return Pref{}, errFloorStoreUnused
}

func (f *floorStore) DeletePref(context.Context, string, string, string) error {
	return errFloorStoreUnused
}

func (f *floorStore) DeleteUserData(context.Context, string, string) (int64, int64, error) {
	return 0, 0, errFloorStoreUnused
}

func (f *floorStore) DeleteRemovedMemberData(context.Context, string, string) (int64, int64, error) {
	return 0, 0, errFloorStoreUnused
}

// recordingFloor answers one fixed floor and records every read, so a test can see
// whether the floor was read at all and for which organisation and Space.
type recordingFloor struct {
	entitled bool
	err      error
	reads    int
	org      string
	space    string
}

func (f *recordingFloor) BrowserFloor(_ context.Context, orgID, spaceID string) (bool, error) {
	f.reads++
	f.org, f.space = orgID, spaceID
	return f.entitled, f.err
}

// decisionFor returns the decision for one host, so a test does not depend on the order
// a resolution lists its hosts in.
func decisionFor(t *testing.T, resolution Resolution, host string) Decision {
	t.Helper()
	for _, decision := range resolution.Decisions {
		if decision.Host == host {
			return decision
		}
	}
	t.Fatalf("no decision for %s in %+v", host, resolution.Decisions)
	return Decision{}
}

func TestResolveForSpaceRefusesEveryHostUnlessTheFloorEntitlesTheSpace(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	store := &floorStore{
		set: PolicySet{Revision: 7, Mode: ModeOpen, Rules: []Rule{{Host: "blocked.example.com", Rule: RuleBlock}}},
		prefs: map[string]Pref{
			"open.example.com": {Host: "open.example.com", Level: LevelAct, ExpiresAt: now.Add(time.Hour)},
		},
	}
	clock := func() time.Time { return now }
	hosts := []string{"blocked.example.com", "open.example.com"}

	plain, err := NewService(store, clock).Resolve(context.Background(), "org-1", "u-member", hosts)
	if err != nil {
		t.Fatalf("plain resolve: %v", err)
	}
	plainOpen := decisionFor(t, plain, "open.example.com")
	if plainOpen.Effective != EffectiveAct || plainOpen.Source != SourceUserPref {
		t.Fatalf("without a Space the member's choice applies: %+v", plainOpen)
	}

	cases := []struct {
		name    string
		floor   SpaceFloorReader
		refuses bool
	}{
		{name: "no reader", floor: nil, refuses: true},
		{name: "reader says not entitled", floor: &recordingFloor{entitled: false}, refuses: true},
		{name: "reader errs although it says entitled", floor: &recordingFloor{entitled: true, err: errors.New("user-core unavailable")}, refuses: true},
		{name: "reader says entitled", floor: &recordingFloor{entitled: true}, refuses: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			service := NewService(store, clock)
			service.SetSpaceFloor(tc.floor)
			got, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "sp-1", hosts)
			if err != nil {
				t.Fatalf("resolve for space: %v", err)
			}
			if got.Revision != 7 || got.OrganizationID != "org-1" || got.UserID != "u-member" || len(got.Decisions) != 2 {
				t.Fatalf("the floor keeps the revision, the identity and every host: %+v", got)
			}
			if blocked := decisionFor(t, got, "blocked.example.com"); blocked.Effective != EffectiveDeny || blocked.Source != SourceOrgBlock {
				t.Errorf("a host already refused keeps its reason: %+v", blocked)
			}
			open := decisionFor(t, got, "open.example.com")
			if tc.refuses {
				if open.Effective != EffectiveDeny || open.Source != SourceSpaceFloor {
					t.Errorf("the floor must refuse the open host: %+v", open)
				}
			} else if open != plainOpen {
				t.Errorf("an entitled Space keeps the answer it had without a Space: %+v", open)
			}
			if recorded, ok := tc.floor.(*recordingFloor); ok {
				if recorded.reads != 1 || recorded.org != "org-1" || recorded.space != "sp-1" {
					t.Errorf("the floor must be read once for this organisation and Space: %+v", recorded)
				}
			}
		})
	}
}

func TestResolveForSpaceReadsTheFloorOnlyForAScopedCall(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	store := &floorStore{set: PolicySet{Revision: 7, Mode: ModeOpen}}
	service := NewService(store, func() time.Time { return now })
	hosts := []string{"open.example.com"}

	plain, err := service.Resolve(context.Background(), "org-1", "u-member", hosts)
	if err != nil {
		t.Fatalf("plain resolve: %v", err)
	}

	// An empty Space is an unscoped call: Resolve's answer, and the floor is not read.
	unscoped := &recordingFloor{entitled: false}
	service.SetSpaceFloor(unscoped)
	got, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "", hosts)
	if err != nil {
		t.Fatalf("unscoped resolve: %v", err)
	}
	if !reflect.DeepEqual(got, plain) {
		t.Fatalf("an unscoped call must get Resolve's answer unchanged:\ngot=%+v\nwant=%+v", got, plain)
	}
	if unscoped.reads != 0 {
		t.Fatalf("an unscoped call must not read the floor: %d reads", unscoped.reads)
	}

	// A whitespace-only Space is neither unscoped nor a Space: once trimmed it is an empty
	// space_id, refused as a validation error before the floor is read.
	blank := &recordingFloor{entitled: true}
	service.SetSpaceFloor(blank)
	_, err = service.ResolveForSpace(context.Background(), "org-1", "u-member", "  ", hosts)
	var invalid *ValidationError
	if !errors.As(err, &invalid) || len(invalid.Errors) != 1 || invalid.Errors[0].Path != "space_id" || invalid.Errors[0].Code != CodeInvalidSpaceID {
		t.Fatalf("a whitespace space_id must be an invalid_space_id ValidationError, got %v", err)
	}
	if blank.reads != 0 {
		t.Fatalf("a refused space_id must not read the floor: %d reads", blank.reads)
	}

	// No host is a refused resolve, so the floor is never read for it.
	none := &recordingFloor{entitled: true}
	service.SetSpaceFloor(none)
	if _, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "sp-1", nil); err == nil {
		t.Fatal("a resolve with no host must be refused")
	}
	if none.reads != 0 {
		t.Fatalf("a refused resolve must not read the floor: %d reads", none.reads)
	}
}

// A malformed space_id is refused before the floor is read, whatever the store holds, and the
// well-formed longest id is accepted.
func TestResolveForSpaceValidatesTheSpaceIdBeforeTheFloorIsRead(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	store := &floorStore{set: PolicySet{Revision: 7, Mode: ModeOpen}}
	hosts := []string{"open.example.com"}

	for name, spaceID := range map[string]string{
		"space":    "sp 1",
		"slash":    "sp/1",
		"dot":      "sp.1",
		"percent":  "sp%20",
		"unicode":  "sp-ä",
		"overlong": strings.Repeat("s", maxSpaceIDLength+1),
	} {
		t.Run(name, func(t *testing.T) {
			floor := &recordingFloor{entitled: true}
			service := NewService(store, func() time.Time { return now })
			service.SetSpaceFloor(floor)
			_, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", spaceID, hosts)
			var invalid *ValidationError
			if !errors.As(err, &invalid) || invalid.Errors[0].Code != CodeInvalidSpaceID {
				t.Fatalf("space id %q: want an invalid_space_id ValidationError, got %v", spaceID, err)
			}
			if floor.reads != 0 {
				t.Fatalf("space id %q: a refused space id must not read the floor: %d reads", spaceID, floor.reads)
			}
		})
	}

	// Surrounding space is trimmed before the floor is read, so the reader sees the id as named.
	padded := &recordingFloor{entitled: true}
	service := NewService(store, func() time.Time { return now })
	service.SetSpaceFloor(padded)
	if _, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "  sp-1 ", hosts); err != nil {
		t.Fatalf("a padded space id is well formed: %v", err)
	}
	if padded.reads != 1 || padded.space != "sp-1" {
		t.Fatalf("the reader must see the trimmed space id: reads=%d space=%q", padded.reads, padded.space)
	}

	longest := &recordingFloor{entitled: true}
	service.SetSpaceFloor(longest)
	if _, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", strings.Repeat("s", maxSpaceIDLength), hosts); err != nil {
		t.Fatalf("a space id at the cap is well formed: %v", err)
	}
	if longest.reads != 1 {
		t.Fatalf("a space id at the cap must be floored: reads=%d", longest.reads)
	}
}

// The floor is read on every Space call and nothing positive is kept: a Space that loses its
// entitlement is refused on the very next resolve.
func TestResolveForSpaceReadsTheFloorAgainOnEveryCall(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	store := &floorStore{set: PolicySet{Revision: 7, Mode: ModeOpen}}
	service := NewService(store, func() time.Time { return now })
	floor := &recordingFloor{entitled: true}
	service.SetSpaceFloor(floor)
	hosts := []string{"open.example.com"}

	first, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "sp-1", hosts)
	if err != nil {
		t.Fatalf("first resolve: %v", err)
	}
	if decision := decisionFor(t, first, "open.example.com"); decision.Effective == EffectiveDeny {
		t.Fatalf("an entitled Space must not refuse the host: %+v", decision)
	}

	floor.entitled = false
	second, err := service.ResolveForSpace(context.Background(), "org-1", "u-member", "sp-1", hosts)
	if err != nil {
		t.Fatalf("second resolve: %v", err)
	}
	if decision := decisionFor(t, second, "open.example.com"); decision.Effective != EffectiveDeny || decision.Source != SourceSpaceFloor {
		t.Fatalf("a Space that lost its entitlement must be refused at once: %+v", decision)
	}
	if floor.reads != 2 {
		t.Fatalf("the floor must be read on every Space call: reads=%d", floor.reads)
	}
}
