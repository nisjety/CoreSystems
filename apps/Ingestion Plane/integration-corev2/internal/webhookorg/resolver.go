// Package webhookorg resolves which tenant (organization) owns an inbound
// provider webhook.
//
// Real account-wide callbacks (Meta Page/Instagram/WhatsApp, Slack Events)
// carry NO Velion organization id — only provider-side account identifiers.
// Before this resolver existed, such events were stored with an empty org and
// silently dropped by every downstream consumer, which kept the Meta inbox
// channels dead in real multi-tenant traffic despite fully working
// normalizers (2026-07-07 verification finding).
//
// Resolution maps payload account ids onto connections:
//   - Meta family: entry[].id (Page id / IG business-account id / WABA id)
//     and entry[].changes[].value.metadata.phone_number_id, matched against
//     the connection's provider_account_id or its persisted
//     provider_context["webhook_account_ids"] enrichment. Connections without
//     enrichment get it lazily: one Graph sweep (/me/accounts +
//     /me/businesses?fields=owned_whatsapp_business_accounts) whose result is
//     persisted, so the cost is once per connection, not per webhook.
//   - Slack: team_id, matched against the connection's tenant_id (auth.test's
//     team_id is stored there at connect time).
//
// Results and misses are cached in-process with a short TTL so webhook bursts
// do not hammer Postgres or the Graph API.
package webhookorg

import (
	"context"
	"maps"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/store"
)

// metaProviderKeys are every provider key a Meta-family connection may carry.
var metaProviderKeys = []string{"meta", "facebook", "instagram", "whatsapp"}

// ConnectionStore is the store subset the resolver needs.
type ConnectionStore interface {
	FindConnectionByWebhookAccount(ctx context.Context, providerKeys []string, accountID string) (store.Connection, error)
	ListConnections(ctx context.Context, filter store.ConnectionFilter) ([]store.Connection, error)
	UpdateConnectionProviderContext(ctx context.Context, id string, providerContext map[string]string) (store.Connection, error)
}

// MetaAssetLister lists every webhook-relevant account id a Meta connection
// can receive callbacks for (Pages, IG business accounts, WABAs, phone
// numbers). Implemented by GraphAssetLister; nil disables lazy enrichment.
type MetaAssetLister interface {
	ListWebhookAccountIDs(ctx context.Context, conn store.Connection) ([]string, error)
}

// Resolution is a successful tenant match.
type Resolution struct {
	OrganizationID string
	ConnectionID   string
}

type cacheEntry struct {
	resolution Resolution
	found      bool
	expiresAt  time.Time
}

type Resolver struct {
	Store  ConnectionStore
	Meta   MetaAssetLister
	Logger *zerolog.Logger

	// PositiveTTL/NegativeTTL bound the in-process cache. Zero values get
	// sensible defaults (10m / 2m).
	PositiveTTL time.Duration
	NegativeTTL time.Duration

	mu    sync.Mutex
	cache map[string]cacheEntry
	// enrichedThisProcess prevents re-sweeping a connection whose enrichment
	// legitimately does not contain some unknown account id.
	enrichedThisProcess map[string]bool
}

// Resolve returns the owning org + reply connection for a webhook payload, or
// ok=false when no connection matches. It never errors — webhook ingestion
// must not fail because resolution had a transient problem; the event is then
// stored org-less exactly as before this resolver existed.
func (r *Resolver) Resolve(ctx context.Context, providerKey string, payload map[string]any) (Resolution, bool) {
	if r == nil || r.Store == nil {
		return Resolution{}, false
	}
	accountIDs, providerKeys := candidateAccountIDs(providerKey, payload)
	if len(accountIDs) == 0 {
		return Resolution{}, false
	}

	for _, accountID := range accountIDs {
		if resolution, found, cached := r.cached(providerKey, accountID); cached {
			if found {
				return resolution, true
			}
			continue
		}
		conn, err := r.Store.FindConnectionByWebhookAccount(ctx, providerKeys, accountID)
		if err == nil {
			resolution := Resolution{OrganizationID: conn.OrganizationID, ConnectionID: conn.ID}
			r.remember(providerKey, accountID, resolution, true)
			return resolution, true
		}
		if err != store.ErrNotFound {
			r.logWarn(err, "webhook org lookup failed for account "+accountID)
			// Transient store failure: do not negative-cache.
			continue
		}
		r.remember(providerKey, accountID, Resolution{}, false)
	}

	// No direct match. For Meta-family webhooks, enrich un-swept connections
	// with their Graph asset ids and retry once.
	if isMetaFamily(providerKey) && r.Meta != nil {
		if resolution, ok := r.enrichAndRetry(ctx, providerKey, accountIDs); ok {
			return resolution, true
		}
	}
	return Resolution{}, false
}

