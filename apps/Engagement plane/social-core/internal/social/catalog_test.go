package social

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
)

// fakeCatalogExecutor is a minimal ActionExecutor for testing catalog.go's
// account-resolution and gateway-delegation logic without a real
// integration-corev2.
type fakeCatalogExecutor struct {
	calls   []ActionRequest
	results map[string]*ActionResult
	err     error
}

func (f *fakeCatalogExecutor) ExecuteAction(_ context.Context, request ActionRequest) (*ActionResult, error) {
	f.calls = append(f.calls, request)
	if f.err != nil {
		return nil, f.err
	}
	if result, ok := f.results[request.Operation]; ok {
		return result, nil
	}
	return &ActionResult{Operation: request.Operation, Result: json.RawMessage(`{}`)}, nil
}

func mustJSONRaw(t *testing.T, v any) json.RawMessage {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}
	return raw
}

func metaAccount(id, connectionID string, capabilities ...string) Account {
	return Account{
		ID: id, OrgID: "org-1", ProviderKey: "meta", ConnectionID: connectionID,
		Status: AccountStatusConnected, Capabilities: capabilities,
	}
}

func TestListCatalogs_OnlyMetaAccountsWithCapability(t *testing.T) {
	repo := &fakeRepository{accounts: []Account{
		metaAccount("acct-1", "conn-1", "social.catalog.manage"),
		metaAccount("acct-2", "conn-2", "social.post.write"), // no catalog capability
		{ID: "acct-3", OrgID: "org-1", ProviderKey: "linkedin", ConnectionID: "conn-3", Status: AccountStatusConnected, Capabilities: []string{"social.catalog.manage"}},
	}}
	executor := &fakeCatalogExecutor{results: map[string]*ActionResult{
		"catalogs.list": {Operation: "catalogs.list", Result: mustJSONRaw(t, map[string]any{
			"data": []map[string]any{{"id": "cat-1", "name": "Demo Catalog"}},
		})},
	}}
	svc := NewService(repo, WithActionExecutor(executor))

	catalogs, err := svc.ListCatalogs(context.Background(), "org-1")
	if err != nil {
		t.Fatalf("ListCatalogs: %v", err)
	}
	if len(catalogs) != 1 {
		t.Fatalf("got %d catalogs, want 1 (only acct-1 qualifies): %+v", len(catalogs), catalogs)
	}
	if catalogs[0]["account_id"] != "acct-1" || catalogs[0]["provider_key"] != "meta" {
		t.Errorf("catalog row not annotated correctly: %+v", catalogs[0])
	}
	if len(executor.calls) != 1 || executor.calls[0].ConnectionID != "conn-1" {
		t.Errorf("expected exactly one call against conn-1, got %+v", executor.calls)
	}
}

func TestListCatalogs_PerAccountFailureSkipped(t *testing.T) {
	repo := &fakeRepository{accounts: []Account{
		metaAccount("acct-1", "conn-1", "social.catalog.manage"),
	}}
	executor := &fakeCatalogExecutor{err: fmt.Errorf("graph api down")}
	svc := NewService(repo, WithActionExecutor(executor))

	catalogs, err := svc.ListCatalogs(context.Background(), "org-1")
	if err != nil {
		t.Fatalf("ListCatalogs should not fail the whole call on a per-account error: %v", err)
	}
	if len(catalogs) != 0 {
		t.Errorf("expected no catalogs when the only account's call fails, got %+v", catalogs)
	}
}

func TestListCatalogProducts_RequiresAccountOwnedByOrg(t *testing.T) {
	repo := &fakeRepository{accounts: []Account{
		metaAccount("acct-1", "conn-1", "social.catalog.manage"),
	}}
	executor := &fakeCatalogExecutor{}
	svc := NewService(repo, WithActionExecutor(executor))

	// A caller-supplied account id that isn't in this org's account list must
	// be rejected — never forwarded to the gateway (IDOR guard).
	if _, err := svc.ListCatalogProducts(context.Background(), "org-1", "acct-from-another-org", "cat-1"); err == nil {
		t.Fatal("expected an error for an account not owned by this org")
	}
	if len(executor.calls) != 0 {
		t.Errorf("gateway should never be called for an unresolvable account, got %+v", executor.calls)
	}
}

func TestListCatalogProducts_RequiresCatalogCapability(t *testing.T) {
	repo := &fakeRepository{accounts: []Account{
		metaAccount("acct-1", "conn-1", "social.post.write"), // no catalog capability
	}}
	executor := &fakeCatalogExecutor{}
	svc := NewService(repo, WithActionExecutor(executor))

	if _, err := svc.ListCatalogProducts(context.Background(), "org-1", "acct-1", "cat-1"); err == nil {
		t.Fatal("expected an error when the account lacks social.catalog.manage")
	}
	if len(executor.calls) != 0 {
		t.Errorf("gateway should never be called without the capability, got %+v", executor.calls)
	}
}

func TestListCatalogProducts_DelegatesToGatewayWithCatalogID(t *testing.T) {
	repo := &fakeRepository{accounts: []Account{
		metaAccount("acct-1", "conn-1", "social.catalog.manage"),
	}}
	executor := &fakeCatalogExecutor{results: map[string]*ActionResult{
		"catalog.products": {Operation: "catalog.products", Result: mustJSONRaw(t, map[string]any{
			"data": []map[string]any{{"id": "prod-1", "name": "Widget"}},
		})},
	}}
	svc := NewService(repo, WithActionExecutor(executor))

	products, err := svc.ListCatalogProducts(context.Background(), "org-1", "acct-1", "cat-1")
	if err != nil {
		t.Fatalf("ListCatalogProducts: %v", err)
	}
	if len(products) != 1 || products[0]["id"] != "prod-1" {
		t.Fatalf("unexpected products: %+v", products)
	}
	if len(executor.calls) != 1 {
		t.Fatalf("expected one gateway call, got %d", len(executor.calls))
	}
	call := executor.calls[0]
	if call.ConnectionID != "conn-1" || call.Operation != "catalog.products" || call.Params["catalogId"] != "cat-1" {
		t.Errorf("gateway call mismatch: %+v", call)
	}
}
