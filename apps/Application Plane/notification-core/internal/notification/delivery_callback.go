package notification

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"
)

const deliveryCallbackMaxSkew = 5 * time.Minute

type DeliveryCallbackReplayStore interface {
	Claim(context.Context, string, time.Time) (bool, error)
}

type DeliveryCallbackEvent struct {
	AttemptID         string `json:"attempt_id"`
	ProviderRequestID string `json:"provider_request_id"`
	ReceiptDigest     string `json:"receipt_digest"`
}

type DeliveryReceiptReconciler interface {
	MarkDeliveryAcknowledged(context.Context, string, string, string, time.Time) (*DeliveryAttempt, error)
}

type DeliveryCallbackVerifier struct {
	secret []byte
	replay DeliveryCallbackReplayStore
	now    TimeSource
}

func NewDeliveryCallbackVerifier(secret []byte, replay DeliveryCallbackReplayStore, now TimeSource) *DeliveryCallbackVerifier {
	secretCopy := append([]byte(nil), secret...)
	if now == nil {
		now = time.Now
	}
	return &DeliveryCallbackVerifier{secret: secretCopy, replay: replay, now: now}
}

func (v *DeliveryCallbackVerifier) Verify(ctx context.Context, timestamp int64, nonce, signature string, body []byte) (*DeliveryCallbackEvent, error) {
	if v == nil || len(v.secret) < 32 || v.replay == nil {
		return nil, errors.New("delivery callback verifier is not configured")
	}
	nonce = strings.TrimSpace(nonce)
	signature = strings.TrimSpace(signature)
	if nonce == "" || len(nonce) > 128 || signature == "" {
		return nil, errors.New("delivery callback signature fields are required")
	}
	now := v.now().UTC()
	occurredAt := time.Unix(timestamp, 0).UTC()
	if occurredAt.Before(now.Add(-deliveryCallbackMaxSkew)) || occurredAt.After(now.Add(deliveryCallbackMaxSkew)) {
		return nil, errors.New("delivery callback timestamp is outside the accepted window")
	}

	mac := hmac.New(sha256.New, v.secret)
	_, _ = fmt.Fprintf(mac, "%d.%s.", timestamp, nonce)
	_, _ = mac.Write(body)
	expected := mac.Sum(nil)
	provided, err := hex.DecodeString(signature)
	if err != nil || len(provided) != len(expected) || subtle.ConstantTimeCompare(provided, expected) != 1 {
		return nil, errors.New("delivery callback signature is invalid")
	}

	var event DeliveryCallbackEvent
	decoder := json.NewDecoder(strings.NewReader(string(body)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&event); err != nil {
		return nil, errors.New("delivery callback body is invalid")
	}
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		return nil, errors.New("delivery callback body contains multiple JSON values")
	}
	if strings.TrimSpace(event.AttemptID) == "" || strings.TrimSpace(event.ProviderRequestID) == "" || strings.TrimSpace(event.ReceiptDigest) == "" {
		return nil, errors.New("delivery callback receipt fields are required")
	}

	claimed, err := v.replay.Claim(ctx, nonce, now.Add(deliveryCallbackMaxSkew))
	if err != nil {
		return nil, fmt.Errorf("claim callback nonce: %w", err)
	}
	if !claimed {
		return nil, errors.New("delivery callback replay detected")
	}
	return &event, nil
}

// ReconcileDeliveryCallback is the narrow callback-to-receipt boundary. The
// provider request ID from the signed callback is passed to the owner ledger;
// the repository must exact-match it before acknowledging an attempt.
func ReconcileDeliveryCallback(
	ctx context.Context,
	verifier *DeliveryCallbackVerifier,
	reconciler DeliveryReceiptReconciler,
	timestamp int64,
	nonce, signature string,
	body []byte,
	now time.Time,
) (*DeliveryAttempt, error) {
	if verifier == nil || reconciler == nil {
		return nil, errors.New("delivery callback reconciliation is not configured")
	}
	event, err := verifier.Verify(ctx, timestamp, nonce, signature, body)
	if err != nil {
		return nil, err
	}
	return reconciler.MarkDeliveryAcknowledged(ctx, event.AttemptID, event.ProviderRequestID, event.ReceiptDigest, now.UTC())
}