// enrichAndRetry sweeps Meta connections that have never been enriched (no
// webhook_account_ids in provider_context and not swept this process),
// persists their asset ids, and matches the payload account ids against the
// fresh enrichment directly.
func (r *Resolver) enrichAndRetry(ctx context.Context, providerKey string, accountIDs []string) (Resolution, bool) {
	wanted := map[string]bool{}
	for _, id := range accountIDs {
		wanted[id] = true
	}
	for _, key := range metaProviderKeys {
		connections, err := r.Store.ListConnections(ctx, store.ConnectionFilter{ProviderKey: key})
		if err != nil {
			r.logWarn(err, "webhook org enrichment: list "+key+" connections")
			continue
		}
		for _, conn := range connections {
			if conn.DeletedAt != nil || (conn.Status != "active" && conn.Status != "needs_refresh") {
				continue
			}
			if conn.ProviderContext["webhook_account_ids"] != "" || r.alreadyEnriched(conn.ID) {
				continue
			}
			r.markEnriched(conn.ID)
			ids, err := r.Meta.ListWebhookAccountIDs(ctx, conn)
			if err != nil {
				r.logWarn(err, "webhook org enrichment: graph sweep for connection "+conn.ID)
				continue
			}
			if len(ids) == 0 {
				continue
			}
			updated := cloneContext(conn.ProviderContext)
			updated["webhook_account_ids"] = strings.Join(ids, ",")
			if _, err := r.Store.UpdateConnectionProviderContext(ctx, conn.ID, updated); err != nil {
				r.logWarn(err, "webhook org enrichment: persist for connection "+conn.ID)
				// Still usable in-memory below even if persistence failed.
			}
			for _, id := range ids {
				if wanted[id] {
					resolution := Resolution{OrganizationID: conn.OrganizationID, ConnectionID: conn.ID}
					r.remember(providerKey, id, resolution, true)
					return resolution, true
				}
			}
		}
	}
	return Resolution{}, false
}

// candidateAccountIDs extracts provider-side account ids from a webhook
// payload, most-specific first, plus the provider keys to match against.
func candidateAccountIDs(providerKey string, payload map[string]any) ([]string, []string) {
	switch {
	case isMetaFamily(providerKey):
		return metaAccountIDs(payload), metaProviderKeys
	case providerKey == "slack":
		if teamID := stringField(payload, "team_id"); teamID != "" {
			return []string{teamID}, []string{"slack"}
		}
		return nil, nil
	default:
		return nil, nil
	}
}

func isMetaFamily(providerKey string) bool {
	switch providerKey {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return true
	default:
		return false
	}
}

// metaAccountIDs walks the Meta webhook envelope: entry[].id is the Page /
// IG-business-account / WABA id; WhatsApp additionally exposes the receiving
// number as entry[].changes[].value.metadata.phone_number_id.
func metaAccountIDs(payload map[string]any) []string {
	var ids []string
	seen := map[string]bool{}
	push := func(id string) {
		id = strings.TrimSpace(id)
		if id != "" && !seen[id] {
			seen[id] = true
			ids = append(ids, id)
		}
	}
	entries, _ := payload["entry"].([]any)
	for _, rawEntry := range entries {
		entry, ok := rawEntry.(map[string]any)
		if !ok {
			continue
		}
		push(stringField(entry, "id"))
		changes, _ := entry["changes"].([]any)
		for _, rawChange := range changes {
			change, ok := rawChange.(map[string]any)
			if !ok {
				continue
			}
			value, ok := change["value"].(map[string]any)
			if !ok {
				continue
			}
			if metadata, ok := value["metadata"].(map[string]any); ok {
				push(stringField(metadata, "phone_number_id"))
			}
		}
	}
	return ids
}

func stringField(m map[string]any, key string) string {
	switch v := m[key].(type) {
	case string:
		return strings.TrimSpace(v)
	case float64:
		// JSON numbers decode to float64; Meta ids are numeric strings but
		// defensive senders may emit them unquoted. Only whole numbers are
		// plausible ids.
		if v == float64(int64(v)) {
			return strconv.FormatInt(int64(v), 10)
		}
		return ""
	default:
		return ""
	}
}

func cloneContext(input map[string]string) map[string]string {
	out := make(map[string]string, len(input)+1)
	maps.Copy(out, input)
	return out
}

func (r *Resolver) cached(providerKey, accountID string) (Resolution, bool, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	entry, ok := r.cache[providerKey+":"+accountID]
	if !ok || time.Now().After(entry.expiresAt) {
		return Resolution{}, false, false
	}
	return entry.resolution, entry.found, true
}

func (r *Resolver) remember(providerKey, accountID string, resolution Resolution, found bool) {
	ttl := r.PositiveTTL
	if !found {
		ttl = r.NegativeTTL
	}
	if ttl <= 0 {
		if found {
			ttl = 10 * time.Minute
		} else {
			ttl = 2 * time.Minute
		}
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.cache == nil {
		r.cache = map[string]cacheEntry{}
	}
	r.cache[providerKey+":"+accountID] = cacheEntry{resolution: resolution, found: found, expiresAt: time.Now().Add(ttl)}
}

func (r *Resolver) alreadyEnriched(connectionID string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.enrichedThisProcess[connectionID]
}

func (r *Resolver) markEnriched(connectionID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.enrichedThisProcess == nil {
		r.enrichedThisProcess = map[string]bool{}
	}
	r.enrichedThisProcess[connectionID] = true
}

func (r *Resolver) logWarn(err error, msg string) {
	if r.Logger != nil {
		r.Logger.Warn().Err(err).Msg(msg)
	}
}
