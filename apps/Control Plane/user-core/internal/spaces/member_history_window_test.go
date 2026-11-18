package spaces

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

// D18 pure legs: the closed per-member modes, the invite and admin validation, the
// content-free audit payload, the view a response shows, the timestamp floor on the
// signed thread-read decision, and every refusal that must come before a database.

// recordingAuditOutbox records the audit rows a change enqueues, in place of user_audit_outbox.
type recordingAuditOutbox struct {
	events []recordedAudit
}

type recordedAudit struct {
	EventID string
	Subject string
	Payload []byte
}

func (o *recordingAuditOutbox) EnqueueAuditTx(_ context.Context, _ pgx.Tx, eventID, subject string, payload []byte) error {
	o.events = append(o.events, recordedAudit{EventID: eventID, Subject: subject, Payload: payload})
	return nil
}

func assertRoomCode(t *testing.T, err error, status int, code string) {
	t.Helper()
	var room *RoomError
	if !errors.As(err, &room) || room.Status != status || room.Code != code {
		t.Fatalf("want %d %s, got %v", status, code, err)
	}
}

func TestMemberHistoryWindowSwitchDefaultsOff(t *testing.T) {
	var nilRepo *Repository
	if nilRepo.MemberHistoryWindowsEnabled() {
		t.Fatal("a nil repository must report the switch off")
	}
	repo := &Repository{}
	if repo.MemberHistoryWindowsEnabled() {
		t.Fatal("SPACE_MEMBER_HISTORY_WINDOWS must default off")
	}
	repo.SetMemberHistoryWindows(true)
	if !repo.MemberHistoryWindowsEnabled() {
		t.Fatal("the switch must turn on")
	}
	repo.SetMemberHistoryWindows(false)
	if repo.MemberHistoryWindowsEnabled() {
		t.Fatal("the switch must turn off again")
	}
}

func TestNormalizeMemberHistoryWindowAcceptsTheThreeModesAndCanonicalisesSinceAt(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	mode, at, err := NormalizeMemberHistoryWindow(" full ", nil, now)
	if err != nil || mode != "full" || at != nil {
		t.Fatalf("full = (%q, %v, %v)", mode, at, err)
	}
	mode, at, err = NormalizeMemberHistoryWindow("since_join", nil, now)
	if err != nil || mode != "since_join" || at != nil {
		t.Fatalf("since_join = (%q, %v, %v)", mode, at, err)
	}
	// 02:00 at UTC+1 is 01:00 UTC: the stored instant is UTC.
	local := time.Date(2026, 11, 1, 2, 0, 0, 0, time.FixedZone("CET", 3600))
	mode, at, err = NormalizeMemberHistoryWindow("since_date", &local, now)
	if err != nil || mode != "since_date" || at == nil || !at.Equal(local) || at.Location() != time.UTC {
		t.Fatalf("since_date = (%q, %v, %v)", mode, at, err)
	}
}

func TestNormalizeMemberHistoryWindowRefusesEveryMalformedWindow(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	past := now.Add(-time.Hour)
	future := now.Add(time.Hour)
	zero := time.Time{}
	cases := []struct {
		name  string
		mode  string
		since *time.Time
	}{
		{"an unknown mode", "forever", nil},
		{"the room default is not a requested mode", MemberHistoryModeRoom, nil},
		{"an empty mode", "", nil},
		{"since_date without a date", "since_date", nil},
		{"since_date with a zero date", "since_date", &zero},
		{"since_date in the future", "since_date", &future},
		{"since_join with a date", "since_join", &past},
		{"full with a date", "full", &past},
	}
	for _, tc := range cases {
		_, _, err := NormalizeMemberHistoryWindow(tc.mode, tc.since, now)
		var room *RoomError
		if err == nil || !errors.As(err, &room) || room.Status != 422 || room.Code != CodeMemberHistoryWindowInvalid {
			t.Fatalf("%s must be refused with 422 %s, got %v", tc.name, CodeMemberHistoryWindowInvalid, err)
		}
	}
}

