package api

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// O-21: with ORG_COMPLIANCE_ROLE_ENABLED the literal "true", a compliance member
// reads the whole org audit trail and the compliance API (events, manifest, keys,
// export status, verify); usage stays owner and admin only. Off, the role lists
// and the 403 text are exactly what they were.

const (
	offComplianceMessage = "the audit compliance API is limited to organization owners and admins"
	onComplianceMessage  = "the audit compliance API is limited to organization owners, admins and compliance members"
)

var complianceReadTargets = []string{
	complianceEventsURL,
	"/v1/compliance/audit/manifest?org_id=org-a",
	"/v1/compliance/audit/keys?org_id=org-a",
	"/v1/compliance/audit/export-status?org_id=org-a",
}

func setComplianceRoleSwitch(t *testing.T, value string) {
	t.Helper()
	t.Setenv(envComplianceRoleEnabled, value)
}

func envelopeErrorMessage(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	failure, _ := decodeEnvelope(t, recorder)["error"].(map[string]any)
	message, _ := failure["message"].(string)
	return message
}

func TestComplianceRoleSwitchReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{
		"true": true, "": false, "1": false, "TRUE": false, "True": false, " true": false, "yes": false, "false": false,
	} {
		getenv := func(string) string { return value }
		if got := complianceRoleEnabled(getenv); got != want {
			t.Fatalf("complianceRoleEnabled(%q) = %t, want %t", value, got, want)
		}
	}
	setComplianceRoleSwitch(t, "true")
	if handler, err := New(&complianceStub{}, complianceTestRegistry()); err != nil || !handler.complianceRoleOn {
		t.Fatalf("New must read the switch: err=%v", err)
	}
	setComplianceRoleSwitch(t, "1")
	if handler, err := New(&complianceStub{}, complianceTestRegistry()); err != nil || handler.complianceRoleOn {
		t.Fatalf("New must leave a non-literal value off: err=%v", err)
	}
}

func TestIsAuditReaderRole(t *testing.T) {
	for role, want := range map[string][2]bool{ // {off, on}
		"owner": {true, true}, "admin": {true, true}, " Admin ": {true, true},
		"compliance": {false, true}, " Compliance ": {false, true},
		"member": {false, false}, "viewer": {false, false}, "": {false, false},
		"compliance,member": {false, false}, "superadmin": {false, false},
	} {
		if got := isAuditReaderRole(role, false); got != want[0] {
			t.Fatalf("off: isAuditReaderRole(%q) = %t, want %t", role, got, want[0])
		}
		if got := isAuditReaderRole(role, true); got != want[1] {
			t.Fatalf("on: isAuditReaderRole(%q) = %t, want %t", role, got, want[1])
		}
	}
}

func TestComplianceRoutesRefuseComplianceWhileTheSwitchIsOffWithTheOriginalMessage(t *testing.T) {
	for _, value := range []string{"", "false", "1", "TRUE"} {
		setComplianceRoleSwitch(t, value)
		for _, target := range complianceReadTargets {
			stub := &complianceStub{}
			recorder := complianceGet(complianceRouter(t, stub), target, "org-a", "compliance")
			if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "compliance_role_required" {
				t.Fatalf("switch %q %s: status %d body %s", value, target, recorder.Code, recorder.Body.String())
			}
			if got := envelopeErrorMessage(t, recorder); got != offComplianceMessage {
				t.Fatalf("switch %q %s: message %q changed, want %q", value, target, got, offComplianceMessage)
			}
			if stub.reads != 0 || len(stub.audit) != 0 {
				t.Fatalf("switch %q %s: refused request reached the store", value, target)
			}
		}
	}
}

