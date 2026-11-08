package spaces

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"regexp"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS04-S8 (pure tier): visibility, the directory policy, self-join and settings
// request validation, and the membership read's new fields. Everything that
// needs the database is in directory_live_test.go.

func asRoomError(t *testing.T, err error) *RoomError {
	t.Helper()
	var roomErr *RoomError
	if !errors.As(err, &roomErr) {
		t.Fatalf("error = %v (%T), want *RoomError", err, err)
	}
	return roomErr
}

func TestValidVisibilityIsTheClosedPair(t *testing.T) {
	for _, ok := range []string{VisibilityPrivate, VisibilityOrgPublic, "private", "org_public"} {
		if !ValidVisibility(ok) {
			t.Errorf("%q must be valid", ok)
		}
	}
	for _, bad := range []string{"", "public", "Private", "ORG_PUBLIC", " org_public", "internal"} {
		if ValidVisibility(bad) {
			t.Errorf("%q must be invalid", bad)
		}
	}
	if VisibilityPrivate != "private" {
		t.Fatal("private must remain the default value of the column")
	}
}

func TestDirectoryPolicyNeedsAnOrganization(t *testing.T) {
	if err := (DirectoryPolicy{OrgID: "org-1"}).Validate(); err != nil {
		t.Fatalf("a policy naming an org is valid (switch off is the default): %v", err)
	}
	for _, org := range []string{"", "   "} {
		if err := (DirectoryPolicy{OrgID: org, OrgPublicRoomsEnabled: true}).Validate(); err == nil {
			t.Errorf("org %q must be refused", org)
		}
	}
	var zero DirectoryPolicy
	if zero.OrgPublicRoomsEnabled {
		t.Fatal("the zero policy must deny")
	}
}

func TestUpsertDirectoryPolicyRefusesBeforeAnyWrite(t *testing.T) {
	ctx := context.Background()
	var nilRepo *Repository
	if _, err := nilRepo.UpsertDirectoryPolicy(ctx, DirectoryPolicy{OrgID: "org-1"}, "control-space-policy"); err == nil {
		t.Fatal("a nil repository must fail closed")
	}
	withDB := &Repository{db: &database.DB{}} // no pool: these must refuse before touching it
	if _, err := withDB.UpsertDirectoryPolicy(ctx, DirectoryPolicy{OrgID: " "}, "control-space-policy"); err == nil {
		t.Fatal("a blank org must be refused")
	}
	if _, err := withDB.UpsertDirectoryPolicy(ctx, DirectoryPolicy{OrgID: "org-1"}, "  "); err == nil || !strings.Contains(err.Error(), "actor") {
		t.Fatalf("a blank actor must be refused: %v", err)
	}
}

func TestListDirectoryRoomsRefusesUnknownCallersAndUnavailableAuthority(t *testing.T) {
	ctx := context.Background()
	repo := &Repository{db: &database.DB{}}
	for _, args := range [][2]string{{"", "user-1"}, {"org-1", ""}, {"  ", "  "}} {
		if _, err := repo.ListDirectoryRooms(ctx, args[0], args[1]); !errors.Is(err, ErrDirectoryCallerNotInOrg) {
			t.Errorf("ListDirectoryRooms(%q,%q) = %v, want ErrDirectoryCallerNotInOrg", args[0], args[1], err)
		}
	}
	var nilRepo *Repository
	_, err := nilRepo.ListDirectoryRooms(ctx, "org-1", "user-1")
	if roomErr := asRoomError(t, err); roomErr.Status != 503 || roomErr.Code != CodeSpaceAuthorityUnavailable {
		t.Fatalf("unavailable authority = %+v", roomErr)
	}
	if DirectoryRoomsLimit != 500 {
		t.Fatalf("DirectoryRoomsLimit = %d; the BFF and the SPA assume 500", DirectoryRoomsLimit)
	}
}