func TestSetMemberHistoryWindowRefusesBeforeTouchingTheDatabase(t *testing.T) {
	// No pool is configured: every refusal below must be answered before the first query.
	req := MemberHistoryWindowRequest{SpaceRef: "space-1", OrgID: "org-1", ActorID: "owner-1", SubjectID: "member-1", Mode: "full"}
	repo := &Repository{}
	_, err := repo.SetMemberHistoryWindow(t.Context(), req)
	assertRoomCode(t, err, 503, CodeMemberHistoryWindowUnavailable)

	repo.SetMemberHistoryWindows(true)
	_, err = repo.SetMemberHistoryWindow(t.Context(), req)
	assertRoomCode(t, err, 503, CodeSpaceAuthorityUnavailable) // no audit outbox is wired

	repo.SetPolicyAuditOutbox(&recordingAuditOutbox{})
	bad := req
	bad.Mode = "forever"
	_, err = repo.SetMemberHistoryWindow(t.Context(), bad)
	assertRoomCode(t, err, 422, CodeMemberHistoryWindowInvalid)

	// Valid input with the switch on and the audit wired reaches the pool, which is absent here.
	_, err = repo.SetMemberHistoryWindow(t.Context(), req)
	assertRoomCode(t, err, 503, CodeSpaceAuthorityUnavailable)

	missing := req
	missing.SubjectID = ""
	_, err = repo.SetMemberHistoryWindow(t.Context(), missing)
	var room *RoomError
	if !errors.As(err, &room) || room.Code != CodeInvalidRequest {
		t.Fatalf("a missing subject must be refused as an invalid request, got %v", err)
	}
}

