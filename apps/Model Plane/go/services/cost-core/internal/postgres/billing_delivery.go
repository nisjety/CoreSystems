package postgres

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

type BillingDelivery struct {
	BaseURL string
	Token   string
	Client  *http.Client
}

func NewBillingDelivery(base, token string) (*BillingDelivery, error) {
	base = strings.TrimRight(strings.TrimSpace(base), "/")
	u, err := url.Parse(base)
	if err != nil || u.Host == "" || (u.Scheme != "http" && u.Scheme != "https") || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.Path != "" || len(strings.TrimSpace(token)) < 32 {
		return nil, fmt.Errorf("valid billing URL and service credential required")
	}
	return &BillingDelivery{BaseURL: base, Token: token, Client: &http.Client{Timeout: 5 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}, nil
}

type billingUsagePayload struct {
	EventID    string    `json:"event_id"`
	Metric     string    `json:"metric"`
	Quantity   int64     `json:"quantity"`
	Source     string    `json:"source"`
	OccurredAt time.Time `json:"occurred_at"`
}

func billingUsagePayloads(id, basis string, input, output int64, at time.Time) []billingUsagePayload {
	if basis != "metered" && basis != "subscription" {
		return nil
	}
	result := []billingUsagePayload{{EventID: "cost:" + id + ":requests", Metric: "ai." + basis + ".requests", Quantity: 1, Source: "cost-core", OccurredAt: at}}
	// Separate input/output meters preserve token semantics and per-event bounds.
	if input > 0 {
		result = append(result, billingUsagePayload{EventID: "cost:" + id + ":input", Metric: "ai." + basis + ".input_tokens", Quantity: input, Source: "cost-core", OccurredAt: at})
	}
	if output > 0 {
		result = append(result, billingUsagePayload{EventID: "cost:" + id + ":output", Metric: "ai." + basis + ".output_tokens", Quantity: output, Source: "cost-core", OccurredAt: at})
	}
	return result
}

// Each HTTP delivery is retryable by its stable Cost Core entry+meter ID.
// Transaction rollback or partial multi-meter delivery cannot double consume.
func (s *Store) DeliverBillingBatch(ctx context.Context, d *BillingDelivery) (int, error) {
	delivered := 0
	for i := 0; i < 50; i++ {
		tx, err := s.pool.Begin(ctx)
		if err != nil {
			return delivered, err
		}
		done, err := deliverBillingOne(ctx, tx, d)
		if err != nil {
			_ = tx.Rollback(ctx)
			return delivered, err
		}
		if err = tx.Commit(ctx); err != nil {
			return delivered, err
		}
		if !done {
			break
		}
		delivered++
	}
	return delivered, nil
}
func deliverBillingOne(ctx context.Context, tx pgx.Tx, d *BillingDelivery) (bool, error) {
	var id, org, basis string
	var input, output int64
	var at time.Time
	var attempts int
	err := tx.QueryRow(ctx, `SELECT o.entry_id::text,c.org_id,c.cost_basis,c.input_tokens,c.output_tokens,c.created_at,o.attempts
 FROM cost_billing_outbox o JOIN cost_entries c ON c.id=o.entry_id
 WHERE o.delivered_at IS NULL AND o.next_attempt_at<=now()
 ORDER BY o.next_attempt_at,o.entry_id LIMIT 1 FOR UPDATE OF o SKIP LOCKED`).Scan(&id, &org, &basis, &input, &output, &at, &attempts)
	if err == pgx.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	status := 0
	for _, payload := range billingUsagePayloads(id, basis, input, output, at) {
		body, err := json.Marshal(payload)
		if err != nil {
			return false, err
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, d.BaseURL+"/api/v1/billing/orgs/"+url.PathEscape(org)+"/usage", bytes.NewReader(body))
		if err != nil {
			return false, err
		}
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Service-Id", "cost-core")
		req.Header.Set("X-Service-Token", d.Token)
		response, err := d.Client.Do(req)
		if err != nil {
			status = 0
			break
		}
		status = response.StatusCode
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 4096))
		response.Body.Close()
		if status < 200 || status >= 300 {
			break
		}
	}
	if status >= 200 && status < 300 {
		_, err = tx.Exec(ctx, `UPDATE cost_billing_outbox SET delivered_at=now(),attempts=attempts+1,last_status=$2 WHERE entry_id=$1::uuid`, id, status)
	} else {
		delay := time.Second * 30 * time.Duration(1<<min(attempts, 7))
		_, err = tx.Exec(ctx, `UPDATE cost_billing_outbox SET attempts=attempts+1,last_status=$2,next_attempt_at=$3 WHERE entry_id=$1::uuid`, id, status, time.Now().UTC().Add(delay))
		slog.Warn("billing usage delivery will retry", "status", status)
	}
	return true, err
}
func (s *Store) RunBillingDelivery(ctx context.Context, d *BillingDelivery) {
	ticker := time.NewTicker(15 * time.Second)
	defer ticker.Stop()
	for {
		if _, err := s.DeliverBillingBatch(ctx, d); err != nil && ctx.Err() == nil {
			slog.Error("billing usage delivery failed", "error", err)
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
