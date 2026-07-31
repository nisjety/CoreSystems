package taskexec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

func detail(status, org, title, description, config string) taskDetail {
	d := taskDetail{status: status, orgID: org, title: title, description: description}
	if config != "" {
		d.config = json.RawMessage(config)
	}
	return d
}

func TestDispatchPlanUsesTaskIDAsRunID(t *testing.T) {
	// The run id must be a single NATS subject token so RUN_COMPLETED lands on
	// mp.v1.run.<id>.event where mp.v1.run.*.event consumers can see it.
	task := TaskRef{ID: "task_9f0c1a2b", OrgID: "org_1", Kind: "cron"}
	req, err := dispatchPlan(task, detail("running", "org_1", "Nightly sweep", "", ""), "")
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	if req.GetRunId() != task.ID {
		t.Fatalf("run_id = %q, want %q", req.GetRunId(), task.ID)
	}
	if strings.ContainsAny(req.GetRunId(), ". *>") {
		t.Fatalf("run_id %q is not a single NATS token", req.GetRunId())
	}
	if req.GetOrgId() != "org_1" {
		t.Fatalf("org_id = %q, want org_1", req.GetOrgId())
	}
	if req.GetWorkflowType() != DefaultWorkflowType {
		t.Fatalf("workflow_type = %q, want %q", req.GetWorkflowType(), DefaultWorkflowType)
	}
}

func TestDispatchPlanRefusesOrgMismatch(t *testing.T) {
	// A claim carrying a different org than the row is a containment failure, not
	// something to reconcile silently.
	task := TaskRef{ID: "task_1", OrgID: "org_attacker", Kind: "cron"}
	if _, err := dispatchPlan(task, detail("running", "org_victim", "t", "", ""), ""); err == nil {
		t.Fatal("expected an org-mismatch error")
	}
}

func TestDispatchPlanRefusesUntenantedTask(t *testing.T) {
	task := TaskRef{ID: "task_1", OrgID: "", Kind: "cron"}
	if _, err := dispatchPlan(task, detail("running", "", "t", "", ""), ""); err == nil {
		t.Fatal("a task with no org_id must not start")
	}
}

func TestDispatchPlanSkipsTasksNoLongerRunning(t *testing.T) {
	// Redelivery after somebody else completed or cancelled the task: nothing to
	// dispatch AND nothing to fail, or the executor would overwrite their state.
	for _, status := range []string{"created", "completed", "cancelled", "failed"} {
		req, err := dispatchPlan(
			TaskRef{ID: "task_1", OrgID: "org_1"},
			detail(status, "org_1", "t", "", ""), "")
		if err != nil {
			t.Fatalf("status %q: unexpected error %v", status, err)
		}
		if req != nil {
			t.Fatalf("status %q: expected no dispatch, got %+v", status, req)
		}
	}
}

func TestDispatchPlanRejectsMalformedTemplate(t *testing.T) {
	// A broken template must fail loudly, not quietly become a default run.
	_, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "t", "", `{"workflow_type":`), "")
	if err == nil {
		t.Fatal("malformed config_json must be an error")
	}
}

func TestDispatchPlanHonoursTemplateWorkflowTypeAndInput(t *testing.T) {
	cfg := `{"workflow_type":"WideResearchWorkflow","workflow_input":{"queries":["a","b"],"max_branches":2}}`
	req, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "ignored title", "", cfg), "")
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	if req.GetWorkflowType() != "WideResearchWorkflow" {
		t.Fatalf("workflow_type = %q", req.GetWorkflowType())
	}
	queries := req.GetInput().GetFields()["queries"].GetListValue().GetValues()
	if len(queries) != 2 || queries[0].GetStringValue() != "a" {
		t.Fatalf("explicit workflow_input was not passed through: %v", req.GetInput().AsMap())
	}
	if _, ok := req.GetInput().GetFields()["goal"]; ok {
		t.Fatal("an explicit workflow_input must not be merged with a derived goal")
	}
}

func TestDispatchPlanDerivesGoalFromTitleAndDescription(t *testing.T) {
	req, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "Summarise inbox", "Only unread since yesterday", ""), "")
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	goal := req.GetInput().GetFields()["goal"].GetStringValue()
	if !strings.Contains(goal, "Summarise inbox") || !strings.Contains(goal, "Only unread since yesterday") {
		t.Fatalf("goal = %q", goal)
	}
}

func TestDispatchPlanPassesPolicyThrough(t *testing.T) {
	req, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "do it", "", `{"policy":"ask"}`), "")
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	if got := req.GetInput().GetFields()["policy"].GetStringValue(); got != "ask" {
		t.Fatalf("policy = %q, want ask", got)
	}
}

func TestDispatchPlanRefusesAGoallessTask(t *testing.T) {
	// No title, no description, no explicit input: there is no honest goal to
	// invent, so refuse rather than start an empty run.
	if _, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "", "", ""), ""); err == nil {
		t.Fatal("a task with nothing to do must not start a run")
	}
}

func TestDispatchPlanRespectsConfiguredDefaultType(t *testing.T) {
	req, err := dispatchPlan(
		TaskRef{ID: "task_1", OrgID: "org_1"},
		detail("running", "org_1", "do it", "", ""), "EvaluatorOptimizerWorkflow")
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	if req.GetWorkflowType() != "EvaluatorOptimizerWorkflow" {
		t.Fatalf("workflow_type = %q", req.GetWorkflowType())
	}
}

