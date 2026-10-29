// Package sandbox declares the contract-shim schema for the tool-sandboxing
// capability: syscall allowlists, resource limits, network and filesystem
// policies enforced around tool/code execution.
package sandbox

import (
	"errors"
	"fmt"
	"time"
)

// ResourceLimits bounds runtime resource consumption for a sandboxed execution.
type ResourceLimits struct {
	CPUMillicores int64         `json:"cpu_millicores"`
	MemoryBytes   int64         `json:"memory_bytes"`
	PIDs          int64         `json:"pids"`
	Timeout       time.Duration `json:"timeout"`
}

// NetworkPolicy constrains egress for a sandboxed execution.
type NetworkPolicy struct {
	AllowCIDRs []string `json:"allow_cidrs"`
	DenyCIDRs  []string `json:"deny_cidrs"`
}

// FilesystemPolicy constrains filesystem visibility for a sandboxed execution.
type FilesystemPolicy struct {
	ReadOnlyPaths  []string `json:"read_only_paths"`
	ReadWritePaths []string `json:"read_write_paths"`
	DeniedPaths    []string `json:"denied_paths"`
}

// SandboxPolicy aggregates all enforcement knobs for a single tool execution.
type SandboxPolicy struct {
	AllowedSyscalls []string         `json:"allowed_syscalls"`
	Resources       ResourceLimits   `json:"resources"`
	Network         NetworkPolicy    `json:"network"`
	Filesystem      FilesystemPolicy `json:"filesystem"`
}

// DefaultSandboxPolicy returns a conservative policy suitable as a baseline.
func DefaultSandboxPolicy() SandboxPolicy {
	return SandboxPolicy{
		AllowedSyscalls: []string{
			"read", "write", "open", "openat", "close",
			"stat", "fstat", "lseek", "mmap", "munmap",
			"brk", "rt_sigaction", "rt_sigprocmask",
			"exit", "exit_group",
		},
		Resources: ResourceLimits{
			CPUMillicores: 500,
			MemoryBytes:   256 * 1024 * 1024,
			PIDs:          64,
			Timeout:       10 * time.Second,
		},
		Network: NetworkPolicy{
			AllowCIDRs: []string{},
			DenyCIDRs:  []string{"0.0.0.0/0"},
		},
		Filesystem: FilesystemPolicy{
			ReadOnlyPaths:  []string{"/usr", "/lib", "/etc"},
			ReadWritePaths: []string{"/tmp/sandbox"},
			DeniedPaths:    []string{"/proc/sys", "/sys"},
		},
	}
}

// Validate enforces the tool-sandboxing invariants.
func (p SandboxPolicy) Validate() error {
	if len(p.AllowedSyscalls) == 0 {
		return errors.New("AllowedSyscalls must not be empty")
	}
	if p.Resources.CPUMillicores <= 0 {
		return errors.New("Resources.CPUMillicores must be > 0")
	}
	if p.Resources.MemoryBytes <= 0 {
		return errors.New("Resources.MemoryBytes must be > 0")
	}
	if p.Resources.PIDs <= 0 {
		return errors.New("Resources.PIDs must be > 0")
	}
	if p.Resources.Timeout <= 0 {
		return errors.New("Resources.Timeout must be > 0")
	}
	if err := disjoint("read_only", p.Filesystem.ReadOnlyPaths, "read_write", p.Filesystem.ReadWritePaths); err != nil {
		return err
	}
	if err := disjoint("read_only", p.Filesystem.ReadOnlyPaths, "denied", p.Filesystem.DeniedPaths); err != nil {
		return err
	}
	if err := disjoint("read_write", p.Filesystem.ReadWritePaths, "denied", p.Filesystem.DeniedPaths); err != nil {
		return err
	}
	return nil
}

func disjoint(aName string, a []string, bName string, b []string) error {
	seen := make(map[string]struct{}, len(a))
	for _, p := range a {
		seen[p] = struct{}{}
	}
	for _, p := range b {
		if _, ok := seen[p]; ok {
			return fmt.Errorf("filesystem path %q appears in both %s and %s", p, aName, bName)
		}
	}
	return nil
}
