// Package reconcile emits capability-registry change events (matrix §4.3) so
// cache holders — chiefly the model-gateway's in-memory runtime_registries —
// can invalidate/refresh instead of going stale. capability-core is the
// registry system of record; these events are how the caches stay consistent
// with it. Subjects: mp.v1.capability.<kind>.<action>.
//
// Emission is the capability-core-owned half of §4.3; the gateway-subscribe
// half + live NATS delivery are wired/verified against the running stack.
package reconcile

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/publisher"
)

// Producer identifies capability-core as the event source.
const Producer = "capability-core"

// SubjectPrefix roots all capability reconcile subjects.
const SubjectPrefix = "mp.v1.capability"

// Capability kinds.
const (
	KindSkill         = "skill"
	KindMCPServer     = "mcp_server"
	KindPlugin        = "plugin"
	KindRoutingPolicy = "routing_policy"
	KindSafetyPolicy  = "safety_policy"
	KindModel         = "model"
)

// Mutation actions.
const (
	ActionRegistered = "registered"
	ActionUpdated    = "updated"
	ActionRemoved    = "removed"
)

// Subject builds the reconcile subject for a kind+action, e.g.
// "mp.v1.capability.mcp_server.registered".
func Subject(kind, action string) string {
	return SubjectPrefix + "." + kind + "." + action
}

// Emit publishes a reconcile event. It is **nil-safe**: a nil publisher is a
// no-op, so the service runs cleanly without an event bus configured (the
// gateway then falls back to its own cache TTL). Failures are returned for the
// caller to log best-effort — a reconcile-emit failure must never block the
// registry mutation that triggered it.
func Emit(ctx context.Context, pub publisher.EventPublisher, kind, action, id, orgID string) error {
	if pub == nil {
		return nil
	}
	payload, err := json.Marshal(map[string]string{"kind": kind, "action": action, "id": id})
	if err != nil {
		return fmt.Errorf("reconcile emit marshal payload (%s.%s): %w", kind, action, err)
	}
	eid := uuid.NewString()
	env := &envelope.Envelope{
		EventID:        eid,
		EventType:      "capability." + kind + "." + action,
		SchemaVersion:  1,
		Ts:             time.Now().UTC(),
		Producer:       Producer,
		IdempotencyKey: eid,
		OrgID:          orgID,
		ResourceRef:    kind + ":" + id,
		Payload:        payload,
	}
	return pub.Publish(ctx, Subject(kind, action), env)
}
