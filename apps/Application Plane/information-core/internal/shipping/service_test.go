package shipping_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/shipping"
)

func TestTrack_InTransit(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.URL.Query().Get("q"); got != "TEST123" {
			t.Errorf("expected q=TEST123, got %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"consignmentSet": [{
				"consignmentId": "TEST123",
				"packageSet": [{
					"statusCode": "IN_TRANSIT",
					"statusDescription": "Package is on its way",
					"dateOfEstimatedDelivery": "2026-06-15",
					"eventSet": [
						{
							"description": "Loaded on vehicle",
							"occurrenceDatestamp": "2026-06-13T10:00:00",
							"limitedAddress": {"city": "Oslo", "country": "NO"}
						},
						{
							"description": "Arrived at terminal",
							"occurrenceDatestamp": "2026-06-13T08:00:00",
							"limitedAddress": {"city": "Bergen", "country": "NO"}
						}
					]
				}]
			}]
		}`))
	}))
	defer srv.Close()

	svc := shipping.NewServiceWithURL(http.DefaultClient, cache.New(), "", "", srv.URL)
	result, err := svc.Track(context.Background(), "TEST123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Status != "In transit" {
		t.Errorf("expected 'In transit', got %q", result.Status)
	}
	if result.TrackingNumber != "TEST123" {
		t.Errorf("expected tracking number TEST123, got %q", result.TrackingNumber)
	}
	if len(result.Events) != 2 {
		t.Errorf("expected 2 events, got %d", len(result.Events))
	}
	if result.Events[0].Location != "Oslo, NO" {
		t.Errorf("expected Oslo, NO, got %q", result.Events[0].Location)
	}
}

func TestTrack_Delivered(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"consignmentSet": [{
				"consignmentId": "DELIVERED999",
				"packageSet": [{
					"statusCode": "DELIVERED",
					"statusDescription": "Delivered to recipient",
					"eventSet": []
				}]
			}]
		}`))
	}))
	defer srv.Close()

	svc := shipping.NewServiceWithURL(http.DefaultClient, cache.New(), "", "", srv.URL)
	result, err := svc.Track(context.Background(), "DELIVERED999")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Status != "Delivered" {
		t.Errorf("expected 'Delivered', got %q", result.Status)
	}
}

func TestTrack_UpstreamError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"consignmentSet": [{
				"consignmentId": "UNKNOWN",
				"error": {"code": "NOT_FOUND", "message": "Tracking number not found"}
			}]
		}`))
	}))
	defer srv.Close()

	svc := shipping.NewServiceWithURL(http.DefaultClient, cache.New(), "", "", srv.URL)
	_, err := svc.Track(context.Background(), "UNKNOWN")
	if err == nil {
		t.Fatal("expected error for upstream NOT_FOUND, got nil")
	}
}

func TestTrack_EmptyTrackingNumber(t *testing.T) {
	svc := shipping.NewServiceWithURL(http.DefaultClient, cache.New(), "", "", "http://unused")
	_, err := svc.Track(context.Background(), "")
	if err == nil {
		t.Fatal("expected error for empty tracking number, got nil")
	}
}

func TestTrack_CacheHit(t *testing.T) {
	calls := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"consignmentSet": [{
				"consignmentId": "CACHE1",
				"packageSet": [{"statusCode": "IN_TRANSIT", "statusDescription": "On the way", "eventSet": []}]
			}]
		}`))
	}))
	defer srv.Close()

	svc := shipping.NewServiceWithURL(http.DefaultClient, cache.New(), "", "", srv.URL)
	ctx := context.Background()

	if _, err := svc.Track(ctx, "CACHE1"); err != nil {
		t.Fatalf("first call: %v", err)
	}
	if _, err := svc.Track(ctx, "CACHE1"); err != nil {
		t.Fatalf("second call: %v", err)
	}
	if calls != 1 {
		t.Errorf("expected 1 upstream call, got %d", calls)
	}
}
