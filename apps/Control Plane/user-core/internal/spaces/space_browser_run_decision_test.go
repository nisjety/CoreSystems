package spaces

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// W3-DEC-BROWSER, WS20-S1 and WS20-S2 converge on this one file: the
// room-scoped browser run decision (model.space.browser_run).

func browserRunRoomEvidence() PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room-1"
	e.Membership.Role = RoleEditor
	e.RecipientAudienceRef = "audience:space-room-1:4"
	e.RecipientAudienceHash = "sha256:audience-hash"
	e.Membership.Revisions.RecipientAudience = 4
	e.ResourceAuthorizationRef = "control:space-room-1:browser-run:7"
	e.BrowserEntitled = true
	return e
}

func browserRunPersonalEvidence() PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.BrowserEntitled = true
	return e
}

func validBrowserRunRequest() BrowserRunDecisionRequest {
	return BrowserRunDecisionRequest{
		DecisionRef:    "browser-decision-1",
		ThreadID:       "thread-room-1",
		GoalDigest:     "sha256:" + strings.Repeat("a", 64),
		AllowedDomains: []string{"fixture.test", "docs.example.org"},
		ViewScope:      BrowserViewOwner,
		MaxSteps:       20,
		MaxRuntimeS:    300,
		IdempotencyKey: "space-browser-1",
		Nonce:          "nonce-1",
	}
}

// WS20-S1: browser_entitled is deny by default and explicit-only; off refuses
// with the typed entitlement code, on admits.
func TestBrowserRunDecisionRequiresEntitlement(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)

	off := browserRunRoomEvidence()
	off.BrowserEntitled = false
	_, _, err := IssueBrowserRunDecision(off, validBrowserRunRequest(), now)
	if denial := wantDenial(t, err, DenialEntitlementOff); denial.Entitlement != "browser" {
		t.Fatalf("entitlement = %q, want browser", denial.Entitlement)
	}

	on := browserRunRoomEvidence()
	decision, claims, err := IssueBrowserRunDecision(on, validBrowserRunRequest(), now)
	if err != nil {
		t.Fatalf("IssueBrowserRunDecision: %v", err)
	}
	if decision.ActionID != browserRunAction || decision.ServiceAudience != browserRunAudience ||
		decision.ActionSchemaHash != browserRunSchema {
		t.Fatalf("decision target = %+v", decision)
	}
	if claims.ThreadMode != browserThreadModeExisting {
		t.Fatalf("thread_mode = %q, want %q", claims.ThreadMode, browserThreadModeExisting)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued decision did not validate: %v", err)
	}
}

// WS20-S1: an empty thread_id signs the new-thread mode and binds no thread.
func TestBrowserRunDecisionNewThreadMode(t *testing.T) {
	now := time.Now().UTC()
	evidence := browserRunRoomEvidence()
	request := validBrowserRunRequest()
	request.ThreadID = ""
	decision, claims, err := IssueBrowserRunDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("IssueBrowserRunDecision: %v", err)
	}
	if claims.ThreadID != "" || claims.ThreadMode != browserThreadModeNew {
		t.Fatalf("claims = %+v, want empty thread id and thread_mode %q", claims, browserThreadModeNew)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued decision did not validate: %v", err)
	}
}

