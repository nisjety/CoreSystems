package http

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/collection"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"net/http/httptest"
	"testing"
)

type inventoryOwner struct{ calls int }

func (s *inventoryOwner) Get(context.Context, string, string) pulse.Projection {
	return pulse.Projection{}
}
func (s *inventoryOwner) CollectionInventory(_ context.Context, id, site, q string) (collection.Inventory, error) {
	s.calls++
	return collection.Inventory{WebsiteID: id, Site: site, WebsiteRevision: 1, Quarter: q, Source: "visibility_collection_inventory", Datasets: []collection.Dataset{}}, nil
}
func TestInventoryHTTPIsAuthenticatedReadOnlyAndStrict(t *testing.T) {
	owner := &inventoryOwner{}
	handler := NewHandler("test-internal-key", owner)
	path := "/api/v1/visibility/collection?websiteId=web_0123456789abcdef0123456789abcdef&site=https%3A%2F%2Ftriodelab.no%2F&quarter=FY"
	for _, tc := range []struct {
		method, path, key string
		want              int
	}{{"GET", path, "", 401}, {"POST", path, "test-internal-key", 405}, {"GET", path + "&organizationId=other", "test-internal-key", 400}, {"GET", path + "&quarter=Q1", "test-internal-key", 400}, {"GET", path, "test-internal-key", 200}} {
		req := httptest.NewRequest(tc.method, tc.path, nil)
		req.Header.Set("X-Internal-API-Key", tc.key)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != tc.want {
			t.Fatalf("%s: %d %s", tc.path, response.Code, response.Body.String())
		}
		if response.Code == 200 && response.Header().Get("Cache-Control") != "no-store" {
			t.Fatal("private inventory can be cached")
		}
	}
	if owner.calls != 1 {
		t.Fatal("invalid/read-only boundary reached owner", owner.calls)
	}
}
