package taskexec

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/types/known/structpb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

func detail(status, org, title, description, config string) taskDetail {
	d := taskDetail{status: status, orgID: org, title: title, description: description}
	if config != "" {
		d.config = json.RawMessage(config)
	}
	return d
}

func scheduledTaskConfig(t *testing.T, template map[string]any, intent cron.FireIntent) (json.RawMessage, cron.FireIntent) {
	t.Helper()
	templateJSON, err := json.Marshal(template)
	if err != nil {
		t.Fatalf("marshal scheduled template: %v", err)
	}
	intent.TemplateDigest = fmt.Sprintf("sha256:%x", sha256.Sum256(templateJSON))
	template["schedule_fire_intent"] = intent
	config, err := json.Marshal(template)
	if err != nil {
		t.Fatalf("marshal scheduled task config: %v", err)
	}
	return config, intent
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

func TestWorkflowDispatchLoadFenceRefusesDeletedCronSchedule(t *testing.T) {
	source, err := os.ReadFile(filepath.Join("workflow_dispatcher.go"))
	if err != nil {
		t.Fatalf("read workflow dispatcher source: %v", err)
	}
	for _, required := range []string{
		"FROM cron_fires AS cf",
		"JOIN cron_schedules AS cs ON cs.id = cf.schedule_id",
		"cf.task_id = t.id AND cs.deleted_at IS NOT NULL",
	} {
		if !strings.Contains(string(source), required) {
			t.Fatalf("workflow handoff must retain deleted-cron fence %q", required)
		}
	}
}

type recordingFireAuthorizer struct {
	intent cron.FireIntent
	err    error
	calls  int
}

func (a *recordingFireAuthorizer) AuthorizeFire(_ context.Context, intent cron.FireIntent) error {
	a.calls++
	a.intent = intent
	return a.err
}

type recordingScheduledRunAuthorizer struct {
	recordingFireAuthorizer
	preparation cron.ScheduledRunPreparation
	taskID      string
}

func (a *recordingScheduledRunAuthorizer) AuthorizeScheduledRun(_ context.Context, _ cron.FireIntent, taskID string) (cron.ScheduledRunPreparation, error) {
	a.taskID = taskID
	return a.preparation, nil
}

type recordingScheduledRunSession struct {
	request  *mpv1.PrepareScheduledRunThreadRequest
	response *mpv1.PrepareScheduledRunThreadResponse
}

func (s *recordingScheduledRunSession) PrepareScheduledRunThread(_ context.Context, req *mpv1.PrepareScheduledRunThreadRequest, _ ...grpc.CallOption) (*mpv1.PrepareScheduledRunThreadResponse, error) {
	s.request = req
	return s.response, nil
}

func TestWorkflowDispatchReauthorizesOnlyBoundCronIntent(t *testing.T) {
	intent := cron.FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey:        "2026-08-13T00:00:00Z",
		IdempotencyKey: "schedule-1:2026-08-13T00:00:00Z",
	}
	config, intent := scheduledTaskConfig(t, map[string]any{
		"workflow_input": map[string]any{"goal": "scheduled work", "policy": "execute"},
	}, intent)
	authorizer := &recordingFireAuthorizer{}
	dispatcher := &WorkflowDispatcher{fireAuthorizer: authorizer}
	detail := taskDetail{config: config, scheduleID: "schedule-1"}
	task := TaskRef{ID: "task-1", OrgID: "org-1"}
	if _, err := dispatcher.reauthorizeScheduleFire(context.Background(), task, detail); err == nil {
		t.Fatal("bound cron intent without scheduled-run preparation must fail closed")
	}
	if authorizer.intent.ScheduleID != "schedule-1" {
		t.Fatalf("authorizer received %+v", authorizer.intent)
	}

	for _, altered := range []taskDetail{
		{config: config, scheduleID: "schedule-other"},
		{config: config, scheduleID: "schedule-1"},
	} {
		if altered.scheduleID == "schedule-1" {
			if _, err := (&WorkflowDispatcher{}).reauthorizeScheduleFire(context.Background(), task, altered); err == nil {
				t.Fatal("cron intent without a fresh authorizer must fail closed")
			}
			continue
		}
		if _, err := dispatcher.reauthorizeScheduleFire(context.Background(), task, altered); err == nil {
			t.Fatal("schedule mismatch must fail closed")
		}
	}
}

func TestScheduledRunRejectsDetachedTemplateBeforeControlReauthorization(t *testing.T) {
	// The Control decision's template digest is a commitment, not merely a
	// label. A task whose executable template no longer hashes to that value
	// must fail before it can obtain a fresh fire or preparation decision.
	intent := cron.FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "2026-08-15T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("f", 64),
		IdempotencyKey: "schedule-1:2026-08-15T00:00:00Z",
	}
	config, err := json.Marshal(map[string]any{
		"workflow_type": "InteractiveRunSupervision",
		"workflow_input": map[string]any{
			"goal":   "an attacker-substituted goal",
			"policy": "execute",
		},
		"schedule_fire_intent": intent,
	})
	if err != nil {
		t.Fatalf("marshal detached task template: %v", err)
	}
	authorizer := &recordingFireAuthorizer{}
	dispatcher := &WorkflowDispatcher{fireAuthorizer: authorizer}
	_, err = dispatcher.reauthorizeScheduleFire(context.Background(), TaskRef{ID: "task-1", OrgID: "org-1"}, taskDetail{
		config: config, scheduleID: "schedule-1",
	})
	if err == nil {
		t.Fatal("detached schedule template reached reauthorization")
	}
	if authorizer.calls != 0 {
		t.Fatalf("detached schedule template called Control %d times", authorizer.calls)
	}
}