// WS20-S1: viewer and non-recipient are refused, Space ZDR refuses the
// effect, and DM/group-DM kinds have no browser run (D-8, as for the canvas).
func TestBrowserRunDecisionRefusals(t *testing.T) {
	now := time.Now().UTC()
	for name, test := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		check  func(*testing.T, error)
	}{
		"viewer": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = RoleViewer },
			func(t *testing.T, err error) { wantDenial(t, err, DenialRoleBelowFloor) },
		},
		"non-recipient": {
			func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef, e.RecipientAudienceHash = "", "" },
			func(t *testing.T, err error) { wantDenial(t, err, DenialAuthorityIncomplete) },
		},
		"missing resource authorization": {
			func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = "" },
			func(t *testing.T, err error) { wantDenial(t, err, DenialAuthorityIncomplete) },
		},
		"zero data retention": {
			func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialZDRForbidsEffect); denial.Effect != "browser_run" {
					t.Fatalf("effect = %q, want browser_run", denial.Effect)
				}
			},
		},
		"dm": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindDM },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialKindUnsupported); denial.Kind != string(KindDM) {
					t.Fatalf("kind = %q, want dm", denial.Kind)
				}
			},
		},
		"group_dm": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindGroupDM },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialKindUnsupported); denial.Kind != string(KindGroupDM) {
					t.Fatalf("kind = %q, want group_dm", denial.Kind)
				}
			},
		},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := browserRunRoomEvidence()
			test.mutate(&evidence)
			decision, _, err := IssueBrowserRunDecision(evidence, validBrowserRunRequest(), now)
			if err == nil {
				t.Fatalf("a browser run was issued: %+v", decision)
			}
			test.check(t, err)
		})
	}
}

// WS20-S1: the room view permission is added only for a shared Space whose
// view scope is room; a personal Space has one member and forces owner.
func TestBrowserRunViewRoomPermissionOnlyOnSharedSpace(t *testing.T) {
	now := time.Now().UTC()

	personal := browserRunPersonalEvidence()
	personalRequest := validBrowserRunRequest()
	personalRequest.ViewScope = BrowserViewRoom
	decision, claims, err := IssueBrowserRunDecision(personal, personalRequest, now)
	if err != nil {
		t.Fatalf("personal Space IssueBrowserRunDecision: %v", err)
	}
	if claims.ViewScope != BrowserViewOwner {
		t.Fatalf("a personal Space must force owner scope, got %q", claims.ViewScope)
	}
	if !reflect.DeepEqual(decision.Permissions, []string{browserPermissionRun}) {
		t.Fatalf("personal Space permissions = %#v", decision.Permissions)
	}

	room := browserRunRoomEvidence()
	roomRoomRequest := validBrowserRunRequest()
	roomRoomRequest.ViewScope = BrowserViewRoom
	decision, claims, err = IssueBrowserRunDecision(room, roomRoomRequest, now)
	if err != nil {
		t.Fatalf("shared Space room-view IssueBrowserRunDecision: %v", err)
	}
	if claims.ViewScope != BrowserViewRoom {
		t.Fatalf("view_scope = %q, want room", claims.ViewScope)
	}
	if !reflect.DeepEqual(decision.Permissions, []string{browserPermissionRun, browserPermissionViewRoom}) {
		t.Fatalf("shared Space room-view permissions = %#v", decision.Permissions)
	}

	roomOwnerRequest := validBrowserRunRequest() // default view_scope is owner
	decision, claims, err = IssueBrowserRunDecision(room, roomOwnerRequest, now)
	if err != nil {
		t.Fatalf("shared Space owner-view IssueBrowserRunDecision: %v", err)
	}
	if claims.ViewScope != BrowserViewOwner {
		t.Fatalf("view_scope = %q, want owner", claims.ViewScope)
	}
	if !reflect.DeepEqual(decision.Permissions, []string{browserPermissionRun}) {
		t.Fatalf("shared Space owner-view permissions = %#v", decision.Permissions)
	}
}

