package cibuf

import (
	"path/filepath"
	"testing"
)

// workflowPath resolves the buf workflow at the repo root from this package's
// location: apps/Model Plane/go/pkg/cibuf/ → ../../../../../.github/workflows/buf.yml
func workflowPath(t *testing.T) string {
	t.Helper()
	abs, err := filepath.Abs(filepath.Join("..", "..", "..", "..", "..", ".github", "workflows", "buf.yml"))
	if err != nil {
		t.Fatalf("abs path: %v", err)
	}
	return abs
}

func TestBufWorkflowExists(t *testing.T) {
	wf, err := Load(workflowPath(t))
	if err != nil {
		t.Fatalf("load workflow: %v", err)
	}
	if wf.Name == "" {
		t.Fatal("workflow.name must be set")
	}
}

func TestBufWorkflowTriggers(t *testing.T) {
	wf, err := Load(workflowPath(t))
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	pr, ok := wf.Trigger("pull_request")
	if !ok {
		t.Fatal("missing pull_request trigger")
	}
	if !HasPath(pr, "apps/Model Plane/proto/**") {
		t.Error("pull_request paths must include apps/Model Plane/proto/**")
	}
	push, ok := wf.Trigger("push")
	if !ok {
		t.Fatal("missing push trigger")
	}
	if !HasPath(push, "apps/Model Plane/proto/**") {
		t.Error("push paths must include apps/Model Plane/proto/**")
	}
}

func TestBufWorkflowJob(t *testing.T) {
	wf, err := Load(workflowPath(t))
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if len(wf.Jobs) == 0 {
		t.Fatal("workflow must define at least one job")
	}
	var found bool
	for _, job := range wf.Jobs {
		if job.RunsOn != "ubuntu-latest" {
			continue
		}
		if !job.UsesAction("actions/checkout") {
			continue
		}
		if !job.UsesAction("bufbuild/buf-setup-action") {
			continue
		}
		if !job.RunsCommand("buf lint") {
			t.Error("job must run `buf lint`")
		}
		if !job.RunsCommand("buf breaking") {
			t.Error("job must run `buf breaking`")
		}
		if !job.RunsCommand("apps/Model Plane/proto") {
			t.Error("buf commands must target apps/Model Plane/proto")
		}
		if !job.RunsCommand("baseline.binpb") {
			t.Error("buf breaking must reference baseline.binpb image")
		}
		found = true
	}
	if !found {
		t.Fatal("no ubuntu-latest job with checkout+buf-setup found")
	}
}
