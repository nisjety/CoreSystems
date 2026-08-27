// Package registry — startup-loaded cache over the Postgres-backed models
// table.
//
// PROVIDER_AND_PRIVACY_STRATEGY.md §4.5 is explicit: capability-core's models
// registry "should stay declarative and be loaded/cached at startup" rather
// than consulted synchronously on the invoke/listing hot path. Before this
// cache existed, Server.ListCapabilities and Server.GetCapability called
// ModelsRegistry.ListAsCapabilitiesForOrg / GetByCapabilityIDForOrg directly,
// putting a Postgres round-trip on every gRPC call. ModelsCache loads the
// full non-deleted catalog once (Reload) and serves both lookups from an
// in-memory snapshot, mirroring the existing Registry/Source pattern used
// for the static+Postgres capabilities catalog (registry.go,
// capabilities_source.go).
package registry

import (
	"context"
	"strings"
	"sync"

	"github.com/google/uuid"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// ModelsLoader loads the full non-deleted models catalog. *ModelsRegistry
// satisfies this via List(ctx, ModelsFilter{}).
type ModelsLoader interface {
	List(ctx context.Context, f ModelsFilter) ([]*Model, error)
}

// ModelsCache is a startup-loaded, in-memory snapshot of the models table.
// Safe for concurrent reads; Reload atomically swaps the snapshot.
type ModelsCache struct {
	loader ModelsLoader

	mu    sync.RWMutex
	items []*Model
}

// NewModelsCache constructs a cache and performs the initial synchronous
// load. A load failure at startup is fatal, mirroring the existing
// capabilities Registry bootstrap (registry.NewFromSource).
func NewModelsCache(ctx context.Context, loader ModelsLoader) (*ModelsCache, error) {
	if loader == nil {
		return nil, domain.ErrInvalidArgument
	}
	c := &ModelsCache{loader: loader}
	if err := c.Reload(ctx); err != nil {
		return nil, err
	}
	return c, nil
}

// Reload re-reads the backing store and atomically swaps the in-memory
// snapshot. Intended for startup and, optionally, an operator-triggered or
// periodic refresh — never the request hot path.
func (c *ModelsCache) Reload(ctx context.Context) error {
	items, err := c.loader.List(ctx, ModelsFilter{})
	if err != nil {
		return err
	}
	c.mu.Lock()
	c.items = items
	c.mu.Unlock()
	return nil
}

// ListAsCapabilitiesForOrg mirrors ModelsRegistry.ListAsCapabilitiesForOrg's
// filtering semantics (enabled, non-deleted, org_id IS NULL OR org_id = X)
// against the in-memory snapshot instead of Postgres.
func (c *ModelsCache) ListAsCapabilitiesForOrg(_ context.Context, orgID string) ([]*models.Capability, error) {
	parsedOrgID, parseErr := uuid.Parse(orgID)
	hasOrg := parseErr == nil && parsedOrgID != uuid.Nil

	c.mu.RLock()
	defer c.mu.RUnlock()

	out := make([]*models.Capability, 0, len(c.items))
	for _, m := range c.items {
		if m == nil || !m.Enabled {
			continue
		}
		if m.OrgID == uuid.Nil || (hasOrg && m.OrgID == parsedOrgID) {
			out = append(out, ToCapability(m))
		}
	}
	return out, nil
}

// GetByCapabilityIDForOrg mirrors ModelsRegistry.GetByCapabilityIDForOrg
// against the in-memory snapshot: a tenant-owned model wins over the global
// fallback for the same (provider, name).
func (c *ModelsCache) GetByCapabilityIDForOrg(_ context.Context, id, orgID string) (*Model, error) {
	const prefix = "cap.model."
	if !strings.HasPrefix(id, prefix) {
		return nil, domain.ErrCapabilityNotFound
	}
	rest := id[len(prefix):]
	dot := strings.Index(rest, ".")
	if dot <= 0 || dot == len(rest)-1 {
		return nil, domain.ErrInvalidArgument
	}
	provider, name := rest[:dot], rest[dot+1:]

	parsedOrgID, parseErr := uuid.Parse(orgID)
	hasOrg := parseErr == nil && parsedOrgID != uuid.Nil

	c.mu.RLock()
	defer c.mu.RUnlock()

	if hasOrg {
		for _, m := range c.items {
			if m != nil && m.OrgID == parsedOrgID && m.Provider == provider && m.Name == name {
				return m, nil
			}
		}
	}
	for _, m := range c.items {
		if m != nil && m.OrgID == uuid.Nil && m.Provider == provider && m.Name == name {
			return m, nil
		}
	}
	return nil, domain.ErrCapabilityNotFound
}