func TestJoinSpaceRefusesMalformedRequestsBeforeTheDatabase(t *testing.T) {
	ctx := context.Background()
	repo := &Repository{db: &database.DB{}}
	cases := []struct {
		name   string
		req    JoinRequest
		status int
		code   string
	}{
		{"no space ref", JoinRequest{OrgID: "o", ActorID: "u"}, 400, CodeInvalidRequest},
		{"no org looks like an unknown room", JoinRequest{SpaceRef: "s", ActorID: "u"}, 404, CodeRoomNotFound},
		{"no actor looks like an unknown room", JoinRequest{SpaceRef: "s", OrgID: "o"}, 404, CodeRoomNotFound},
		{"oversized idempotency key", JoinRequest{SpaceRef: "s", OrgID: "o", ActorID: "u", IdempotencyKey: strings.Repeat("k", 129)}, 400, CodeInvalidRequest},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := repo.JoinSpace(ctx, tc.req)
			roomErr := asRoomError(t, err)
			if roomErr.Status != tc.status || roomErr.Code != tc.code {
				t.Fatalf("= %d %s, want %d %s", roomErr.Status, roomErr.Code, tc.status, tc.code)
			}
		})
	}
	var nilRepo *Repository
	_, err := nilRepo.JoinSpace(ctx, JoinRequest{SpaceRef: "s", OrgID: "o", ActorID: "u"})
	if roomErr := asRoomError(t, err); roomErr.Status != 503 {
		t.Fatalf("no authority repository = %+v", roomErr)
	}
}

func TestJoinRefusalsForPrivateAndMissingRoomsShareOneBody(t *testing.T) {
	// The "byte-equal 404" contract: the constructor takes no argument, so an
	// unknown room, a private room, a disabled org, a personal Space and another
	// org's room cannot differ.
	a, _ := json.Marshal(roomNotFound())
	b, _ := json.Marshal(roomNotFound())
	if string(a) != string(b) {
		t.Fatalf("roomNotFound must be a constant body: %s vs %s", a, b)
	}
	if roomNotFound().Status != 404 || roomNotFound().Code != "room_not_found" || roomNotFound().Details != nil {
		t.Fatalf("roomNotFound = %+v", roomNotFound())
	}
	// A join may never mint an elevated role, whatever the column says.
	for _, elevated := range []string{RoleManager, RoleOwner, "", "root"} {
		if CanJoin(elevated) == nil {
			t.Errorf("CanJoin(%q) must refuse", elevated)
		}
	}
	for _, ok := range []string{RoleViewer, RoleEditor} {
		if CanJoin(ok) != nil {
			t.Errorf("CanJoin(%q) must allow", ok)
		}
	}
}

func TestUpdateRoomSettingsRefusesMalformedRequestsBeforeTheDatabase(t *testing.T) {
	ctx := context.Background()
	repo := &Repository{db: &database.DB{}}
	str := func(s string) *string { return &s }
	base := func() SettingsRequest { return SettingsRequest{SpaceRef: "s", OrgID: "o", ActorID: "u"} }
	cases := []struct {
		name   string
		mutate func(*SettingsRequest)
		status int
		code   string
		field  string
	}{
		{"no space", func(r *SettingsRequest) { r.SpaceRef = "" }, 400, CodeInvalidRequest, "space_ref"},
		{"no org looks like an unknown room", func(r *SettingsRequest) { r.OrgID = ""; r.Visibility = str("private") }, 404, CodeRoomNotFound, ""},
		{"nothing to change", func(r *SettingsRequest) {}, 400, CodeInvalidRequest, "settings"},
		{"unknown visibility", func(r *SettingsRequest) { r.Visibility = str("public") }, 400, CodeInvalidRequest, "visibility"},
		{"blank visibility", func(r *SettingsRequest) { r.Visibility = str("  ") }, 400, CodeInvalidRequest, "visibility"},
		{"manager join role", func(r *SettingsRequest) { r.JoinRole = str("manager") }, 400, CodeInvalidRequest, "join_role"},
		{"owner join role", func(r *SettingsRequest) { r.JoinRole = str("owner") }, 400, CodeInvalidRequest, "join_role"},
		{"empty join role", func(r *SettingsRequest) { r.JoinRole = str("") }, 400, CodeInvalidRequest, "join_role"},
		{"unknown history mode", func(r *SettingsRequest) { r.HistoryMode = str("forever") }, 400, CodeInvalidRequest, "history_mode"},
		{"oversized key", func(r *SettingsRequest) {
			r.Visibility = str("private")
			r.IdempotencyKey = strings.Repeat("k", 129)
		}, 400, CodeInvalidRequest, "idempotency_key"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := base()
			tc.mutate(&req)
			_, err := repo.UpdateRoomSettings(ctx, req)
			roomErr := asRoomError(t, err)
			if roomErr.Status != tc.status || roomErr.Code != tc.code {
				t.Fatalf("= %d %s, want %d %s", roomErr.Status, roomErr.Code, tc.status, tc.code)
			}
			if tc.field != "" && roomErr.Details["field"] != tc.field {
				t.Fatalf("field = %v, want %s", roomErr.Details["field"], tc.field)
			}
		})
	}
}

