package social

import (
	"context"
	"fmt"
	"strings"
)

// Task #29 (provider business modules program): commerce/catalog ownership
// decision. Two genuinely different provider domains carry a "commerce"
// capability today:
//
//   - Meta Commerce Catalog (catalogs.list / catalog.products, capability
//     social.catalog.manage) feeds Facebook/Instagram shoppable posts and
//     ads — this is a social-advertising concern, and social-core already
//     owns every other Meta operation. Implemented below, read-only.
//   - Shopify (products.read / orders.read / customers.read) is described in
//     its own capability catalog as support for "ecommerce support answers"
//     and "authenticated customer support workflows" — i.e. a conversation-core
//     concern (an agent answering "where's my order"), not a social one.
//     NOT implemented here: it needs its own design (how a support agent
//     resolves which Shopify order/customer a conversation is about), which
//     is a distinct feature from "list rows for an org", not a small
//     read-endpoint bolt-on. Deferred to a follow-up scoped to conversation-core.
//
// Write operations (catalog.product.upsert, catalog.batch, orders.write) are
// deliberately NOT exposed anywhere yet — no reviewed workflow needs them.

// ListCatalogs lists the Meta commerce catalogs owned by every connected
// Meta-family account in the org that has the social.catalog.manage
// capability. Per-account gateway failures are skipped, not fatal — one
// account's provider hiccup shouldn't blank the whole list.
func (s *Service) ListCatalogs(ctx context.Context, orgID string) ([]map[string]any, error) {
	if s.actionExecutor == nil {
		return nil, fmt.Errorf("action executor is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	accounts, err := s.ListAccounts(ctx, orgID)
	if err != nil {
		return nil, err
	}

	catalogs := []map[string]any{}
	for _, account := range accounts {
		providerKey := normalizePlatform(account.ProviderKey)
		if !isMetaFamilyProvider(providerKey) {
			continue
		}
		if account.Status != AccountStatusConnected || strings.TrimSpace(account.ConnectionID) == "" {
			continue
		}
		if !hasAnyCapability(account, "social.catalog.manage") {
			continue
		}
		result, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
			ConnectionID: account.ConnectionID,
			Operation:    "catalogs.list",
			Params:       map[string]any{"limit": 100},
		})
		if err != nil {
			continue // best-effort across accounts; caller sees what succeeded
		}
		for _, row := range decodeActionRows(result) {
			row["account_id"] = account.ID
			row["provider_key"] = providerKey
			catalogs = append(catalogs, row)
		}
	}
	return catalogs, nil
}

// ListCatalogProducts lists products in one catalog. accountID must be an
// account social-core already resolved for this org (via ListAccounts) —
// never a caller-supplied connection id — so a caller cannot read a
// catalog belonging to another org's connection (IDOR).
func (s *Service) ListCatalogProducts(ctx context.Context, orgID, accountID, catalogID string) ([]map[string]any, error) {
	if s.actionExecutor == nil {
		return nil, fmt.Errorf("action executor is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	accountID = strings.TrimSpace(accountID)
	catalogID = strings.TrimSpace(catalogID)
	if orgID == "" || accountID == "" || catalogID == "" {
		return nil, fmt.Errorf("%w: org_id, account_id, and catalog_id are required", ErrInvalidInput)
	}
	accounts, err := s.ListAccounts(ctx, orgID)
	if err != nil {
		return nil, err
	}
	var account *Account
	for i := range accounts {
		if accounts[i].ID == accountID {
			account = &accounts[i]
			break
		}
	}
	if account == nil {
		return nil, fmt.Errorf("%w: account does not belong to this org", ErrInvalidInput)
	}
	if !hasAnyCapability(*account, "social.catalog.manage") {
		return nil, fmt.Errorf("%w: account lacks the social.catalog.manage capability", ErrInvalidInput)
	}
	if strings.TrimSpace(account.ConnectionID) == "" {
		return nil, fmt.Errorf("%w: account has no connection id", ErrInvalidInput)
	}

	result, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "catalog.products",
		Params:       map[string]any{"catalogId": catalogID, "limit": 100},
	})
	if err != nil {
		return nil, err
	}
	return decodeActionRows(result), nil
}
