package api

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"time"

	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// DefaultMCPDNSRevalidationInterval is how often MCPDNSRevalidator re-resolves
// every registered MCP server's hostname.
//
// normalizeMCPRegistration's DNS-address check only ever runs once, at
// registration/update time. DNS answers can legitimately change afterward
// even without an attacker — a host migration, an operator's own zone change
// — and model-gateway's per-dial revalidation (safe_mcp_http_client) only
// protects the network path: a registry row can otherwise sit indefinitely
// advertising a server that would be rejected if registered today, visible to
// anyone who lists the catalog even before anything ever dials it again.
const DefaultMCPDNSRevalidationInterval = 15 * time.Minute

// MCPDNSRevalidator periodically re-resolves each enabled, non-quarantined
// HTTP MCP server's hostname and quarantines it (enabled=false,
// rollout_state='quarantine') if DNS now resolves into a forbidden address
// range — the same range table normalizeMCPRegistration checks at write time.
type MCPDNSRevalidator struct {
	pool     registryDatabase
	resolver mcpHostResolver
	interval time.Duration
	pub      publisher.EventPublisher
}

// NewMCPDNSRevalidator constructs a revalidator that re-checks every
// interval. A non-positive interval falls back to
// DefaultMCPDNSRevalidationInterval.
func NewMCPDNSRevalidator(pool registryDatabase, interval time.Duration) *MCPDNSRevalidator {
	if interval <= 0 {
		interval = DefaultMCPDNSRevalidationInterval
	}
	return &MCPDNSRevalidator{pool: pool, resolver: net.DefaultResolver, interval: interval}
}

// WithPublisher wires reconcile-event emission so model-gateway's cache picks
// up a quarantine before its own TTL would. Optional and nil-safe.
func (r *MCPDNSRevalidator) WithPublisher(pub publisher.EventPublisher) *MCPDNSRevalidator {
	r.pub = pub
	return r
}

// Start runs the revalidation loop until ctx is cancelled. Errors are logged,
// not fatal — a failed sweep leaves every row exactly as it was and simply
// tries again next tick.
func (r *MCPDNSRevalidator) Start(ctx context.Context) {
	ticker := time.NewTicker(r.interval)
	defer ticker.Stop()
	r.sweepAndLog(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			r.sweepAndLog(ctx)
		}
	}
}

func (r *MCPDNSRevalidator) sweepAndLog(ctx context.Context) {
	checked, quarantined, err := r.RunOnce(ctx)
	if err != nil {
		slog.Warn("mcp dns revalidation sweep failed", "error", err)
		return
	}
	if quarantined > 0 {
		slog.Warn("mcp dns revalidation quarantined drifted servers", "checked", checked, "quarantined", quarantined)
	}
}

type mcpDNSCandidate struct {
	id          string
	orgID       string
	endpointURL string
}

// RunOnce re-resolves every enabled, non-quarantined HTTP MCP server's
// hostname and quarantines any whose DNS answer now falls inside a forbidden
// range. Returns how many rows were checked and how many were quarantined.
//
// A resolver error or empty answer is treated as a transient DNS hiccup, not
// a security event, and is skipped rather than quarantined — the goal is
// catching a server that actively resolves somewhere forbidden, not punishing
// an outage.
func (r *MCPDNSRevalidator) RunOnce(ctx context.Context) (checked, quarantined int, err error) {
	if r.resolver == nil {
		return 0, 0, fmt.Errorf("mcp dns revalidator has no resolver configured")
	}
	rows, err := r.pool.Query(ctx, `
		SELECT id, org_id, endpoint_url FROM mcp_servers
		WHERE transport='http' AND enabled AND rollout_state <> 'quarantine'
		  AND endpoint_url <> '' AND deleted_at IS NULL
	`)
	if err != nil {
		return 0, 0, err
	}
	var candidates []mcpDNSCandidate
	for rows.Next() {
		var c mcpDNSCandidate
		if scanErr := rows.Scan(&c.id, &c.orgID, &c.endpointURL); scanErr != nil {
			rows.Close()
			return 0, 0, scanErr
		}
		candidates = append(candidates, c)
	}
	rows.Close()

	for _, c := range candidates {
		checked++
		reason, forbidden := r.hostDriftedToForbiddenAddress(ctx, c.endpointURL)
		if !forbidden {
			continue
		}
		if execErr := r.quarantine(ctx, c); execErr != nil {
			slog.Warn("failed to quarantine drifted mcp server",
				"server_id", c.id, "org_id", c.orgID, "error", execErr)
			continue
		}
		quarantined++
		slog.Warn("mcp server DNS drifted to a forbidden address; quarantined",
			"server_id", c.id, "org_id", c.orgID, "reason", reason)
		if eerr := reconcile.Emit(ctx, r.pub, reconcile.KindMCPServer, reconcile.ActionUpdated, c.id, c.orgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindMCPServer, "id", c.id, "error", eerr)
		}
	}
	return checked, quarantined, nil
}

func (r *MCPDNSRevalidator) quarantine(ctx context.Context, c mcpDNSCandidate) error {
	_, err := r.pool.Exec(ctx, `
		UPDATE mcp_servers SET enabled=false, rollout_state='quarantine', updated_at=$1
		WHERE id=$2 AND org_id=$3
	`, time.Now().UTC(), c.id, c.orgID)
	return err
}

// hostDriftedToForbiddenAddress re-parses and re-resolves one server's
// endpoint the same way normalizeMCPRegistration validated it at write time.
// An allowlisted internal host is exempt, same as at registration — that
// allowlist is itself the operator's trust decision, not something DNS drift
// should reverse. A parse failure (a durable row that no longer meets the
// current contract) counts as drifted too: hydrateMCPServerView already
// masks these on read, but that's recomputed per-request, not persisted —
// this makes the quarantine durable instead of true only for whichever
// request happens to read the row next.
func (r *MCPDNSRevalidator) hostDriftedToForbiddenAddress(ctx context.Context, endpointURL string) (string, bool) {
	_, host, err := parseMCPEndpoint(endpointURL)
	if err != nil {
		return "endpoint no longer passes validation", true
	}
	if mcpHostInInternalAllowlist(host) {
		return "", false
	}
	lookupCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	addresses, lookupErr := r.resolver.LookupNetIP(lookupCtx, "ip", host)
	if lookupErr != nil || len(addresses) == 0 {
		return "", false
	}
	for _, address := range addresses {
		if !mcpAddressIsPublic(address) {
			return fmt.Sprintf("resolved to forbidden address %s", address), true
		}
	}
	return "", false
}
