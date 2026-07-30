package main

import (
	"sort"
	"testing"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/orchestration"
)

// TestStartAllowlistMatchesWorkerRegistrations pins the invariant that makes
// StartWorkflow safe to expose: the allowlist and the worker must describe the
// same set of workflow types.
//
//   - A name in the allowlist that the worker does not register would accept a
//     start whose task then sits on the queue with nobody to run it.
//   - A registered name missing from the allowlist is dead capacity — the
//     situation this whole RPC exists to end.
func TestStartAllowlistMatchesWorkerRegistrations(t *testing.T) {
	registered := make([]string, 0, len(registeredWorkflows()))
	for _, reg := range registeredWorkflows() {
		if reg.Fn == nil {
			t.Fatalf("workflow %q registered with a nil function", reg.Name)
		}
		registered = append(registered, reg.Name)
	}
	sort.Strings(registered)

	allowed := orchestration.AllowedWorkflowTypes()

	if len(registered) != len(allowed) {
		t.Fatalf("worker registers %v but the start allowlist is %v", registered, allowed)
	}
	for i := range registered {
		if registered[i] != allowed[i] {
			t.Fatalf("worker registers %v but the start allowlist is %v", registered, allowed)
		}
	}
}

// TestWorkerRegistrationsAreUnique guards against a copy-paste that would make
// one workflow shadow another on the task queue.
func TestWorkerRegistrationsAreUnique(t *testing.T) {
	seen := map[string]bool{}
	for _, reg := range registeredWorkflows() {
		if seen[reg.Name] {
			t.Fatalf("workflow %q is registered twice", reg.Name)
		}
		seen[reg.Name] = true
	}
}

// TestAutoresearchIsNotReachable documents a deliberate omission rather than an
// oversight: AutoresearchWorkflow enforces `budget_usd` against a hardcoded
// $0.10-per-step figure because ExecuteStepResponse carries no usage data, so
// making it startable would advertise a spending cap that does not exist.
func TestAutoresearchIsNotReachable(t *testing.T) {
	for _, reg := range registeredWorkflows() {
		if reg.Name == "AutoresearchWorkflow" {
			t.Fatal("AutoresearchWorkflow must stay unregistered until its cost accounting is real")
		}
	}
	if _, ok := orchestration.LookupWorkflow("AutoresearchWorkflow"); ok {
		t.Fatal("AutoresearchWorkflow must not be on the start allowlist")
	}
}
