package main

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"shipping-core/internal/booking"
	"shipping-core/internal/dataplane"
	"shipping-core/internal/events"
)

type recordingPublisher struct {
	mu     sync.Mutex
	events []events.Event
}

func (p *recordingPublisher) Publish(_ context.Context, event events.Event) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.events = append(p.events, event)
	return nil
}

func TestDeliveryHooksZDRLeavesNoDurableSideEffects(t *testing.T) {
	documentCalls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		documentCalls++
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"id":"should-not-exist"}`))
	}))
	defer server.Close()
	publisher := &recordingPublisher{}
	hooks := &deliveryHooks{
		events: publisher,
		dataPlane: dataplane.New(dataplane.Config{
			BaseURL: server.URL, AuthCoreURL: server.URL,
			ServiceID: "shipping-core", ServiceCredential: "synthetic-service-key",
		}),
		logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
	}

	hooks.OnDelivered(context.Background(), booking.Record{
		ID:    "booking-test",
		OrgID: "org-test",
		ZDR:   true,
	}, time.Now().UTC())

	if len(publisher.events) != 0 {
		t.Fatalf("published events = %d, want 0", len(publisher.events))
	}
	if documentCalls != 0 {
		t.Fatalf("document calls = %d, want 0", documentCalls)
	}
}

func TestDeliveryHooksUseBookingOrganization(t *testing.T) {
	var document dataplane.DocumentRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/data-plane/internal-token" {
			_ = json.NewEncoder(w).Encode(map[string]string{"token": "synthetic-data-plane-token"})
			return
		}
		if err := json.NewDecoder(r.Body).Decode(&document); err != nil {
			t.Errorf("decode document: %v", err)
		}
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"id":"document-test"}`))
	}))
	defer server.Close()
	publisher := &recordingPublisher{}
	hooks := &deliveryHooks{
		events: publisher,
		dataPlane: dataplane.New(dataplane.Config{
			BaseURL: server.URL, AuthCoreURL: server.URL,
			ServiceID: "shipping-core", ServiceCredential: "synthetic-service-key",
		}),
		logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
	}

	hooks.OnDelivered(context.Background(), booking.Record{
		ID:          "booking-test",
		OrgID:       "org-test",
		CarrierCode: "bring",
		CarrierName: "Bring",
		TrackingNo:  "SYNTHETIC-TRACKING",
	}, time.Date(2026, 7, 14, 12, 0, 0, 0, time.UTC))

	if len(publisher.events) != 1 || publisher.events[0].OrganizationID != "org-test" {
		t.Fatalf("published events = %#v, want one event for org-test", publisher.events)
	}
	if document.OrgID != "org-test" {
		t.Fatalf("document org = %q, want org-test", document.OrgID)
	}
}