func TestComplianceRoutesServeAComplianceMemberWhileTheSwitchIsOn(t *testing.T) {
	setComplianceRoleSwitch(t, "true")
	for _, target := range complianceReadTargets {
		stub := &complianceStub{}
		recorder := complianceGet(complianceRouter(t, stub), target, "org-a", "compliance")
		if recorder.Code != http.StatusOK {
			t.Fatalf("%s as compliance: status %d body %s", target, recorder.Code, recorder.Body.String())
		}
		if len(stub.audit) != 1 {
			t.Fatalf("%s as compliance: the read must still write its self-audit row, got %d", target, len(stub.audit))
		}
	}

	stub := &complianceStub{}
	router := complianceRouter(t, stub)
	request := httptest.NewRequest(http.MethodPost, "/v1/compliance/audit/verify?org_id=org-a", strings.NewReader(`{}`))
	signComplianceRequest(request, "org-a", "compliance", []byte(`{}`))
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusOK || len(stub.verified) != 1 {
		t.Fatalf("compliance verify: status %d (%s), verified %d times", recorder.Code, recorder.Body.String(), len(stub.verified))
	}
}

func TestComplianceRoutesStillRefuseMembersAndViewersWhileTheSwitchIsOn(t *testing.T) {
	setComplianceRoleSwitch(t, "true")
	for _, target := range complianceReadTargets {
		for _, role := range []string{"member", "viewer", "billing"} {
			stub := &complianceStub{}
			recorder := complianceGet(complianceRouter(t, stub), target, "org-a", role)
			if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "compliance_role_required" {
				t.Fatalf("%s as %q: status %d body %s", target, role, recorder.Code, recorder.Body.String())
			}
			if got := envelopeErrorMessage(t, recorder); got != onComplianceMessage {
				t.Fatalf("%s as %q: message %q, want %q", target, role, got, onComplianceMessage)
			}
			if stub.reads != 0 || len(stub.audit) != 0 {
				t.Fatalf("%s as %q reached the store", target, role)
			}
		}
	}
}

func TestAuditListScopesAComplianceMemberByTheSwitch(t *testing.T) {
	setReadMode(t, "", "")

	// Off: compliance is not a reader role, so it is narrowed to its own rows.
	setComplianceRoleSwitch(t, "")
	stub := &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "user-a"}}}
	recorder := scopedGet(scopeRouter(t, stub), auditURL, "user-a", "compliance")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeSelf {
		t.Fatalf("off: status %d scope %q", recorder.Code, metaScope(t, recorder))
	}
	if len(stub.auditFilters) != 1 || stub.auditFilters[0].UserID != "user-a" {
		t.Fatalf("off: compliance must be pinned to the caller: %+v", stub.auditFilters)
	}

	// On: the whole org trail, any user_id.
	setComplianceRoleSwitch(t, "true")
	stub = &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "someone-else", Event: "login"}}}
	recorder = scopedGet(scopeRouter(t, stub), auditURL+"&user_id=user-z", "user-a", "compliance")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeOrg {
		t.Fatalf("on: status %d scope %q body %s", recorder.Code, metaScope(t, recorder), recorder.Body.String())
	}
	if len(stub.auditFilters) != 1 || stub.auditFilters[0].UserID != "user-z" {
		t.Fatalf("on: compliance may ask for any user: %+v", stub.auditFilters)
	}

	// On: a member is still narrowed.
	stub = &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "user-a"}}}
	recorder = scopedGet(scopeRouter(t, stub), auditURL, "user-a", "member")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeSelf {
		t.Fatalf("on, member: status %d scope %q", recorder.Code, metaScope(t, recorder))
	}
}

func TestUsageReadsStayOwnerAndAdminOnlyWhileTheSwitchIsOn(t *testing.T) {
	setReadMode(t, "", "")
	setComplianceRoleSwitch(t, "true")
	for _, target := range []string{"/v1/usage?org_id=org-a", "/v1/usage/summary?org_id=org-a"} {
		stub := &scopeStub{}
		recorder := scopedGet(scopeRouter(t, stub), target, "user-a", "compliance")
		if recorder.Code != http.StatusForbidden {
			t.Fatalf("%s as compliance: status %d body %s", target, recorder.Code, recorder.Body.String())
		}
		if stub.usageCalls != 0 || stub.summaryCalls != 0 {
			t.Fatalf("%s as compliance reached the usage store", target)
		}
	}
}
