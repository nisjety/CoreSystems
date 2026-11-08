package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS09-S7: POST /api/v1/internal/spaces/run-control-decision (pure tier). The
// 403 and 200 legs need a database and live in run_control_decision_live_test.go.

const runControlPath = "/api/v1/internal/spaces/run-control-decision"

func validRunControlBody() map[string]any {
	return map[string]any{
		"space_ref": "room-1", "run_id": "run-1", "operation": "cancel", "idempotency_key": "control-1",
	}
}

func TestRunControlRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+runControlPath] != 1 {
		t.Fatalf("run-control-decision registered %d times, want 1", counts["POST "+runControlPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, runControlPath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
}

func TestRunControlRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"other service":       {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			rec := postJSON(engine, runControlPath, validRunControlBody())
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestRunControlRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceIssuerHarness(t, nil, gatewayContext())
	rec := postJSON(engine, runControlPath, validRunControlBody())
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

func TestRunControlRouteRejectsMissingFieldsAndUnknownOperations(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for _, field := range []string{"space_ref", "run_id", "operation", "idempotency_key"} {
		t.Run("missing "+field, func(t *testing.T) {
			body := validRunControlBody()
			delete(body, field)
			rec := postJSON(engine, runControlPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
	for _, operation := range []string{"pause", "resume", "kill", "CANCEL", "cancel "} {
		t.Run("operation "+operation, func(t *testing.T) {
			body := validRunControlBody()
			body["operation"] = operation
			rec := postJSON(engine, runControlPath, body)
			if operation == "cancel " {
				// Trailing whitespace is trimmed before the check, as in the issuer.
				if rec.Code == http.StatusBadRequest {
					t.Fatalf("a padded cancel is still cancel: %d %s", rec.Code, rec.Body.String())
				}
				return
			}
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), "token") {
				t.Fatalf("a 400 must not carry a token: %s", rec.Body.String())
			}
		})
	}
	body := validRunControlBody()
	body["run_id"] = strings.Repeat("r", 129)
	if rec := postJSON(engine, runControlPath, body); rec.Code != http.StatusBadRequest {
		t.Fatalf("oversized run id: status = %d, want 400", rec.Code)
	}
}

func TestRunControlDenialIsTheFixedStringForEditorAndEntitlementOff(t *testing.T) {
	const message = "Space run control is not authorized"
	evidence := validRunControlHTTPEvidence()

	// Editor: below the manager floor -> typed role denial.
	evidence.Membership.Role = "editor"
	_, err := spaces.IssueSharedRunControlDecision(evidence, validRunControlIssuerRequest(), time.Now().UTC())
	assertFixedRunControlDenial(t, err, message, string(spaces.DenialRoleBelowFloor))

	// Entitlement off -> typed entitlement denial.
	evidence = validRunControlHTTPEvidence()
	evidence.RunControlEntitled = false
	_, err = spaces.IssueSharedRunControlDecision(evidence, validRunControlIssuerRequest(), time.Now().UTC())
	assertFixedRunControlDenial(t, err, message, string(spaces.DenialEntitlementOff))

	// The handler's constant is the string the BFF maps.
	if runControlDeniedMessage != message {
		t.Fatalf("runControlDeniedMessage = %q, want %q", runControlDeniedMessage, message)
	}
}

func validRunControlHTTPEvidence() spaces.PersonalThreadDecisionEvidence {
	return spaces.PersonalThreadDecisionEvidence{
		Membership: spaces.CurrentMembership{
			SpaceRef: "room-1", OrgID: "org-1", SubjectID: "user-1", Kind: spaces.KindRoom, Role: "manager",
			Revisions: spaces.AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 2, Entitlement: 3},
		},
		RecipientAudienceRef: "audience:room-1:2", RecipientAudienceHash: "sha256:a",
		RunControlEntitled: true, ResourceAuthorizationRef: "control:room-1:run-control:7",
		Privacy: spaces.PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
}

func validRunControlIssuerRequest() spaces.RunControlDecisionRequest {
	return spaces.RunControlDecisionRequest{
		DecisionRef: "d", RunID: "run-1", Operation: "cancel", IdempotencyKey: "k", Nonce: "n",
	}
}

func assertFixedRunControlDenial(t *testing.T, err error, message, code string) {
	t.Helper()
	if err == nil {
		t.Fatal("expected a refusal")
	}
	rec := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(rec)
	writeSpaceIssuerDenial(c, err, message)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403", rec.Code)
	}
	body := rec.Body.String()
	if !strings.Contains(body, `"error":"`+message+`"`) || !strings.Contains(body, `"code":"`+code+`"`) {
		t.Fatalf("body = %s", body)
	}
}