// WS20-S1: the domain policy's structural fail-fast. Quarry's SSRF/DNS guards
// remain the enforcement; this only keeps a malformed policy from being
// signed. A list of more than 8 hosts is refused by count, a repeated host by
// the duplicate check, and every other case here by the single-host shape
// check (validBrowserHost).
func TestBrowserRunDomainValidation(t *testing.T) {
	const hostMessage = "browser run allowed domain is not a permitted host"
	cases := []struct {
		name    string
		domains []string
		want    string
	}{
		{"ip literal", []string{"203.0.113.5"}, hostMessage},
		{"localhost", []string{"localhost"}, hostMessage},
		{"dot localhost", []string{"service.localhost"}, hostMessage},
		{"wildcard", []string{"*.example.com"}, hostMessage},
		{"empty", []string{""}, hostMessage},
		{"uppercase", []string{"Example.com"}, hostMessage},
		{"scheme prefix", []string{"https://example.com"}, hostMessage},
		{"path suffix", []string{"example.com/path"}, hostMessage},
		{"port suffix", []string{"example.com:8080"}, hostMessage},
		{"single-label host", []string{"example"}, hostMessage},
		{"all-digit final label", []string{"example.123"}, hostMessage},
		{"nine hosts", []string{
			"h1.example.com", "h2.example.com", "h3.example.com", "h4.example.com", "h5.example.com",
			"h6.example.com", "h7.example.com", "h8.example.com", "h9.example.com",
		}, "browser run allowed_domains must name 1 to 8 hosts"},
		{"duplicate entries", []string{"example.com", "example.com"},
			"browser run allowed_domains must not repeat a host"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			request := validBrowserRunRequest()
			request.AllowedDomains = tc.domains
			err := request.Validate()
			if err == nil {
				t.Fatalf("expected a refusal for domains %v", tc.domains)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error = %q, want it to contain %q", err.Error(), tc.want)
			}
		})
	}
}

// SD-23: at most 10 minutes and a step range whose bounds exclude 4 and 41.
func TestBrowserRunLimits(t *testing.T) {
	for _, steps := range []int{browserMinSteps - 1, browserMaxSteps + 1} {
		request := validBrowserRunRequest()
		request.MaxSteps = steps
		if err := request.Validate(); err == nil {
			t.Fatalf("max_steps=%d must be refused", steps)
		}
	}
	for _, steps := range []int{browserMinSteps, browserMaxSteps} {
		request := validBrowserRunRequest()
		request.MaxSteps = steps
		if err := request.Validate(); err != nil {
			t.Fatalf("max_steps=%d must be accepted: %v", steps, err)
		}
	}
	for _, runtime := range []int{browserMinRuntimeS - 1, browserMaxRuntimeS + 1} {
		request := validBrowserRunRequest()
		request.MaxRuntimeS = runtime
		if err := request.Validate(); err == nil {
			t.Fatalf("max_runtime_s=%d must be refused", runtime)
		}
	}
	for _, runtime := range []int{browserMinRuntimeS, browserMaxRuntimeS} {
		request := validBrowserRunRequest()
		request.MaxRuntimeS = runtime
		if err := request.Validate(); err != nil {
			t.Fatalf("max_runtime_s=%d must be accepted: %v", runtime, err)
		}
	}
}

