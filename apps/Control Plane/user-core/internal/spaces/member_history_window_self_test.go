package spaces

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// D18: a member's own effective window, as the membership read shows it to that member. Each case names the resolved
// floor the reads sign and the view it must produce, so the self-read cannot drift from what the reads enforce.

// sameOwnHistoryWindow compares two views by value: the instant by its instant, not by pointer.
func sameOwnHistoryWindow(a, b OwnHistoryWindow) bool {
	if a.Source != b.Source || a.Mode != b.Mode || a.Hidden != b.Hidden {
		return false
	}
	if a.SinceAt == nil || b.SinceAt == nil {
		return a.SinceAt == b.SinceAt
	}
	return a.SinceAt.Equal(*b.SinceAt)
}

func TestOwnHistoryWindowAgreesWithTheFloorTheReadsEnforce(t *testing.T) {
	since := time.Date(2026, 10, 1, 9, 30, 0, 0, time.FixedZone("CET", 3600))
	sinceUTC := since.UTC()
	joined := time.Date(2026, 9, 12, 8, 0, 0, 0, time.UTC)

	cases := []struct {
		name  string
		floor HistoryFloor
		want  OwnHistoryWindow
	}{
		{
			name:  "a member's full row hides nothing",
			floor: HistoryFloor{Source: historyFloorSourceMember, Mode: MemberHistoryModeFull},
			want:  OwnHistoryWindow{Source: "member", Mode: "full", SinceAt: nil, Hidden: false},
		},
		{
			name:  "a since_date row hides before its instant, normalised to UTC",
			floor: HistoryFloor{At: &since, Source: historyFloorSourceMember, Mode: MemberHistoryModeSinceDate},
			want:  OwnHistoryWindow{Source: "member", Mode: "since_date", SinceAt: &sinceUTC, Hidden: true},
		},
		{
			name:  "a member's since_join row hides before its join instant",
			floor: HistoryFloor{At: &joined, Source: historyFloorSourceMember, Mode: MemberHistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "member", Mode: "since_join", SinceAt: &joined, Hidden: true},
		},
		{
			name:  "a founding member with no join and no revision hides nothing",
			floor: HistoryFloor{Source: historyFloorSourceMember, Mode: MemberHistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "member", Mode: "since_join", SinceAt: nil, Hidden: false},
		},
		{
			name:  "a since_join room with a WS04-S11 revision floor hides and names the join",
			floor: HistoryFloor{Revision: 3, At: &joined, Source: historyFloorSourceRoom, Mode: HistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "room", Mode: "since_join", SinceAt: &joined, Hidden: true},
		},
		{
			name:  "a since_join room whose join instant has no revision floor hides no message, though the membership feed uses the join",
			floor: HistoryFloor{At: &joined, Source: historyFloorSourceRoom, Mode: HistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "room", Mode: "since_join", SinceAt: &joined, Hidden: false},
		},
		{
			name:  "a since_join room with the claim off hides nothing although the room sets the mode",
			floor: HistoryFloor{Source: historyFloorSourceRoom, Mode: HistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "room", Mode: "since_join", SinceAt: nil, Hidden: false},
		},
		{
			name:  "a full room hides nothing",
			floor: HistoryFloor{Source: historyFloorSourceRoom, Mode: HistoryModeFull},
			want:  OwnHistoryWindow{Source: "room", Mode: "full", SinceAt: nil, Hidden: false},
		},
		{
			name:  "a since_join room founder with no revision floor hides nothing",
			floor: HistoryFloor{Source: historyFloorSourceRoom, Mode: HistoryModeSinceJoin},
			want:  OwnHistoryWindow{Source: "room", Mode: "since_join", SinceAt: nil, Hidden: false},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := ownHistoryWindowFrom(tc.floor)
			if !sameOwnHistoryWindow(got, tc.want) {
				t.Fatalf("ownHistoryWindowFrom(%+v) = %+v, want %+v", tc.floor, got, tc.want)
			}
		})
	}
}

func TestOwnHistoryWindowIsTheSelfShapeOnTheWire(t *testing.T) {
	raw, err := json.Marshal(ownHistoryWindowFrom(HistoryFloor{Source: historyFloorSourceMember, Mode: MemberHistoryModeFull}))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if got, want := string(raw), `{"source":"member","mode":"full","since_at":null,"hidden":false}`; got != want {
		t.Fatalf("self window wire shape = %s, want %s", got, want)
	}
	at := time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
	raw, err = json.Marshal(ownHistoryWindowFrom(HistoryFloor{At: &at, Source: historyFloorSourceMember, Mode: MemberHistoryModeSinceDate}))
	if err != nil {
		t.Fatalf("marshal dated: %v", err)
	}
	if !strings.Contains(string(raw), `"since_at":"2026-10-01T00:00:00Z"`) || !strings.Contains(string(raw), `"hidden":true`) {
		t.Fatalf("a dated window must name its instant and hide: %s", raw)
	}
}

// The membership read attaches the window only for the caller. Every other membership payload, including the one a
// decision is built from, leaves the field unset, so it is omitted and nothing downstream reads it.
func TestCurrentMembershipCarriesTheWindowOnlyWhenTheSelfReadSetsIt(t *testing.T) {
	unset, err := json.Marshal(CurrentMembership{SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: KindRoom, Role: "viewer"})
	if err != nil {
		t.Fatalf("marshal unset: %v", err)
	}
	if strings.Contains(string(unset), "history_window") {
		t.Fatalf("a membership without the self read must omit the window: %s", unset)
	}
	set := CurrentMembership{SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: KindRoom, Role: "viewer",
		HistoryWindow: &OwnHistoryWindow{Source: historyFloorSourceRoom, Mode: HistoryModeFull}}
	raw, err := json.Marshal(set)
	if err != nil {
		t.Fatalf("marshal set: %v", err)
	}
	if !strings.Contains(string(raw), `"history_window":{"source":"room","mode":"full","since_at":null,"hidden":false}`) {
		t.Fatalf("the self read must carry its window: %s", raw)
	}
}

// The self read reads through the one floor resolver. With no database it must fail closed, never report an
// unwindowed view.
func TestOwnHistoryWindowFailsClosedWithoutADatabase(t *testing.T) {
	repo := &Repository{}
	view, err := repo.OwnHistoryWindowFor(t.Context(), &CurrentMembership{
		SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: KindRoom, Role: "viewer",
	})
	if err == nil || view != nil {
		t.Fatalf("a repository without a database must refuse the self window, got view=%+v err=%v", view, err)
	}
	var nilRepo *Repository
	if view, err := nilRepo.OwnHistoryWindowFor(t.Context(), &CurrentMembership{SpaceRef: "space-room", SubjectID: "user-1"}); err == nil || view != nil {
		t.Fatalf("a nil repository must refuse the self window, got view=%+v err=%v", view, err)
	}
	if view, err := repo.OwnHistoryWindowFor(t.Context(), nil); err == nil || view != nil {
		t.Fatalf("no membership means no self window, got view=%+v err=%v", view, err)
	}
}
