// Package webhookorg resolves which tenant (organization) owns an inbound
// provider webhook.
//
// Real account-wide callbacks (Meta Page/Instagram/WhatsApp, Slack Events)
// carry NO Verevon organization id — only provider-side account identifiers.
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
	"fmt"
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

// MetaWebhookAssets preserves the provider's asset type alongside the mixed
// ID list used for webhook ownership resolution. The typed lists are UI
// readiness evidence: a Page must not make Instagram or WhatsApp look live.
type MetaWebhookAssets struct {
	AccountIDs                 []string
	InstagramAccountIDs        []string
	PageIDs                    []string
	WhatsAppBusinessAccountIDs []string
	WhatsAppPhoneNumberIDs     []string
}

type detailedMetaAssetLister interface {
	ListWebhookAssets(ctx context.Context, conn store.Connection) (MetaWebhookAssets, error)
}

type metaAssetSubscriber interface {
	SubscribeWebhookAccounts(ctx context.Context, conn store.Connection, accountIDs []string) error
}

// Resolution is a successful tenant match.
type Resolution struct {
	OrganizationID string
	ConnectionID   string
}

type ResolvedPayload struct {
	Resolution Resolution
	Payload    map[string]any
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

// ProvisionConnection discovers every Meta inbox asset, subscribes those
// assets through the provider adapter, and persists the account ids used for
// tenant resolution. It is intentionally idempotent and is called by the
// connection Sync action; provider subscription endpoints treat repeats as a
// refresh of the same app subscription.
func (r *Resolver) ProvisionConnection(ctx context.Context, conn store.Connection) (store.Connection, error) {
	if !isMetaFamily(conn.ProviderKey) || !hasInboxCapability(conn.Capabilities) {
		return conn, nil
	}
	if r == nil || r.Store == nil || r.Meta == nil {
		return store.Connection{}, fmt.Errorf("Meta webhook provisioning is not configured")
	}
	var ids []string
	var assets MetaWebhookAssets
	var err error
	if detailed, ok := r.Meta.(detailedMetaAssetLister); ok {
		assets, err = detailed.ListWebhookAssets(ctx, conn)
		ids = assets.AccountIDs
	} else {
		ids, err = r.Meta.ListWebhookAccountIDs(ctx, conn)
	}
	if err != nil {
		return store.Connection{}, err
	}
	if len(ids) == 0 {
		return store.Connection{}, fmt.Errorf("Meta connection has no accessible Page, Instagram, or WhatsApp Business assets")
	}
	if err := r.validateOwnership(ctx, conn.ID, ids); err != nil {
		return store.Connection{}, err
	}
	updatedContext := cloneContext(conn.ProviderContext)
	updatedContext["webhook_account_ids"] = strings.Join(ids, ",")
	if len(assets.PageIDs) > 0 {
		updatedContext["meta_page_ids"] = strings.Join(assets.PageIDs, ",")
	}
	if len(assets.InstagramAccountIDs) > 0 {
		updatedContext["meta_instagram_account_ids"] = strings.Join(assets.InstagramAccountIDs, ",")
	}
	if len(assets.WhatsAppBusinessAccountIDs) > 0 {
		updatedContext["meta_whatsapp_business_account_ids"] = strings.Join(assets.WhatsAppBusinessAccountIDs, ",")
	}
	updated, err := r.Store.UpdateConnectionProviderContext(ctx, conn.ID, updatedContext)
	if err != nil {
		return store.Connection{}, fmt.Errorf("claim Meta webhook assets: %w", err)
	}
	subscriber, ok := r.Meta.(metaAssetSubscriber)
	if !ok {
		return store.Connection{}, fmt.Errorf("Meta webhook subscription adapter is not configured")
	}
	if err := subscriber.SubscribeWebhookAccounts(ctx, conn, ids); err != nil {
		// Best-effort release of the durable claim. The provider subscription is
		// idempotent, while retaining a failed tenant binding would misroute data.
		_, _ = r.Store.UpdateConnectionProviderContext(ctx, conn.ID, conn.ProviderContext)
		return store.Connection{}, fmt.Errorf("subscribe Meta webhook assets: %w", err)
	}
	r.markEnriched(conn.ID)
	return updated, nil
}

func (r *Resolver) validateOwnership(ctx context.Context, connectionID string, ids []string) error {
	for _, id := range ids {
		existing, lookupErr := r.Store.FindConnectionByWebhookAccount(ctx, metaProviderKeys, id)
		switch {
		case lookupErr == nil && existing.ID != connectionID:
			return fmt.Errorf("Meta asset %s is already bound to another connection", id)
		case lookupErr == store.ErrConflict:
			return fmt.Errorf("Meta asset %s has ambiguous ownership", id)
		case lookupErr != nil && lookupErr != store.ErrNotFound:
			return fmt.Errorf("check Meta asset %s ownership: %w", id, lookupErr)
		}
	}
	return nil
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

	if resolution, ok := r.resolveAllCandidates(ctx, providerKey, providerKeys, accountIDs); ok {
		return resolution, true
	}

	// No direct match. For Meta-family webhooks, enrich un-swept connections
	// with their Graph asset ids and retry once.
	if isMetaFamily(providerKey) && r.Meta != nil {
		if _, enriched := r.enrichAndRetry(ctx, providerKey, accountIDs); enriched {
			return r.resolveAllCandidates(ctx, providerKey, providerKeys, accountIDs)
		}
	}
	return Resolution{}, false
}

// ResolvePayloads partitions a multi-entry Meta callback by authoritative
// owner. Meta may batch Pages belonging to different Verevon organizations in
// one signed delivery; persisting the whole batch under one tenant is unsafe,
// while rejecting it forever is operationally dead. Every entry must resolve
// before any partition is returned.
func (r *Resolver) ResolvePayloads(ctx context.Context, providerKey string, payload map[string]any) ([]ResolvedPayload, bool) {
	if !isMetaFamily(providerKey) {
		resolution, ok := r.Resolve(ctx, providerKey, payload)
		if !ok {
			return nil, false
		}
		return []ResolvedPayload{{Resolution: resolution, Payload: payload}}, true
	}
	entries, _ := payload["entry"].([]any)
	if len(entries) <= 1 {
		resolution, ok := r.Resolve(ctx, providerKey, payload)
		if !ok {
			return nil, false
		}
		return []ResolvedPayload{{Resolution: resolution, Payload: payload}}, true
	}
	groups := map[string]*ResolvedPayload{}
	order := []string{}
	for _, entry := range entries {
		entryPayload := make(map[string]any, len(payload))
		maps.Copy(entryPayload, payload)
		entryPayload["entry"] = []any{entry}
		resolution, ok := r.Resolve(ctx, providerKey, entryPayload)
		if !ok {
			return nil, false
		}
		key := resolution.OrganizationID + "\x00" + resolution.ConnectionID
		group := groups[key]
		if group == nil {
			groupPayload := make(map[string]any, len(payload))
			maps.Copy(groupPayload, payload)
			groupPayload["entry"] = []any{}
			group = &ResolvedPayload{Resolution: resolution, Payload: groupPayload}
			groups[key] = group
			order = append(order, key)
		}
		group.Payload["entry"] = append(group.Payload["entry"].([]any), entry)
	}
	resolved := make([]ResolvedPayload, 0, len(order))
	for _, key := range order {
		resolved = append(resolved, *groups[key])
	}
	return resolved, true
}

// resolveAllCandidates fails closed unless every provider account id in the
// batch resolves to the exact same connection and organization. Meta can send
// multiple entry[] values in one signed callback; assigning the whole payload
// from the first match could otherwise cross tenant boundaries.
func (r *Resolver) resolveAllCandidates(ctx context.Context, providerKey string, providerKeys, accountIDs []string) (Resolution, bool) {
	var matched *Resolution
	for _, accountID := range accountIDs {
		// Meta misses are not cached because lazy enrichment may make the binding
		// durable during this request. Slack has no enrichment and benefits from
		// a short negative cache during unknown-team bursts.
		if !isMetaFamily(providerKey) {
			if _, _, cached := r.cached(providerKey, accountID); cached {
				return Resolution{}, false
			}
		}
		conn, err := r.Store.FindConnectionByWebhookAccount(ctx, providerKeys, accountID)
		if err != nil {
			if err == store.ErrNotFound && !isMetaFamily(providerKey) {
				r.remember(providerKey, accountID, Resolution{}, false)
			} else if err != store.ErrNotFound {
				r.logWarn(err, "webhook org lookup failed for account "+accountID)
			}
			return Resolution{}, false
		}
		resolution := Resolution{OrganizationID: conn.OrganizationID, ConnectionID: conn.ID}
		if matched != nil && (matched.OrganizationID != resolution.OrganizationID || matched.ConnectionID != resolution.ConnectionID) {
			r.logWarn(store.ErrConflict, "webhook batch has multiple tenant owners")
			return Resolution{}, false
		}
		matched = &resolution
	}
	if matched == nil {
		return Resolution{}, false
	}
	return *matched, true
}

// enrichAndRetry sweeps Meta connections that have never been enriched (no
// webhook_account_ids in provider_context and not swept this process),
// persists their asset ids, and matches the payload account ids against the
// fresh enrichment directly.
func (r *Resolver) enrichAndRetry(ctx context.Context, providerKey string, accountIDs []string) (Resolution, bool) {
	wanted := map[string]bool{}
	var matched *Resolution
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
			if conn.DeletedAt != nil || (conn.Status != "active" && conn.Status != "needs_refresh") || !hasInboxCapability(conn.Capabilities) {
				continue
			}
			if conn.ProviderContext["webhook_account_ids"] != "" || r.alreadyEnriched(conn.ID) {
				continue
			}
			ids, err := r.Meta.ListWebhookAccountIDs(ctx, conn)
			if err != nil {
				r.logWarn(err, "webhook org enrichment: graph sweep for connection "+conn.ID)
				continue
			}
			if len(ids) == 0 {
				continue
			}
			if err := r.validateOwnership(ctx, conn.ID, ids); err != nil {
				r.logWarn(err, "webhook org enrichment: ownership for connection "+conn.ID)
				continue
			}
			// Only suppress future sweeps after Graph returned a usable asset
			// snapshot. Marking before the call made one transient provider
			// failure disable tenant resolution for this connection until the
			// whole API process restarted.
			updated := cloneContext(conn.ProviderContext)
			updated["webhook_account_ids"] = strings.Join(ids, ",")
			if _, err := r.Store.UpdateConnectionProviderContext(ctx, conn.ID, updated); err != nil {
				r.logWarn(err, "webhook org enrichment: persist for connection "+conn.ID)
				// Do not suppress the next sweep: the durable binding remains absent.
				continue
			}
			r.markEnriched(conn.ID)
			for _, id := range ids {
				if wanted[id] {
					resolution := Resolution{OrganizationID: conn.OrganizationID, ConnectionID: conn.ID}
					if matched != nil && (matched.OrganizationID != resolution.OrganizationID || matched.ConnectionID != resolution.ConnectionID) {
						r.logWarn(store.ErrConflict, "webhook org enrichment found ambiguous ownership for account "+id)
						return Resolution{}, false
					}
					matched = &resolution
				}
			}
		}
	}
	if matched != nil {
		return *matched, true
	}
	return Resolution{}, false
}

func hasInboxCapability(capabilities []string) bool {
	for _, capability := range capabilities {
		switch strings.TrimSpace(strings.ToLower(capability)) {
		case "social.inbox.read", "social.messenger.manage", "social.whatsapp.manage":
			return true
		}
	}
	return false
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
