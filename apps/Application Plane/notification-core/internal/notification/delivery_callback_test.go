package notification

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"testing"
	"time"
)

type callbackReplayStoreFake struct {
	seen map[string]bool
}

type callbackReconcilerFake struct {
	attemptID         string
	providerRequestID string
	receiptDigest     string
}

func (f *callbackReconcilerFake) MarkDeliveryAcknowledged(_ context.Context, attemptID, providerRequestID, receiptDigest string, _ time.Time) (*DeliveryAttempt, error) {
	f.attemptID = attemptID
	f.providerRequestID = providerRequestID
	f.receiptDigest = receiptDigest
	return &DeliveryAttempt{ID: attemptID, Status: DeliveryAttemptAcknowledged}, nil
}

func (f *callbackReplayStoreFake) Claim(_ context.Context, key string, _ time.Time) (bool, error) {
	if f.seen == nil {
		f.seen = map[string]bool{}
	}
	if f.seen[key] {
		return false, nil
	}
	f.seen[key] = true
	return true, nil
}

func TestDeliveryCallbackRequiresFreshSignedReplayProtectedEnvelope(t *testing.T) {
	secret := []byte("delivery-callback-test-secret-32-bytes")
	now := time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC)
	replay := &callbackReplayStoreFake{}
	verifier := NewDeliveryCallbackVerifier(secret, replay, func() time.Time { return now })

	body := []byte(`{"attempt_id":"attempt-1","provider_request_id":"provider-1","receipt_digest":"receipt-1"}`)
	timestamp := now.Unix()
	nonce := "nonce-1"
	signature := callbackSignature(secret, timestamp, nonce, body)
	event, err := verifier.Verify(context.Background(), timestamp, nonce, signature, body)
	if err != nil || event.AttemptID != "attempt-1" {
		t.Fatalf("Verify() = %#v, %v", event, err)
	}
	if _, err := verifier.Verify(context.Background(), timestamp, nonce, signature, body); err == nil {
		t.Fatal("replayed callback was accepted")
	}

	stale := now.Add(-11 * time.Minute).Unix()
	if _, err := verifier.Verify(context.Background(), stale, "nonce-stale", callbackSignature(secret, stale, "nonce-stale", body), body); err == nil {
		t.Fatal("stale callback was accepted")
	}
}

func TestDeliveryCallbackRejectsTamperedBody(t *testing.T) {
	secret := []byte("delivery-callback-test-secret-32-bytes")
	now := time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC)
	verifier := NewDeliveryCallbackVerifier(secret, &callbackReplayStoreFake{}, func() time.Time { return now })
	body := []byte(`{"attempt_id":"attempt-1","provider_request_id":"provider-1","receipt_digest":"receipt-1"}`)
	timestamp := now.Unix()
	signature := callbackSignature(secret, timestamp, "nonce-1", body)
	if _, err := verifier.Verify(context.Background(), timestamp, "nonce-1", signature, []byte(`{"attempt_id":"attempt-2"}`)); err == nil {
		t.Fatal("tampered callback body was accepted")
	}
}

func TestDeliveryCallbackReconciliationBindsProviderReceipt(t *testing.T) {
	secret := []byte("delivery-callback-test-secret-32-bytes")
	now := time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC)
	verifier := NewDeliveryCallbackVerifier(secret, &callbackReplayStoreFake{}, func() time.Time { return now })
	reconciler := &callbackReconcilerFake{}
	body := []byte(`{"attempt_id":"attempt-1","provider_request_id":"provider-1","receipt_digest":"receipt-1"}`)
	timestamp := now.Unix()
	_, err := ReconcileDeliveryCallback(context.Background(), verifier, reconciler, timestamp, "nonce-1", callbackSignature(secret, timestamp, "nonce-1", body), body, now)
	if err != nil {
		t.Fatal(err)
	}
	if reconciler.attemptID != "attempt-1" || reconciler.providerRequestID != "provider-1" || reconciler.receiptDigest != "receipt-1" {
		t.Fatalf("reconciler received %q/%q/%q", reconciler.attemptID, reconciler.providerRequestID, reconciler.receiptDigest)
	}
}

func callbackSignature(secret []byte, timestamp int64, nonce string, body []byte) string {
	mac := hmac.New(sha256.New, secret)
	_, _ = fmt.Fprintf(mac, "%d.%s.", timestamp, nonce)
	_, _ = mac.Write(body)
	return hex.EncodeToString(mac.Sum(nil))
}
