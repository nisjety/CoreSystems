package sandbox

import (
	"encoding/json"
	"testing"
	"time"
)

func TestDefaultSandboxPolicyValidates(t *testing.T) {
	p := DefaultSandboxPolicy()
	if err := p.Validate(); err != nil {
		t.Fatalf("default policy must validate: %v", err)
	}
}

func TestValidateRejectsEmptySyscalls(t *testing.T) {
	p := DefaultSandboxPolicy()
	p.AllowedSyscalls = nil
	if err := p.Validate(); err == nil {
		t.Fatal("expected error when AllowedSyscalls empty")
	}
}

func TestValidateRejectsNonPositiveResourceLimits(t *testing.T) {
	cases := []func(*ResourceLimits){
		func(r *ResourceLimits) { r.CPUMillicores = 0 },
		func(r *ResourceLimits) { r.MemoryBytes = 0 },
		func(r *ResourceLimits) { r.PIDs = 0 },
		func(r *ResourceLimits) { r.Timeout = 0 },
	}
	for i, mutate := range cases {
		p := DefaultSandboxPolicy()
		mutate(&p.Resources)
		if err := p.Validate(); err == nil {
			t.Fatalf("case %d: expected error for non-positive resource limit", i)
		}
	}
}

func TestValidateRejectsOverlappingFSPaths(t *testing.T) {
	p := DefaultSandboxPolicy()
	p.Filesystem.ReadOnlyPaths = []string{"/etc"}
	p.Filesystem.ReadWritePaths = []string{"/etc"}
	if err := p.Validate(); err == nil {
		t.Fatal("expected error when readonly and readwrite overlap")
	}

	p = DefaultSandboxPolicy()
	p.Filesystem.ReadWritePaths = []string{"/tmp"}
	p.Filesystem.DeniedPaths = []string{"/tmp"}
	if err := p.Validate(); err == nil {
		t.Fatal("expected error when readwrite and denied overlap")
	}
}

func TestSandboxPolicyJSONRoundTrip(t *testing.T) {
	in := DefaultSandboxPolicy()
	in.Resources.Timeout = 30 * time.Second
	b, err := json.Marshal(in)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var out SandboxPolicy
	if err := json.Unmarshal(b, &out); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if err := out.Validate(); err != nil {
		t.Fatalf("round-tripped policy must validate: %v", err)
	}
	if out.Resources.Timeout != in.Resources.Timeout {
		t.Fatalf("timeout not preserved: got %v want %v", out.Resources.Timeout, in.Resources.Timeout)
	}
	if len(out.AllowedSyscalls) != len(in.AllowedSyscalls) {
		t.Fatalf("syscalls not preserved")
	}
}
