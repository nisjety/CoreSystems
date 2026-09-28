package policy

import (
	"testing"
	"time"
)

func fixture() (VerifiedAccess, AssistanceGrant, time.Time) {
	now := time.Date(2026, 9, 24, 10, 0, 0, 0, time.UTC)
	scope := VerifiedAccess{OrganizationID: "school-a", GroupID: "class-a", TeacherID: "teacher-a", LearnerID: "learner-a", DeviceID: "device-a", LocalSessionID: "desktop-a", AuthorityVerified: true, DeviceAccepted: true}
	return scope, AssistanceGrant{Scope: scope, IssuedAt: now.Add(-time.Minute), ExpiresAt: now.Add(5 * time.Minute), Permissions: []AssistanceAction{View}}, now
}

func TestAssistanceBoundaries(t *testing.T) {
	cases := []struct {
		name    string
		mutate  func(*VerifiedAccess, *AssistanceGrant)
		action  AssistanceAction
		allowed bool
	}{
		{"scoped-view", func(a *VerifiedAccess, g *AssistanceGrant) {}, View, true},
		{"view-is-not-control", func(a *VerifiedAccess, g *AssistanceGrant) {}, Pointer, false},
		{"view-is-not-ai-consent", func(a *VerifiedAccess, g *AssistanceGrant) {}, ObserveAI, false},
		{"explicit-input", func(a *VerifiedAccess, g *AssistanceGrant) { g.Permissions = append(g.Permissions, Keyboard) }, Keyboard, true},
		{"other-school", func(a *VerifiedAccess, g *AssistanceGrant) { a.OrganizationID = "school-b" }, View, false},
		{"other-class", func(a *VerifiedAccess, g *AssistanceGrant) { a.GroupID = "class-b" }, View, false},
		{"other-teacher", func(a *VerifiedAccess, g *AssistanceGrant) { a.TeacherID = "teacher-b" }, View, false},
		{"other-learner", func(a *VerifiedAccess, g *AssistanceGrant) { a.LearnerID = "learner-b" }, View, false},
		{"other-device", func(a *VerifiedAccess, g *AssistanceGrant) { a.DeviceID = "device-b" }, View, false},
		{"other-windows-session", func(a *VerifiedAccess, g *AssistanceGrant) { a.LocalSessionID = "desktop-b" }, View, false},
		{"missing-scope", func(a *VerifiedAccess, g *AssistanceGrant) { a.DeviceID = ""; g.Scope.DeviceID = "" }, View, false},
		{"authority-outage", func(a *VerifiedAccess, g *AssistanceGrant) { a.AuthorityVerified = false }, View, false},
		{"unsigned-grant", func(a *VerifiedAccess, g *AssistanceGrant) { g.Scope.AuthorityVerified = false }, View, false},
		{"device-refuses", func(a *VerifiedAccess, g *AssistanceGrant) { a.DeviceAccepted = false }, View, false},
		{"revoked", func(a *VerifiedAccess, g *AssistanceGrant) { g.Revoked = true }, View, false},
		{"future-grant", func(a *VerifiedAccess, g *AssistanceGrant) { g.IssuedAt = g.IssuedAt.Add(2 * time.Minute) }, View, false},
		{"expired-at-boundary", func(a *VerifiedAccess, g *AssistanceGrant) { g.ExpiresAt = g.IssuedAt.Add(time.Minute) }, View, false},
		{"unbounded-grant", func(a *VerifiedAccess, g *AssistanceGrant) { g.ExpiresAt = g.IssuedAt.Add(time.Hour) }, View, false},
		{"login-not-teaching", func(a *VerifiedAccess, g *AssistanceGrant) { g.Permissions = append(g.Permissions, "windows.logon") }, "windows.logon", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			access, grant, now := fixture()
			tc.mutate(&access, &grant)
			if got := CheckAssistance(access, grant, tc.action, now) == nil; got != tc.allowed {
				t.Fatalf("allowed=%v, want %v", got, tc.allowed)
			}
		})
	}
}

func TestStrictProcessing(t *testing.T) {
	for _, tc := range []struct {
		name    string
		p       Processor
		allowed bool
	}{
		{"private-whisper", Processor{"self-hosted", true, false}, true},
		{"cloud-even-if-eu", Processor{"cloud", true, false}, false},
		{"unknown-location", Processor{"self-hosted", false, false}, false},
		{"hidden-cloud-fallback", Processor{"self-hosted", true, true}, false},
		{"unconfigured", Processor{}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := CheckStrictProcessing(tc.p) == nil; got != tc.allowed {
				t.Fatalf("allowed=%v, want %v", got, tc.allowed)
			}
		})
	}
}
