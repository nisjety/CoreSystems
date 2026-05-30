package modules

import (
	"fmt"
	"sort"
	"strings"
	"sync"
)

type Registry struct {
	mu      sync.RWMutex
	modules map[string]Module
}

func NewRegistry() *Registry {
	return &Registry{modules: map[string]Module{}}
}

func (r *Registry) Register(m Module) error {
	if m == nil {
		return fmt.Errorf("module is nil")
	}
	name := strings.ToLower(strings.TrimSpace(m.Name()))
	if name == "" {
		return fmt.Errorf("module name is empty")
	}

	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.modules[name]; exists {
		return fmt.Errorf("module already registered: %s", name)
	}
	r.modules[name] = m
	return nil
}

func (r *Registry) Get(name string) (Module, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	m, ok := r.modules[strings.ToLower(strings.TrimSpace(name))]
	return m, ok
}

func (r *Registry) MustGetOrDefault(name, fallback string) (Module, bool) {
	if m, ok := r.Get(name); ok {
		return m, true
	}
	return r.Get(fallback)
}

func (r *Registry) Names() []string {
	r.mu.RLock()
	defer r.mu.RUnlock()
	names := make([]string, 0, len(r.modules))
	for k := range r.modules {
		names = append(names, k)
	}
	sort.Strings(names)
	return names
}
