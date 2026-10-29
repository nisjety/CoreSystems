package activities

import (
	"testing"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
)

// TestToInferRequest exercises the pure InvokeRequest→InferRequest mapping,
// which is where ZDR and org scope must survive on their way to the provider
// boundary. Kept white-box so the mapping is asserted without a gRPC server.
func TestToInferRequest(t *testing.T) {
	req := evaloptimizer.InvokeRequest{
		Call:  evaloptimizer.CallJudge,
		Model: "claude-haiku-4-5",
		Messages: []evaloptimizer.Message{
			{Role: evaloptimizer.RoleSystem, Content: "sys"},
			{Role: evaloptimizer.RoleUser, Content: "usr"},
		},
		MaxTokens:              256,
		Temperature:            0,
		StructuredOutputSchema: evaloptimizer.VerdictSchema,
		ZDR:                    true,
		OrgID:                  "org-42",
	}

	ir := toInferRequest(req, "run-9", 3)

	if ir.OrgId != "org-42" {
		t.Errorf("OrgId = %q, want org-42", ir.OrgId)
	}
	if !ir.Zdr {
		t.Errorf("Zdr = false, want true (ZDR must propagate)")
	}
	if ir.Model != "claude-haiku-4-5" {
		t.Errorf("Model = %q", ir.Model)
	}
	if ir.MaxTokens != 256 {
		t.Errorf("MaxTokens = %d, want 256", ir.MaxTokens)
	}
	if ir.StructuredOutputSchema != evaloptimizer.VerdictSchema {
		t.Errorf("StructuredOutputSchema not propagated")
	}
	if ir.RequestId != "run-9-judge-3" {
		t.Errorf("RequestId = %q, want run-9-judge-3", ir.RequestId)
	}
	if len(ir.Messages) != 2 || ir.Messages[0].Role != "system" || ir.Messages[1].Content != "usr" {
		t.Errorf("messages not mapped correctly: %+v", ir.Messages)
	}
}

func TestToInferRequest_ZDRFalseStaysFalse(t *testing.T) {
	ir := toInferRequest(evaloptimizer.InvokeRequest{Call: evaloptimizer.CallGenerator, Model: "m", ZDR: false}, "run", 1)
	if ir.Zdr {
		t.Errorf("Zdr = true, want false")
	}
}
