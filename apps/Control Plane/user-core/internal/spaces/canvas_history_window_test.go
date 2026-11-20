package spaces

import (
	"errors"
	"testing"
	"time"
)

// D18/D21m canvas rules. A canvas read or a save carries the member's timestamp floor so the canvas verifier can
// window the canvas by time, and both stay refused while SPACE_CANVAS_HISTORY_WINDOW is off (ResolveSharedCanvasDecisionEvidence
// gates on op; IssueCanvasDecision below trusts that gate and never re-checks it). A save under the older WS04-S11
// revision floor is still refused outright: D21m touches only the timestamp floor. A restore is refused under any
// floor, switch or not — D21m hides every version from before the window. A run action never writes under any floor.

func canvasWindowInstant() time.Time {
	return time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
}

func withTimestampFloor(e PersonalThreadDecisionEvidence) PersonalThreadDecisionEvidence {
	at := canvasWindowInstant()
	e.HistoryFloorAt = &at
	return e
}

func withRevisionFloor(e PersonalThreadDecisionEvidence) PersonalThreadDecisionEvidence {
	e.HistoryFloorRevision = 3
	return e
}

func TestCanvasHistoryWindowSwitchDefaultsOffAndTurnsOnAndOff(t *testing.T) {
	var nilRepo *Repository
	if nilRepo.CanvasHistoryWindowEnabled() {
		t.Fatal("a nil repository must report the canvas window off")
	}
	repo := &Repository{}
	if repo.CanvasHistoryWindowEnabled() {
		t.Fatal("SPACE_CANVAS_HISTORY_WINDOW must default off")
	}
	repo.SetCanvasHistoryWindow(true)
	if !repo.CanvasHistoryWindowEnabled() {
		t.Fatal("the switch must turn on")
	}
	repo.SetCanvasHistoryWindow(false)
	if repo.CanvasHistoryWindowEnabled() {
		t.Fatal("the switch must turn off again")
	}
}

// The one gate both canvas read paths share. It refuses a timestamp floor while the window is off, and passes a
// revision floor through, because a revision floor is refused by the canvas verifier itself (WS04-S11, unchanged).
func TestRefuseCanvasTimestampFloorGatesOnlyATimestampFloorOnARead(t *testing.T) {
	plain := validSharedCanvasEvidence("viewer")
	timestamp := withTimestampFloor(plain)
	revision := withRevisionFloor(plain)

	cases := []struct {
		name     string
		evidence PersonalThreadDecisionEvidence
		enabled  bool
		refused  bool
	}{
		{"no floor, window off", plain, false, false},
		{"no floor, window on", plain, true, false},
		{"timestamp floor, window off is refused", timestamp, false, true},
		{"timestamp floor, window on is issued", timestamp, true, false},
		{"revision floor, window off passes to the verifier", revision, false, false},
		{"revision floor, window on passes to the verifier", revision, true, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := refuseCanvasTimestampFloor(tc.evidence, tc.enabled)
			if tc.refused && !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
				t.Fatalf("err = %v, want ErrCanvasHistoryFloorUnsupported", err)
			}
			if !tc.refused && err != nil {
				t.Fatalf("err = %v, want nil", err)
			}
		})
	}
}

// A canvas read under a timestamp floor is issued with that floor, for the verifier to window. The digest does not
// bind the floor, so the floored read and the plain read have the same payload digest.
func TestIssueCanvasReadCarriesTheTimestampFloorAndTheDigestIgnoresIt(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	plain, err := IssueCanvasDecision(validSharedCanvasEvidence("viewer"), canvasReadRequest(), now)
	if err != nil {
		t.Fatalf("plain read: %v", err)
	}
	windowed, err := IssueCanvasDecision(withTimestampFloor(validSharedCanvasEvidence("viewer")), canvasReadRequest(), now)
	if err != nil {
		t.Fatalf("floored read: %v", err)
	}
	if windowed.HistoryFloorAt == nil || !windowed.HistoryFloorAt.Equal(canvasWindowInstant()) {
		t.Fatalf("the read must carry the member's instant, got %v", windowed.HistoryFloorAt)
	}
	if plain.HistoryFloorAt != nil || plain.HistoryFloorRevision != 0 {
		t.Fatalf("a plain read must carry no floor: %+v", plain)
	}
	if plain.PayloadDigest != windowed.PayloadDigest {
		t.Fatal("the floor must not change the canvas payload digest")
	}
}

// A restore is refused under any floor, timestamp or revision: it re-heads an older version, which D21m never
// allows a windowed member to see. Without a floor it is allowed for an editor.
func TestIssueCanvasRestoreIsRefusedUnderAnyFloor(t *testing.T) {
	now := canvasWindowInstant().Add(24 * time.Hour)
	request := canvasRestoreRequest()
	if _, err := IssueCanvasDecision(validSharedCanvasEvidence("editor"), request, now); err != nil {
		t.Fatalf("an unfloored editor's restore must be issued: %v", err)
	}
	floors := map[string]func(PersonalThreadDecisionEvidence) PersonalThreadDecisionEvidence{
		"timestamp": withTimestampFloor,
		"revision":  withRevisionFloor,
	}
	for name, floor := range floors {
		t.Run(name, func(t *testing.T) {
			_, err := IssueCanvasDecision(floor(validSharedCanvasEvidence("editor")), request, now)
			if !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
				t.Fatalf("a restore under a %s floor must be refused, got %v", name, err)
			}
		})
	}
}