// W3-DEC-BROWSER / WS20-S1 / WS20-S2: the shared Control/Rust golden. Go and
// the model-gateway verifier must stay pinned to the same bytes, so this
// reproduces every vector's payload_digest from the real browserRunPayloadDigest,
// including the thread_mode field (W3-DEC-BROWSER) and the new-thread vector
// (empty thread_id signs thread_mode "new").
func TestBrowserRunDigestGolden(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "space-browser-run-decision.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden struct {
		Input struct {
			OrgID                     string `json:"org_id"`
			UserID                    string `json:"user_id"`
			SpaceID                   string `json:"space_id"`
			RecipientAudienceRef      string `json:"recipient_audience_ref"`
			RecipientAudienceHash     string `json:"recipient_audience_hash"`
			PrivacyPolicyRef          string `json:"privacy_policy_ref"`
			ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
			AuthorityRevision         int64  `json:"authority_revision"`
			MembershipRevision        int64  `json:"membership_revision"`
			PrivacyRevision           int64  `json:"privacy_revision"`
			RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
			EntitlementRevision       int64  `json:"entitlement_revision"`
		} `json:"input"`
		Vectors []struct {
			Name                    string   `json:"name"`
			GoalDigest              string   `json:"goal_digest"`
			ThreadID                string   `json:"thread_id"`
			ThreadMode              string   `json:"thread_mode"`
			AllowedDomains          []string `json:"allowed_domains"`
			ViewScope               string   `json:"view_scope"`
			MaxSteps                int      `json:"max_steps"`
			MaxRuntimeS             int      `json:"max_runtime_s"`
			IdempotencyKey          string   `json:"idempotency_key"`
			CanonicalAllowedDomains []string `json:"canonical_allowed_domains"`
			PayloadDigest           string   `json:"payload_digest"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	in := golden.Input
	if len(golden.Vectors) == 0 {
		t.Fatal("golden carries no vectors")
	}

	byName := map[string]string{}
	for _, vector := range golden.Vectors {
		vector := vector
		t.Run(vector.Name, func(t *testing.T) {
			if got := browserThreadModeOf(vector.ThreadID); got != vector.ThreadMode {
				t.Fatalf("browserThreadModeOf(%q) = %q, want %q", vector.ThreadID, got, vector.ThreadMode)
			}
			canonical := canonicalBrowserDomains(vector.AllowedDomains)
			if !reflect.DeepEqual(canonical, vector.CanonicalAllowedDomains) {
				t.Fatalf("canonicalBrowserDomains(%v) = %v, want %v", vector.AllowedDomains, canonical, vector.CanonicalAllowedDomains)
			}

			evidence := PersonalThreadDecisionEvidence{
				Membership: CurrentMembership{
					OrgID: in.OrgID, SubjectID: in.UserID, SpaceRef: in.SpaceID,
					Revisions: AuthorityRevision{
						Authority: in.AuthorityRevision, Membership: in.MembershipRevision,
						Privacy: in.PrivacyRevision, RecipientAudience: in.RecipientAudienceRevision,
						Entitlement: in.EntitlementRevision,
					},
				},
				RecipientAudienceRef:     in.RecipientAudienceRef,
				RecipientAudienceHash:    in.RecipientAudienceHash,
				ResourceAuthorizationRef: in.ResourceAuthorizationRef,
				Privacy:                  PrivacyPolicySnapshot{PolicyRef: in.PrivacyPolicyRef},
			}
			request := BrowserRunDecisionRequest{IdempotencyKey: vector.IdempotencyKey}
			claims := BrowserRunClaims{
				ThreadID: vector.ThreadID, ThreadMode: vector.ThreadMode, GoalDigest: vector.GoalDigest,
				AllowedDomains: canonical, ProfileScope: BrowserProfileEphemeral,
				ViewScope: vector.ViewScope, MaxSteps: vector.MaxSteps, MaxRuntimeS: vector.MaxRuntimeS,
			}
			got := browserRunPayloadDigest(evidence, request, claims)
			if got != vector.PayloadDigest {
				t.Fatalf("browserRunPayloadDigest = %s, golden %s (change only on an intended wire change, on both sides)",
					got, vector.PayloadDigest)
			}
			byName[vector.Name] = got
		})
	}

	// allowed_domains is order-independent: room_owner_view and
	// domains_listed_reversed differ only in the order the caller listed the
	// domains and must share one digest.
	owner, reversed := byName["room_owner_view"], byName["domains_listed_reversed"]
	if owner == "" || reversed == "" {
		t.Fatal("expected both room_owner_view and domains_listed_reversed vectors in the golden")
	}
	if owner != reversed {
		t.Fatalf("room_owner_view digest %s != domains_listed_reversed digest %s", owner, reversed)
	}

	// The new-thread vector (thread_id "", thread_mode "new") must not collide
	// with the matching existing-thread vector.
	if newThread, ok := byName["new_thread"]; ok && newThread == owner {
		t.Fatal("a new-thread decision must not share its digest with an existing-thread decision")
	}
}
