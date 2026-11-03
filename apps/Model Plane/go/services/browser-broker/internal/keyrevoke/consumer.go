// Package keyrevoke ends the browser grants of a revoked API key (INNH-10
// MP-2). Auth Core publishes verevon.audit.v2.control.auth-core.api_key_revoked
// (CP-3) when a key is deleted, disabled, expires, or its owner leaves the
// org; this consumer revokes every live grant of that key, in both grant
// stores, with reason key_revoked.
//
// The durable is pre-provisioned server-side (like cost-core's GDPR erasure
// consumer); Start binds it and never creates one, so a broker cannot widen
// what the bus delivers to it. Wired in cmd/main.go only when
// BROWSER_BROKER_KEY_GRANTS_ENABLED is on and a NATS URL is set.
package keyrevoke

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
)

// Subject is Auth Core's API-key revocation event (CP-3).
const Subject = "verevon.audit.v2.control.auth-core.api_key_revoked"

// DefaultStream holds the Control Plane's audit/observability events.
const DefaultStream = "VEREVON_CONTROL_OBSERVABILITY"

// Durable is the broker's pre-provisioned consumer name. It must match the
// provisioner's; Bind refuses to start without it.
const Durable = "browser-broker-api-key-revoked"

// maxEventBytes bounds an event before it is decoded.
const maxEventBytes = 16 << 10

// retryDelay spaces redeliveries after a store error.
const retryDelay = 5 * time.Second

// Revoker ends every grant of one key (server.Server.RevokeKey).
type Revoker interface {
	RevokeKey(ctx context.Context, keyID string) (int64, error)
}

// event is the CP-3 payload: {key_id, org_id, reason}, either top level or
// under details (the audit v2 envelope).
type event struct {
	KeyID   string `json:"key_id"`
	Details struct {
		KeyID string `json:"key_id"`
	} `json:"details"`
}

// Outcome says what to do with a message.
type Outcome int

const (
	// Ack: done, or a definitive skip (malformed: redelivery cannot fix it).
	Ack Outcome = iota
	// Retry: a transient failure; nak so JetStream redelivers.
	Retry
)

// errMalformed marks an event that will never succeed.
var errMalformed = errors.New("malformed api_key_revoked event")

// keyID extracts and validates the key id. Top level wins; details.key_id is
// read when the top level has none. Two different ids are malformed.
func keyID(data []byte) (string, error) {
	if len(data) == 0 || len(data) > maxEventBytes {
		return "", errMalformed
	}
	var ev event
	if err := json.Unmarshal(data, &ev); err != nil {
		return "", errMalformed
	}
	top, nested := ev.KeyID, ev.Details.KeyID
	id := top
	if id == "" {
		id = nested
	}
	if top != "" && nested != "" && top != nested {
		return "", errMalformed
	}
	if !grant.ValidKeyID(id) {
		return "", errMalformed
	}
	return id, nil
}

// Process handles one event body. It is the testable core (no NATS).
func Process(ctx context.Context, revoker Revoker, data []byte) Outcome {
	id, err := keyID(data)
	if err != nil {
		slog.WarnContext(ctx, "api key revoke event skipped", "reason", "malformed", "bytes", len(data))
		return Ack
	}
	n, err := revoker.RevokeKey(ctx, id)
	if err != nil {
		// Never fail open: the key is already on the revoked list, and the
		// event is redelivered until the sweep succeeds.
		slog.WarnContext(ctx, "api key revoke failed, will retry", "key_id", id, "revoked", n, "error", err.Error())
		return Retry
	}
	slog.InfoContext(ctx, "api key grants revoked", "key_id", id, "revoked", n)
	return Ack
}

// Consumer binds the durable and revokes on each event.
type Consumer struct {
	js      nats.JetStreamContext
	stream  string
	revoker Revoker
	sub     *nats.Subscription
}

// New returns a consumer over js. An empty stream is DefaultStream.
func New(js nats.JetStreamContext, stream string, revoker Revoker) *Consumer {
	if strings.TrimSpace(stream) == "" {
		stream = DefaultStream
	}
	return &Consumer{js: js, stream: stream, revoker: revoker}
}

// Start binds the pre-provisioned durable (it never creates one).
func (c *Consumer) Start() error {
	sub, err := c.js.QueueSubscribe(Subject, Durable, c.handle,
		nats.Bind(c.stream, Durable),
		nats.ManualAck(),
	)
	if err != nil {
		return err
	}
	c.sub = sub
	slog.Info("api key revoke consumer bound", "stream", c.stream, "durable", Durable)
	return nil
}

// Stop unsubscribes.
func (c *Consumer) Stop() {
	if c == nil || c.sub == nil {
		return
	}
	_ = c.sub.Unsubscribe()
	c.sub = nil
}

func (c *Consumer) handle(msg *nats.Msg) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	switch Process(ctx, c.revoker, msg.Data) {
	case Retry:
		if err := msg.NakWithDelay(retryDelay); err != nil {
			slog.Warn("api key revoke nak failed", "error", err.Error())
		}
	default:
		if err := msg.Ack(); err != nil {
			slog.Warn("api key revoke ack failed", "error", err.Error())
		}
	}
}