func TestMemberHistoryWindowAuditPayloadIsContentFreeAndNamesTheRoomDefault(t *testing.T) {
	since := time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
	when := time.Date(2026, 11, 18, 9, 30, 0, 0, time.UTC)
	change := memberWindowChange{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "member-1", ActorID: "owner-1", ActorRole: RoleOwner,
		Source: historyWindowSourceAdmin, From: memberWindowState{Exists: true, Mode: "since_join", Revision: 2},
		ToMode: MemberHistoryModeSinceDate, ToSinceAt: &since,
	}
	raw, err := buildMemberHistoryWindowAuditPayload(change, 3, when)
	if err != nil {
		t.Fatalf("build: %v", err)
	}
	var payload map[string]any
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if payload["event"] != "space_member_history_window_changed" || payload["event_id"] != "member-history:space-1:member-1:3" {
		t.Fatalf("event identity: %v", payload)
	}
	details, ok := payload["details"].(map[string]any)
	if !ok {
		t.Fatalf("details missing: %v", payload)
	}
	if details["from_mode"] != "since_join" || details["to_mode"] != "since_date" || details["to_since_at"] != "2026-10-01T00:00:00Z" {
		t.Fatalf("modes and instant: %v", details)
	}
	if details["from_since_at"] != nil {
		t.Fatalf("an absent instant must be JSON null: %v", details["from_since_at"])
	}
	if details["revision_before"] != float64(2) || details["revision_after"] != float64(3) {
		t.Fatalf("revisions: %v", details)
	}

	// No row before: the audit names the room default, not an empty string.
	change.From = memberWindowState{}
	change.ToMode = MemberHistoryModeSinceJoin
	change.ToSinceAt = nil
	change.Source = historyWindowSourceInvite
	raw, err = buildMemberHistoryWindowAuditPayload(change, 1, when)
	if err != nil {
		t.Fatalf("build without a row: %v", err)
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("decode without a row: %v", err)
	}
	details = payload["details"].(map[string]any)
	if details["from_mode"] != MemberHistoryModeRoom || details["source"] != historyWindowSourceInvite {
		t.Fatalf("a first invite change reads from the room: %v", details)
	}

	// Content-free by construction: the top-level and detail keys are a closed set.
	keys := make([]string, 0, len(payload))
	for k := range payload {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	want := "actor_role,details,event,event_id,occurred_at,org_id,outcome,plane,producer,resource_id,subject,user_id"
	if got := strings.Join(keys, ","); got != want {
		t.Fatalf("top-level keys = %s, want %s", got, want)
	}
}

func TestMemberHistoryWindowEventIdIsUniquePerRowRevision(t *testing.T) {
	first := memberHistoryWindowEventID("space-1", "member-1", 1)
	second := memberHistoryWindowEventID("space-1", "member-1", 2)
	if first == second || first != "member-history:space-1:member-1:1" {
		t.Fatalf("event ids: %q %q", first, second)
	}
}

func TestWindowViewReadsTheRoomUnlessAMemberRowDecides(t *testing.T) {
	room := windowView(memberWindowState{}, HistoryModeSinceJoin)
	if room.Source != historyFloorSourceRoom || room.Mode != HistoryModeSinceJoin || room.Revision != 0 {
		t.Fatalf("no row reads the room: %+v", room)
	}
	reset := windowView(memberWindowState{Exists: true, Mode: MemberHistoryModeRoom, Revision: 4}, HistoryModeFull)
	if reset.Source != historyFloorSourceRoom || reset.Mode != HistoryModeFull || reset.Revision != 4 {
		t.Fatalf("a room reset reads the room and keeps its revision: %+v", reset)
	}
	member := windowView(memberWindowState{Exists: true, Mode: MemberHistoryModeSinceJoin, Revision: 1}, HistoryModeFull)
	if member.Source != historyFloorSourceMember || member.Mode != MemberHistoryModeSinceJoin {
		t.Fatalf("a member row decides over the room: %+v", member)
	}
}

func TestThreadReadDecisionCarriesAMemberTimestampFloorAndDigestIgnoresIt(t *testing.T) {
	now := time.Now().UTC()
	since := time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
	plain := validSharedThreadReadEvidence()
	windowed := validSharedThreadReadEvidence()
	windowed.HistoryFloorAt = &since

	base, err := IssueSharedThreadReadDecision(plain, readRequest(), now)
	if err != nil {
		t.Fatalf("issue plain: %v", err)
	}
	with, err := IssueSharedThreadReadDecision(windowed, readRequest(), now)
	if err != nil {
		t.Fatalf("issue windowed: %v", err)
	}
	if with.HistoryFloorAt == nil || !with.HistoryFloorAt.Equal(since) {
		t.Fatalf("floor instant = %v, want %v", with.HistoryFloorAt, since)
	}
	raw, _ := json.Marshal(with)
	if !strings.Contains(string(raw), `"history_floor_at":"2026-10-01T00:00:00Z"`) {
		t.Fatalf("the instant must be signed with the decision: %s", raw)
	}
	if base.PayloadDigest != with.PayloadDigest {
		t.Fatal("the instant must not be part of the payload digest")
	}
	plainRaw, _ := json.Marshal(base)
	if strings.Contains(string(plainRaw), "history_floor_at") {
		t.Fatalf("a decision without a member window must omit the claim: %s", plainRaw)
	}
}

func TestCanvasRunActionFloorIsANoOpWithoutCanvasEntitlementAndFailsClosedWithout(t *testing.T) {
	repo := &Repository{}
	evidence := validSharedThreadReadEvidence()
	evidence.CanvasEntitled = false
	if err := repo.ApplyCanvasRunActionFloor(t.Context(), &evidence); err != nil {
		t.Fatalf("an unentitled room is refused on the bit alone, not here: %v", err)
	}
	evidence.CanvasEntitled = true
	if err := repo.ApplyCanvasRunActionFloor(t.Context(), &evidence); err == nil {
		t.Fatal("an entitled room with no database must fail the canvas floor closed")
	}
}
