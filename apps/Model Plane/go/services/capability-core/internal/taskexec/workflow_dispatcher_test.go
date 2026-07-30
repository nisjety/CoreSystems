package taskexec

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"google.golang.org/grpc"

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
	// Without the internal token every StartWorkflow would be refused, so the
	// dispatcher must refuse to exist rather than fail every task at runtime.
	if _, err := NewWorkflowDispatcher(nil, nil, "", nil, ""); err == nil {
		t.Fatal("expected a construction error with no pool/client/token")
	}
	if _, err := NewWorkflowDispatcher(nil, stubStarter{}, "tok", nil, ""); err == nil {
		t.Fatal("expected a construction error with no pool")
	}
}

type stubStarter struct{}

func (stubStarter) StartWorkflow(
	_ context.Context,
	_ *mpv1.StartWorkflowRequest,
	_ ...grpc.CallOption,
) (*mpv1.StartWorkflowResponse, error) {
	return &mpv1.StartWorkflowResponse{}, nil
}
