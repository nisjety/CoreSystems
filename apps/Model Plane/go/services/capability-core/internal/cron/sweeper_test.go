package cron

import (
	"encoding/json"
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
			got := taskConfigJSON([]byte(tc.template))

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

// The column is NOT NULL, so the function must never return something Postgres
// would reject as a jsonb value.
func TestTaskConfigJSONIsNeverEmptyBytes(t *testing.T) {
	for _, template := range []string{``, `null`, `[]`, `"x"`, `{`, `  `} {
		if got := taskConfigJSON([]byte(template)); len(got) == 0 {
			t.Errorf("taskConfigJSON(%q) returned empty bytes; NOT NULL jsonb needs a value", template)
		}
	}
}
