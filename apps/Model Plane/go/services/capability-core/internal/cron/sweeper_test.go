package cron

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

// taskConfigJSON is what carries a schedule's workflow selection into the task
// the sweeper creates. taskexec's dispatchPlan reads tasks.config_json to pick
// the workflow type, so a regression here silently pins every scheduled run to
// the default workflow — which is exactly the bug this function fixed.
func TestTaskConfigJSON(t *testing.T) {
	tests := []struct {
		name     string
		template string
		// wantWorkflowType is what dispatchPlan would read back; empty means it
		// would fall through to the default workflow.
		wantWorkflowType string
		wantObject       bool
	}{
		{
			name:             "workflow type survives",
			template:         `{"kind":"cron","title":"Nightly","workflow_type":"DeepTaskWorkflow"}`,
			wantWorkflowType: "DeepTaskWorkflow",
			wantObject:       true,
		},
		{
			name:             "workflow input survives alongside the type",
			template:         `{"workflow_type":"WideResearchWorkflow","workflow_input":{"queries":["a","b"]}}`,
			wantWorkflowType: "WideResearchWorkflow",
			wantObject:       true,
		},
		{
			name:             "template without workflow fields is still a valid object",
			template:         `{"kind":"cron","title":"Plain"}`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "absent template yields an empty object, never invalid JSON",
			template:         ``,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "whitespace-only template yields an empty object",
			template:         "  \n\t ",
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "json null yields an empty object",
			template:         `null`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			// An array would make dispatchPlan's unmarshal fail and turn every
			// fire into a failed task, so it must be normalized away here.
			name:             "array template is rejected rather than passed through",
			template:         `[{"workflow_type":"DeepTaskWorkflow"}]`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "scalar template is rejected rather than passed through",
			template:         `"DeepTaskWorkflow"`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "malformed json is rejected rather than passed through",
			template:         `{"workflow_type":`,
			wantWorkflowType: "",
			wantObject:       true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := taskConfigJSON([]byte(tc.template), nil)

			// Mirror dispatchPlan exactly: plain Unmarshal into the dispatch
			// template. If this errors, the dispatcher would fail the task.
			var tpl struct {
				WorkflowType  string          `json:"workflow_type"`
				WorkflowInput json.RawMessage `json:"workflow_input"`
				Policy        string          `json:"policy"`
			}
			if err := json.Unmarshal(got, &tpl); err != nil {
				t.Fatalf("dispatchPlan would fail to unmarshal config_json %q: %v", got, err)
			}
			if tpl.WorkflowType != tc.wantWorkflowType {
				t.Errorf("workflow_type = %q, want %q (config_json was %q)",
					tpl.WorkflowType, tc.wantWorkflowType, got)
			}

			if tc.wantObject {
				var probe map[string]json.RawMessage
				if err := json.Unmarshal(got, &probe); err != nil {
					t.Errorf("config_json %q is not a JSON object: %v", got, err)
				}
			}
		})
	}
}

func TestSweeperFailsClosedWithoutFreshFireAuthorizer(t *testing.T) {
	_, err := NewSweeper(nil).RunOnce(context.Background())
	if err == nil || !strings.Contains(err.Error(), "authorizer") {
		t.Fatalf("unconfigured sweeper must refuse to claim schedules, got %v", err)
	}
}

// The column is NOT NULL, so the function must never return something Postgres
// would reject as a jsonb value.
func TestTaskConfigJSONIsNeverEmptyBytes(t *testing.T) {
	for _, template := range []string{``, `null`, `[]`, `"x"`, `{`, `  `} {
		if got := taskConfigJSON([]byte(template), nil); len(got) == 0 {
			t.Errorf("taskConfigJSON(%q) returned empty bytes; NOT NULL jsonb needs a value", template)
		}
	}
}

func TestTaskConfigCarriesNonSecretFireIntentButNoDecisionToken(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-1:2026-08-13T00:00:00Z",
	}
	var got map[string]json.RawMessage
	if err := json.Unmarshal(taskConfigJSON([]byte(`{"workflow_type":"x"}`), intent), &got); err != nil {
		t.Fatalf("decode task config: %v", err)
	}
	if _, ok := got["schedule_fire_intent"]; !ok {
		t.Fatal("fired task is missing the non-secret reauthorization intent")
	}
	if strings.Contains(string(got["schedule_fire_intent"]), "token") {
		t.Fatalf("task config must not retain a Control bearer: %s", got["schedule_fire_intent"])
	}
}

func TestMalformedTemplateCannotDropFreshFireIntent(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-1:2026-08-13T00:00:00Z",
	}
	var got map[string]json.RawMessage
	if err := json.Unmarshal(taskConfigJSON([]byte(`{"workflow_type":`), intent), &got); err != nil {
		t.Fatalf("decode normalized malformed template: %v", err)
	}
	if _, ok := got["schedule_fire_intent"]; !ok {
		t.Fatal("malformed template erased the required fresh-fire intent")
	}
}

func TestTemplateCannotOverrideSchedulerDerivedFireIntent(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-real",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-real:2026-08-13T00:00:00Z",
	}
	template := `{"schedule_fire_intent":{"schedule_id":"attacker"}}`
	var got struct {
		Intent FireIntent `json:"schedule_fire_intent"`
	}
	if err := json.Unmarshal(taskConfigJSON([]byte(template), intent), &got); err != nil {
		t.Fatalf("decode task config: %v", err)
	}
	if got.Intent.ScheduleID != intent.ScheduleID || got.Intent.OrgID != intent.OrgID {
		t.Fatalf("template overrode scheduler-derived fire intent: %+v", got.Intent)
	}
}
