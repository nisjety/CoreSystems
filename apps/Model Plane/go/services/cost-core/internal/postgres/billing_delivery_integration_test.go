//go:build integration

package postgres

import (
	"context"
	"encoding/json"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"
)

func TestBillingOutboxRetryAndPartialDelivery(t *testing.T) {
	dsn := os.Getenv("COST_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("disposable postgres not configured")
	}
	ctx := context.Background()
	pool, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	applyMigration(ctx, t, pool)
	store, _ := New(pool)
	org := "billing-delivery-fixture"
	defer pool.Exec(ctx, `DELETE FROM cost_entries WHERE org_id=$1`, org)
	entry := ledger.Entry{OrgID: org, UserID: "user", ProducerID: "model-gateway", IdempotencyKey: "billing-delivery-fixture", InputTokens: 123, OutputTokens: 4, CostBasis: "subscription", CreatedAt: time.Now().UTC()}
	for i := 0; i < 2; i++ {
		if err = store.RecordEntry(ctx, entry); err != nil {
			t.Fatal(err)
		}
	}
	var count int
	if err = pool.QueryRow(ctx, `SELECT COUNT(*) FROM cost_billing_outbox o JOIN cost_entries c ON c.id=o.entry_id WHERE c.org_id=$1`, org).Scan(&count); err != nil || count != 1 {
		t.Fatal("duplicate outbox", count, err)
	}
	received := map[string]int{}
	fail := true
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Service-Id") != "cost-core" || r.Header.Get("X-Service-Token") == "" {
			t.Error("missing service credentials")
		}
		var payload billingUsagePayload
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Error(err)
			w.WriteHeader(400)
			return
		}
		if payload.Metric == "ai.subscription.input_tokens" && fail {
			w.WriteHeader(503)
			return
		}
		received[payload.EventID]++
		w.WriteHeader(202)
	}))
	defer server.Close()
	delivery, err := NewBillingDelivery(server.URL, "abcdefghijklmnopqrstuvwxyz1234567890")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.DeliverBillingBatch(ctx, delivery); err != nil {
		t.Fatal(err)
	}
	var delivered bool
	if err = pool.QueryRow(ctx, `SELECT delivered_at IS NOT NULL FROM cost_billing_outbox o JOIN cost_entries c ON c.id=o.entry_id WHERE c.org_id=$1`, org).Scan(&delivered); err != nil || delivered {
		t.Fatal("partial delivery acknowledged", err)
	}
	fail = false
	if _, err = pool.Exec(ctx, `UPDATE cost_billing_outbox SET next_attempt_at=now() WHERE entry_id IN(SELECT id FROM cost_entries WHERE org_id=$1)`, org); err != nil {
		t.Fatal(err)
	}
	if _, err = store.DeliverBillingBatch(ctx, delivery); err != nil {
		t.Fatal(err)
	}
	if len(received) != 3 {
		t.Fatal("missing meters after retry", received)
	}
	if err = pool.QueryRow(ctx, `SELECT delivered_at IS NOT NULL FROM cost_billing_outbox o JOIN cost_entries c ON c.id=o.entry_id WHERE c.org_id=$1`, org).Scan(&delivered); err != nil || !delivered {
		t.Fatal("retry not acknowledged", err)
	}
}