func TestNewWorkflowDispatcherRequiresACredential(t *testing.T) {
	// Without ANY credential every StartWorkflow would be refused, so the
	// dispatcher must refuse to exist rather than fail every task at runtime.
	if _, err := NewWorkflowDispatcher(nil, nil, nil, "", nil, ""); err == nil {
		t.Fatal("expected a construction error with no pool/client/credential")
	}
	if _, err := NewWorkflowDispatcher(nil, stubStarter{}, nil, "tok", nil, ""); err == nil {
		t.Fatal("expected a construction error with no pool")
	}
}

// A minter alone is sufficient: the shared secret is the fallback, not a
// requirement, and a deployment that has moved to minted JWTs should not be
// forced to keep a static secret around.
func TestNewWorkflowDispatcherAcceptsAMinterWithoutTheSharedSecret(t *testing.T) {
	_, err := NewWorkflowDispatcher(nil, stubStarter{}, stubMinter{token: "t"}, "", nil, "")
	if err == nil {
		t.Fatal("expected the no-pool error, proving the credential check passed")
	}
	if !strings.Contains(err.Error(), "database pool") {
		t.Fatalf("err = %v, want the pool error rather than a credential error", err)
	}
}

// The decisive rule. orchestrator-core checks the internal metadata key FIRST and
// returns immediately when present, so a request carrying both credentials is
// still authenticated as the internal caller — which has no signed retention
// posture and is refused for every workflow that persists derived content. The
// bearer would be silently ignored and the failure would read as a scope problem.
func TestAuthorizeSendsTheBearerAloneAndNeverTheInternalKey(t *testing.T) {
	d := &WorkflowDispatcher{minter: stubMinter{token: "minted"}, internalToken: "shared"}
	md, ok := metadata.FromOutgoingContext(d.authorize(context.Background(), "org-1"))
	if !ok {
		t.Fatal("no outgoing metadata")
	}
	if got := md.Get("authorization"); len(got) != 1 || got[0] != "Bearer minted" {
		t.Fatalf("authorization = %v, want [Bearer minted]", got)
	}
	if got := md.Get(InternalTokenMetadataKey); len(got) != 0 {
		t.Fatalf("%s = %v, must be absent or the bearer is ignored", InternalTokenMetadataKey, got)
	}
}

// The token is minted per tenant, so the org must reach the minter — a token for
// the wrong org authenticates and is then refused on every call.
func TestAuthorizeMintsForTheTaskOrg(t *testing.T) {
	minter := &recordingMinter{token: "t"}
	d := &WorkflowDispatcher{minter: minter, internalToken: "shared"}
	d.authorize(context.Background(), "org-42")
	if minter.org != "org-42" {
		t.Fatalf("minted for %q, want org-42", minter.org)
	}
}

// A mint failure degrades to the shared secret rather than failing the dispatch:
// run-scoped workflows work under either credential, so starting those beats
// starting nothing. Only the retention-gated ones are lost.
func TestAuthorizeFallsBackToTheSharedSecretWhenMintingFails(t *testing.T) {
	d := &WorkflowDispatcher{
		minter:        stubMinter{err: errors.New("issuer down")},
		internalToken: "shared",
	}
	md, _ := metadata.FromOutgoingContext(d.authorize(context.Background(), "org-1"))
	if got := md.Get(InternalTokenMetadataKey); len(got) != 1 || got[0] != "shared" {
		t.Fatalf("%s = %v, want the shared secret fallback", InternalTokenMetadataKey, got)
	}
	if got := md.Get("authorization"); len(got) != 0 {
		t.Fatalf("authorization = %v, must be absent when minting failed", got)
	}
}

// With no minter at all the behaviour is exactly what it was before this change.
func TestAuthorizeWithoutAMinterSendsTheInternalKey(t *testing.T) {
	d := &WorkflowDispatcher{internalToken: "shared"}
	md, _ := metadata.FromOutgoingContext(d.authorize(context.Background(), "org-1"))
	if got := md.Get(InternalTokenMetadataKey); len(got) != 1 || got[0] != "shared" {
		t.Fatalf("%s = %v, want the unchanged internal path", InternalTokenMetadataKey, got)
	}
}

// An empty minted token must not be presented as a credential.
func TestAuthorizeTreatsABlankMintedTokenAsAFailure(t *testing.T) {
	d := &WorkflowDispatcher{minter: stubMinter{token: "   "}, internalToken: "shared"}
	md, _ := metadata.FromOutgoingContext(d.authorize(context.Background(), "org-1"))
	if got := md.Get(InternalTokenMetadataKey); len(got) != 1 {
		t.Fatalf("want the shared-secret fallback for a blank token, got %v", got)
	}
}

// PolicyGlobalRegistry requires BOTH start scopes, so requesting only the base
// one would leave SkillPromotion and FeedbackPromotion refused.
func TestOrchestratorScopesCoverTheRegistryWideWorkflows(t *testing.T) {
	want := map[string]bool{
		"orchestration:workflow:start":        false,
		"orchestration:workflow:start:global": false,
	}
	for _, s := range WorkflowStartScopes {
		if _, ok := want[s]; ok {
			want[s] = true
		}
	}
	for scope, found := range want {
		if !found {
			t.Fatalf("missing scope %q", scope)
		}
	}
}

type stubMinter struct {
	token string
	err   error
}

func (s stubMinter) Token(context.Context, string) (string, error) { return s.token, s.err }

type recordingMinter struct {
	token string
	org   string
}

func (r *recordingMinter) Token(_ context.Context, org string) (string, error) {
	r.org = org
	return r.token, nil
}

type stubStarter struct{}

func (stubStarter) StartWorkflow(
	_ context.Context,
	_ *mpv1.StartWorkflowRequest,
	_ ...grpc.CallOption,
) (*mpv1.StartWorkflowResponse, error) {
	return &mpv1.StartWorkflowResponse{}, nil
}
