package http

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/address"
	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
)

func TestReady_RequiresServiceCredential(t *testing.T) {
	handler := NewHandler(config.Config{ServiceName: "information-core"}, nil, nil, nil, nil)
	router := newRouter(handler, "")

	req := httptest.NewRequest("GET", "/ready", nil)
	res := httptest.NewRecorder()
	router.ServeHTTP(res, req)

	if res.Code != 503 {
		t.Fatalf("ready status = %d, want 503", res.Code)
	}
}

func TestReady_IsHealthyWhenServiceCredentialExists(t *testing.T) {
	handler := NewHandler(config.Config{ServiceName: "information-core", InternalAPIKey: "secret"}, nil, nil, nil, nil)
	router := newRouter(handler, "secret")

	req := httptest.NewRequest("GET", "/ready", nil)
	res := httptest.NewRecorder()
	router.ServeHTTP(res, req)

	if res.Code != 200 {
		t.Fatalf("ready status = %d, want 200", res.Code)
	}
}

func TestAddressRouteRequiresInternalKeyAndReturnsProviderEnvelope(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"metadata":{},"adresser":[]}`))
	}))
	defer upstream.Close()

	addressService := address.NewServiceWithURL(http.DefaultClient, cache.New(), upstream.URL)
	handler := NewHandler(config.Config{ServiceName: "information-core", InternalAPIKey: "secret"}, addressService, nil, nil, nil)
	router := newRouter(handler, "secret")

	unauthorized := httptest.NewRequest("GET", "/api/v1/address?q=Oslo", nil)
	unauthorizedResponse := httptest.NewRecorder()
	router.ServeHTTP(unauthorizedResponse, unauthorized)
	if unauthorizedResponse.Code != http.StatusUnauthorized {
		t.Fatalf("unauthorized status = %d, want 401", unauthorizedResponse.Code)
	}

	request := httptest.NewRequest("GET", "/api/v1/address?q=Oslo", nil).WithContext(context.Background())
	request.Header.Set("x-internal-api-key", "secret")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("address status = %d, want 200: %s", response.Code, response.Body.String())
	}
}

func TestShippingTrackingRouteIsNotDuplicated(t *testing.T) {
	handler := NewHandler(config.Config{ServiceName: "information-core", InternalAPIKey: "secret"}, nil, nil, nil, nil)
	router := newRouter(handler, "secret")

	request := httptest.NewRequest("GET", "/api/v1/shipping/track?trackingNumber=123", nil)
	request.Header.Set("x-internal-api-key", "secret")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusNotFound {
		t.Fatalf("duplicate shipping route status = %d, want 404", response.Code)
	}
}

func TestRegisteredSourceRoutesFailClosedWithoutConfiguration(t *testing.T) {
	handler := NewHandler(config.Config{ServiceName: "information-core", InternalAPIKey: "secret"}, nil, nil, nil, nil)
	router := newRouter(handler, "secret")
	for _, path := range []string{"/api/v1/datex/situation", "/api/v1/frost/observations?sources=SN18700&elements=air_temperature&referencetime=latest"} {
		req := httptest.NewRequest(http.MethodGet, path, nil)
		req.Header.Set("x-internal-api-key", "secret")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, req)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("%s status = %d, want 503", path, response.Code)
		}
	}
}