// D21m: a save under a D18 timestamp floor alone is allowed and carries the floor (the current canvas is
// current state, not history; fail-closed for an unwindowed build sits in ResolveSharedCanvasDecisionEvidence,
// not here). A save under the older WS04-S11 revision floor is still refused outright.
func TestIssueCanvasSaveIsAllowedUnderATimestampFloorButNotARevisionFloor(t *testing.T) {
	now := canvasWindowInstant().Add(24 * time.Hour)
	request := canvasSaveRequest()
	if _, err := IssueCanvasDecision(validSharedCanvasEvidence("editor"), request, now); err != nil {
		t.Fatalf("an unfloored editor's save must be issued: %v", err)
	}
	decision, err := IssueCanvasDecision(withTimestampFloor(validSharedCanvasEvidence("editor")), request, now)
	if err != nil {
		t.Fatalf("a save under a timestamp floor must be issued: %v", err)
	}
	if decision.HistoryFloorAt == nil || !decision.HistoryFloorAt.Equal(canvasWindowInstant()) {
		t.Fatalf("the save must carry the member's floor, got %v", decision.HistoryFloorAt)
	}
	if _, err := IssueCanvasDecision(withRevisionFloor(validSharedCanvasEvidence("editor")), request, now); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a save under a revision floor must still be refused, got %v", err)
	}
}

// A create writes a canvas only where none exists, so it exposes no pre-floor content and is issued under a floor,
// carrying it so the verifier can see the member is windowed.
func TestIssueCanvasCreateUnderAFloorIsAllowedAndCarriesIt(t *testing.T) {
	now := canvasWindowInstant().Add(24 * time.Hour)
	decision, err := IssueCanvasDecision(withTimestampFloor(validSharedCanvasEvidence("editor")), canvasCreateRequest(), now)
	if err != nil {
		t.Fatalf("a create under a floor writes new content only and must be issued: %v", err)
	}
	if decision.HistoryFloorAt == nil || !decision.HistoryFloorAt.Equal(canvasWindowInstant()) {
		t.Fatalf("the create must carry the member's floor, got %v", decision.HistoryFloorAt)
	}
	if decision.ActionID != canvasWriteAction {
		t.Fatalf("a create is a write decision, got action %q", decision.ActionID)
	}
}

// A personal canvas carries no floor (a personal Space has one member), and the shared-only helper leaves it alone
// without touching the database.
func TestApplyReadHistoryFloorSkipsAPersonalSpaceAndFailsClosedForARoom(t *testing.T) {
	repo := &Repository{}
	personal := personalCanvasEvidence("owner")
	if err := repo.ApplyReadHistoryFloor(t.Context(), &personal); err != nil {
		t.Fatalf("a personal Space has no window and must not be resolved: %v", err)
	}
	if personal.HistoryFloorAt != nil || personal.HistoryFloorRevision != 0 {
		t.Fatalf("a personal Space must stay floor-free: %+v", personal)
	}
	room := validSharedCanvasEvidence("viewer")
	if err := repo.ApplyReadHistoryFloor(t.Context(), &room); err == nil {
		t.Fatal("a shared Space floor with no database must fail closed, never read as full history")
	}
	if err := repo.ApplyReadHistoryFloor(t.Context(), nil); err != nil {
		t.Fatalf("nil evidence has nothing to resolve: %v", err)
	}
}

// A canvas run action reads under a timestamp floor only through the rollout gate (ApplyCanvasRunActionFloor), and a
// run action never writes under any floor.
func TestValidateForCanvasRunActionRefusesAWriteUnderAnyFloorAndARevisionFloorOnARead(t *testing.T) {
	plain := validSharedCanvasEvidence("editor")
	cases := []struct {
		name     string
		evidence PersonalThreadDecisionEvidence
		write    bool
		refused  bool
	}{
		{"read, no floor", plain, false, false},
		{"write, no floor", plain, true, false},
		{"read, timestamp floor (gated by the rollout switch, not here)", withTimestampFloor(plain), false, false},
		{"write, timestamp floor", withTimestampFloor(plain), true, true},
		{"read, revision floor", withRevisionFloor(plain), false, true},
		{"write, revision floor", withRevisionFloor(plain), true, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.evidence.ValidateForCanvasRunAction(tc.write)
			if tc.refused && !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
				t.Fatalf("err = %v, want ErrCanvasHistoryFloorUnsupported", err)
			}
			if !tc.refused && err != nil {
				t.Fatalf("err = %v, want nil", err)
			}
		})
	}
}