func TestWorkflowDispatchConsumesDecisionBeforeTemporalHandoff(t *testing.T) {
	intent := cron.FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey:        "2026-08-14T00:00:00Z",
		IdempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
	}
	config, intent := scheduledTaskConfig(t, map[string]any{
		"workflow_input": map[string]any{"goal": "scheduled work", "policy": "execute"},
	}, intent)
	authorizer := &recordingScheduledRunAuthorizer{preparation: cron.ScheduledRunPreparation{
		Token: "one-fire-control-bearer", SystemThreadKey: "schedule/schedule-1/2026-08-14T00:00:00Z",
	}}
	session := &recordingScheduledRunSession{response: &mpv1.PrepareScheduledRunThreadResponse{
		ThreadId: "thread-scheduled-1", RunId: "task-1", OwnerId: "service:orchestrator-core",
	}}
	dispatcher := &WorkflowDispatcher{fireAuthorizer: authorizer, scheduledRunSession: session}
	prepared, err := dispatcher.reauthorizeScheduleFire(context.Background(), TaskRef{ID: "task-1", OrgID: "org-1"}, taskDetail{
		config: config, scheduleID: "schedule-1",
	})
	if err != nil {
		t.Fatalf("reauthorizeScheduleFire: %v", err)
	}
	if authorizer.taskID != "task-1" || session.request.GetControlDecisionToken() != "one-fire-control-bearer" {
		t.Fatalf("Control decision was not consumed only by Session preparation: task=%q request=%+v", authorizer.taskID, session.request)
	}
	if prepared == nil || prepared.threadID != "thread-scheduled-1" || prepared.spaceRef != intent.SpaceRef ||
		prepared.subjectID != intent.SubjectID || prepared.scheduleID != "schedule-1" ||
		prepared.fireKey != intent.FireKey || prepared.templateDigest != intent.TemplateDigest ||
		prepared.templateJSON == "" ||
		prepared.idempotencyKey != intent.IdempotencyKey {
		t.Fatalf("prepared non-secret handoff = %#v", prepared)
	}
}

func TestAttachScheduledRunInputCarriesOnlyNonSecretPreparationFacts(t *testing.T) {
	req, err := dispatchPlan(
		TaskRef{ID: "task-1", OrgID: "org-1"},
		detail("running", "org-1", "Nightly sweep", "", ""),
		"",
	)
	if err != nil {
		t.Fatalf("dispatchPlan: %v", err)
	}
	// A stale/wrong template must never be able to copy the one-fire authority
	// into the durable workflow input.
	req.Input.Fields["schedule_fire_intent"] = structpb.NewStringValue("secret")
	req.Input.Fields["control_decision_token"] = structpb.NewStringValue("secret")
	req.Input.Fields["payload_digest"] = structpb.NewStringValue("secret")

	if err := attachScheduledRunInput(req, preparedScheduledRun{
		threadID: "thread-scheduled-1", spaceRef: "space-1", subjectID: "user-1",
		scheduleID: "schedule-1", fireKey: "2026-08-14T00:00:00Z",
		templateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		templateJSON:   `{"workflow_input":{"goal":"Nightly sweep"}}`,
		idempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
	}, "task-1"); err != nil {
		t.Fatalf("attachScheduledRunInput: %v", err)
	}

	input := req.GetInput().AsMap()
	if got := input["thread_id"]; got != "thread-scheduled-1" {
		t.Fatalf("thread_id = %#v, want prepared scheduled-run thread", got)
	}
	if got := input["schedule_id"]; got != "schedule-1" {
		t.Fatalf("schedule_id = %#v", got)
	}
	if got := input["fire_key"]; got != "2026-08-14T00:00:00Z" {
		t.Fatalf("fire_key = %#v", got)
	}
	for field, want := range map[string]string{
		"space_ref": "space-1", "subject_id": "user-1",
		"template_digest":    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"task_template_json": `{"workflow_input":{"goal":"Nightly sweep"}}`,
		"idempotency_key":    "schedule-1:2026-08-14T00:00:00Z",
	} {
		if got := input[field]; got != want {
			t.Fatalf("%s = %#v, want %q", field, got, want)
		}
	}
	for _, forbidden := range []string{
		"schedule_fire_intent", "control_decision_token", "decision", "token", "payload_digest",
		"authority_revision", "recipient_audience_hash", "resource_authorization_ref",
		"goal", "policy",
	} {
		if _, found := input[forbidden]; found {
			t.Fatalf("scheduled workflow input leaked %q: %#v", forbidden, input)
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
