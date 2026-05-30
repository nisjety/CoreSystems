// Package cibuf validates the buf CI workflow shape.
package cibuf

import (
	"os"

	"gopkg.in/yaml.v3"
)

// Workflow is a minimal subset of GitHub Actions workflow YAML used for
// asserting the buf proto CI gate is wired correctly.
type Workflow struct {
	Name string         `yaml:"name"`
	On   map[string]any `yaml:"on"`
	Jobs map[string]Job `yaml:"jobs"`
}

// Job models a single GitHub Actions job.
type Job struct {
	RunsOn string `yaml:"runs-on"`
	Steps  []Step `yaml:"steps"`
}

// Step models a single GitHub Actions step.
type Step struct {
	Name string `yaml:"name"`
	Uses string `yaml:"uses"`
	Run  string `yaml:"run"`
}

// Load reads and parses a workflow YAML file from disk.
func Load(path string) (*Workflow, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var wf Workflow
	if err := yaml.Unmarshal(b, &wf); err != nil {
		return nil, err
	}
	return &wf, nil
}

// Trigger returns the trigger config for a given event ("push", "pull_request").
func (w *Workflow) Trigger(event string) (map[string]any, bool) {
	if w.On == nil {
		return nil, false
	}
	v, ok := w.On[event]
	if !ok {
		return nil, false
	}
	m, ok := v.(map[string]any)
	return m, ok
}

// HasPath reports whether the trigger's `paths` filter contains the given glob.
func HasPath(trigger map[string]any, glob string) bool {
	raw, ok := trigger["paths"]
	if !ok {
		return false
	}
	list, ok := raw.([]any)
	if !ok {
		return false
	}
	for _, p := range list {
		if s, ok := p.(string); ok && s == glob {
			return true
		}
	}
	return false
}

// UsesAction reports whether any step in the job uses the given action prefix.
func (j Job) UsesAction(prefix string) bool {
	for _, s := range j.Steps {
		if s.Uses == prefix || hasPrefix(s.Uses, prefix+"@") {
			return true
		}
	}
	return false
}

// RunsCommand reports whether any step's `run` script contains the substring.
func (j Job) RunsCommand(substr string) bool {
	for _, s := range j.Steps {
		if contains(s.Run, substr) {
			return true
		}
	}
	return false
}

func hasPrefix(s, p string) bool { return len(s) >= len(p) && s[:len(p)] == p }

func contains(s, sub string) bool {
	if sub == "" {
		return true
	}
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}
