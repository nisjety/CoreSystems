package reconcile

import (
	"context"
	"testing"

	"github.com/triodelab/model-plane/pkg/publisher"
)

func TestEmit_PublishesReconcileEvent(t *testing.T) {
	pub := publisher.NewInMemoryPublisher()
	if err := Emit(context.Background(), pub, KindMCPServer, ActionRegistered, "srv1", "org1"); err != nil {
		t.Fatalf("emit: %v", err)
	}
	recs := pub.Drain()
	if len(recs) != 1 {
		t.Fatalf("expected 1 published event, got %d", len(recs))
	}
	r := recs[0]
	if r.Subject != "mp.v1.capability.mcp_server.registered" {
		t.Fatalf("subject: %q", r.Subject)
	}
	if r.Envelope.Producer != "capability-core" || r.Envelope.OrgID != "org1" {
		t.Fatalf("envelope: %+v", r.Envelope)
	}
	if r.Envelope.ResourceRef != "mcp_server:srv1" {
		t.Fatalf("resource_ref: %q", r.Envelope.ResourceRef)
	}
	if r.Envelope.EventType != "capability.mcp_server.registered" {
		t.Fatalf("event_type: %q", r.Envelope.EventType)
	}
}

func TestEmit_NilPublisherIsNoop(t *testing.T) {
	if err := Emit(context.Background(), nil, KindSkill, ActionRegistered, "s1", "o1"); err != nil {
		t.Fatalf("nil publisher must be a no-op, got %v", err)
	}
}

func TestSubject_Format(t *testing.T) {
	if got := Subject(KindRoutingPolicy, ActionUpdated); got != "mp.v1.capability.routing_policy.updated" {
		t.Fatalf("subject: %q", got)
	}
}