func TestDirectoryLedgerActionsMatchTheMigrationVocabulary(t *testing.T) {
	if ActionVisibilityChanged != "visibility_changed" || ActionSettingsChanged != "settings_changed" || ActionMemberJoined != "member_joined" {
		t.Fatalf("ledger actions drifted: %s %s %s", ActionVisibilityChanged, ActionSettingsChanged, ActionMemberJoined)
	}
	if CodeDirectoryDisabled != "directory_disabled" {
		t.Fatalf("CodeDirectoryDisabled = %q", CodeDirectoryDisabled)
	}
}

// GET /spaces/{ref}/membership carries visibility and join_role (WS04-S8): the
// SPA reads them to decide whether to offer the room's settings and join UI.
func TestMembershipReadCarriesVisibilityAndJoinRole(t *testing.T) {
	field := func(name string) reflect.StructField {
		f, ok := reflect.TypeOf(CurrentMembership{}).FieldByName(name)
		if !ok {
			t.Fatalf("CurrentMembership has no %s", name)
		}
		return f
	}
	if tag := field("Visibility").Tag.Get("json"); tag != "visibility,omitempty" {
		t.Errorf("Visibility json tag = %q", tag)
	}
	if tag := field("JoinRole").Tag.Get("json"); tag != "join_role,omitempty" {
		t.Errorf("JoinRole json tag = %q", tag)
	}
	body, err := json.Marshal(CurrentMembership{Visibility: "org_public", JoinRole: "viewer"})
	if err != nil || !strings.Contains(string(body), `"visibility":"org_public"`) || !strings.Contains(string(body), `"join_role":"viewer"`) {
		t.Fatalf("membership JSON = %s (%v)", body, err)
	}

	resolver := repositoryFunctions(t)["resolveUserMembership"]
	for _, column := range []string{"visibility", "join_role"} {
		if !regexp.MustCompile(`(?m)\b` + column + `\b`).MatchString(resolver) {
			t.Errorf("resolveUserMembership must select %s", column)
		}
	}
	if !strings.Contains(resolver, "&membership.Visibility") || !strings.Contains(resolver, "&membership.JoinRole") {
		t.Error("resolveUserMembership must scan visibility and join_role into the membership")
	}
}

// The directory read selects exactly the rooms the plan lists, in SQL.
func TestDirectoryRoomsQueryEncodesEveryListingRule(t *testing.T) {
	raw, err := os.ReadFile("directory_rooms.go")
	if err != nil {
		t.Fatalf("read directory_rooms.go: %v", err)
	}
	flat := strings.Join(strings.Fields(string(raw)), " ")
	for _, rule := range []string{
		"s.org_id=$1",
		"s.registration_state='active'",
		"s.space_kind='room'",
		"s.roster_mode='managed'",
		"s.visibility='org_public'",
		"NOT EXISTS ( SELECT 1 FROM space_memberships me",
		"me.subject_id=$2 AND me.active=TRUE",
	} {
		if !strings.Contains(flat, rule) {
			t.Errorf("directory query lacks rule %q", rule)
		}
	}
	if !strings.Contains(flat, "directoryEnabledTx") {
		t.Error("the directory must read the org policy (absence denies)")
	}
	if strings.Contains(flat, "registration_state IN") || strings.Contains(flat, "'archived'") {
		t.Error("archived rooms must never be listed")
	}
}
