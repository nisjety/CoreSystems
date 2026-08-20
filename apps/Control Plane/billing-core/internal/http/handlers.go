package http

import (
	"context"
	"crypto/subtle"
	"errors"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
)

func (s *Server) health(c *gin.Context) {
	// Deep check: round-trip the DB so an unreachable/unauthenticated database
	// (e.g. a stale DB password) reports unhealthy instead of silently serving
	// stale reads. The docker healthcheck hits /health, so a 503 marks the
	// container unhealthy.
	ctx, cancel := context.WithTimeout(c.Request.Context(), 2*time.Second)
	defer cancel()
	if err := s.billingCore.Ping(ctx); err != nil {
		log.Printf("health: database ping failed: %v", err)
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"status":  "unhealthy",
			"service": "billing-core",
			"error":   "database unreachable",
		})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"status":    "healthy",
		"service":   "billing-core",
		"timestamp": time.Now().UTC().Format(time.RFC3339),
	})
}

func (s *Server) deactivateOrganization(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	var req struct {
		Reason string `json:"reason"`
	}
	if orgID == "" || c.ShouldBindJSON(&req) != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId and request body are required"})
		return
	}
	if err := s.billingCore.DeactivateOrganization(c.Request.Context(), orgID, req.Reason); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to deactivate organization billing"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) getAccount(c *gin.Context) {
	orgID := c.Param("orgId")
	account, err := s.billingCore.GetAccount(c.Request.Context(), orgID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to get account"})
		return
	}
	c.JSON(http.StatusOK, account)
}

func (s *Server) upsertAccount(c *gin.Context) {
	orgID := c.Param("orgId")

	var req billing.Account
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	req.OrgID = orgID
	if err := s.billingCore.UpsertAccount(c.Request.Context(), req); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to upsert account"})
		return
	}

	updated, err := s.billingCore.GetAccount(c.Request.Context(), orgID)
	if err != nil {
		c.JSON(http.StatusAccepted, gin.H{"status": "accepted"})
		return
	}

	c.JSON(http.StatusOK, updated)
}

func (s *Server) recordUsage(c *gin.Context) {
	orgID := c.Param("orgId")

	var req struct {
		EventID    string                 `json:"event_id" binding:"required"`
		Metric     string                 `json:"metric" binding:"required"`
		Quantity   float64                `json:"quantity" binding:"required"`
		Source     string                 `json:"source,omitempty"`
		OccurredAt string                 `json:"occurred_at" binding:"required"`
		Metadata   map[string]interface{} `json:"metadata,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "event_id, metric, quantity, and occurred_at are required"})
		return
	}

	if err := billing.ValidateUsageEventID(req.EventID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "event_id must be a stable 1-128 byte identifier"})
		return
	}
	parsed, err := time.Parse(time.RFC3339, req.OccurredAt)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "occurred_at must be RFC3339 format"})
		return
	}
	usage := billing.UsageEvent{
		EventID:    req.EventID,
		OrgID:      orgID,
		Metric:     req.Metric,
		Quantity:   req.Quantity,
		Source:     req.Source,
		OccurredAt: parsed,
		Metadata:   req.Metadata,
	}
	if err := billing.ValidateUsageEvent(usage); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid usage event"})
		return
	}

	if err := s.billingCore.RecordUsage(c.Request.Context(), usage); errors.Is(err, billing.ErrUsageEventConflict) {
		c.JSON(http.StatusConflict, gin.H{"error": "event_id is already bound to a different usage event"})
		return
	} else if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to record usage"})
		return
	}

	c.JSON(http.StatusAccepted, gin.H{"status": "usage recorded"})
}

func (s *Server) checkEntitlement(c *gin.Context) {
	orgID := c.Param("orgId")
	feature := c.Param("feature")

	allowed, account, err := s.billingCore.CanUseFeature(c.Request.Context(), orgID, feature)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to check entitlement"})
		return
	}

	status := http.StatusOK
	if !allowed {
		status = http.StatusPaymentRequired
	}

	// Report the *effective* plan (elevated to Pro during an active trial) as
	// `plan` so tier-based gates (e.g. integration-core requires "pro") pass
	// pre-paywall; `base_plan` preserves the org-mirrored stored plan.
	// Resolved, not raw: includes D-A billing-group inheritance so `plan` cannot
	// disagree with `allowed`, which CanUseFeature already resolves the same way.
	effectivePlan := s.billingCore.ResolveEffectivePlan(
		c.Request.Context(), account, time.Now().UTC(),
	)
	c.JSON(status, gin.H{
		"org_id":         orgID,
		"feature":        feature,
		"allowed":        allowed,
		"plan":           effectivePlan,
		"base_plan":      account.Plan,
		"effective_plan": effectivePlan,
		"trialing":       account.SubscriptionState == billing.SubscriptionStateTrialing && account.TrialEndsAt != nil,
		"trial_ends_at":  account.TrialEndsAt,
		"required":       !allowed,
	})
}

func (s *Server) getQuotaStatus(c *gin.Context) {
	orgID := c.Param("orgId")
	metric := c.Param("metric")

	status, err := s.billingCore.GetQuotaStatus(c.Request.Context(), orgID, metric)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to get quota status"})
		return
	}

	remainingFormatted := strconv.FormatFloat(status.Remaining, 'f', -1, 64)
	c.JSON(http.StatusOK, gin.H{
		"quota":              status,
		"remaining_readable": remainingFormatted,
	})
}

func (s *Server) createInvoice(c *gin.Context) {
	orgID := c.Param("orgId")

	var req struct {
		Provider    string                 `json:"provider,omitempty"`
		AmountCents int64                  `json:"amount_cents" binding:"required"`
		Currency    string                 `json:"currency,omitempty"`
		DueAt       string                 `json:"due_at,omitempty"`
		AutoCharge  bool                   `json:"auto_charge"`
		Metadata    map[string]interface{} `json:"metadata,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "amount_cents is required"})
		return
	}

	dueAt := time.Time{}
	if req.DueAt != "" {
		parsed, err := time.Parse(time.RFC3339, req.DueAt)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "due_at must be RFC3339 format"})
			return
		}
		dueAt = parsed
	}

	err := s.billingCore.CreateInvoice(c.Request.Context(), billing.Invoice{
		OrgID:       orgID,
		Provider:    req.Provider,
		AmountCents: req.AmountCents,
		Currency:    req.Currency,
		DueAt:       dueAt,
		Metadata:    req.Metadata,
	}, req.AutoCharge)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to create invoice"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{"status": "invoice created"})
}

func (s *Server) createCheckoutSession(c *gin.Context) {
	orgID := c.Param("orgId")

	var req struct {
		Plan       string `json:"plan" binding:"required"`
		SuccessURL string `json:"success_url" binding:"required"`
		CancelURL  string `json:"cancel_url" binding:"required"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "plan, success_url, and cancel_url are required"})
		return
	}

	session, err := s.billingCore.CreateCheckoutSession(
		c.Request.Context(),
		orgID,
		req.Plan,
		req.SuccessURL,
		req.CancelURL,
	)
	if err != nil {
		message := err.Error()
		switch {
		case strings.Contains(message, "required"),
			strings.Contains(message, "supported"),
			strings.Contains(message, "invalid"):
			c.JSON(http.StatusBadRequest, gin.H{"error": message})
		default:
			// The client is deliberately told nothing beyond "failed" — the
			// provider error can carry account detail. But discarding it
			// server-side too left a 502 with no cause anywhere in the fleet,
			// which is unowned-diagnosis by construction. Log it here; the
			// response body is unchanged.
			log.Printf("checkout-session create failed org=%s plan=%s: %v", orgID, req.Plan, err)
			c.JSON(http.StatusBadGateway, gin.H{"error": "failed to create checkout session"})
		}
		return
	}

	c.JSON(http.StatusCreated, session)
}

func (s *Server) confirmCheckoutSession(c *gin.Context) {
	orgID := c.Param("orgId")

	var req struct {
		Plan         string `json:"plan" binding:"required"`
		PaymentID    string `json:"payment_id,omitempty"`
		ClientSecret string `json:"client_secret,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "plan and payment reference are required"})
		return
	}
	if strings.TrimSpace(req.PaymentID) == "" && strings.TrimSpace(req.ClientSecret) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "payment_id or client_secret is required"})
		return
	}

	status, err := s.billingCore.ConfirmCheckoutSession(
		c.Request.Context(),
		orgID,
		req.Plan,
		req.PaymentID,
		req.ClientSecret,
	)
	if err != nil {
		message := err.Error()
		switch {
		case strings.Contains(message, "required"),
			strings.Contains(message, "supported"),
			strings.Contains(message, "mismatch"),
			strings.Contains(message, "not configured"):
			c.JSON(http.StatusBadRequest, gin.H{"error": message})
		default:
			c.JSON(http.StatusBadGateway, gin.H{"error": "failed to confirm checkout session"})
		}
		return
	}

	c.JSON(http.StatusOK, status)
}

// nexiWebhook receives Nexi Checkout payment webhooks. Nexi authenticates the
// callback by echoing the per-webhook `authorization` string we registered when
// creating the payment; we verify it (constant-time) against
// NEXI_WEBHOOK_AUTHORIZATION and FAIL CLOSED when the secret is unset — an
// unverifiable webhook must never activate a paid plan. On a paid event we
// activate the plan from the payment itself (org/plan derived server-side, not
// trusted from the request body).
func (s *Server) nexiWebhook(c *gin.Context) {
	// The shared secret Nexi echoes back. Primary env name matches nettbutikk
	// (NEXI_WEBHOOK_SECRET); NEXI_WEBHOOK_AUTHORIZATION kept as a fallback.
	expected := strings.TrimSpace(os.Getenv("NEXI_WEBHOOK_SECRET"))
	if expected == "" {
		expected = strings.TrimSpace(os.Getenv("NEXI_WEBHOOK_AUTHORIZATION"))
	}
	if expected == "" {
		log.Println("nexi webhook rejected: NEXI_WEBHOOK_SECRET not configured")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "webhook not configured"})
		return
	}
	provided := strings.TrimSpace(c.GetHeader("Authorization"))
	if provided == "" || subtle.ConstantTimeCompare([]byte(provided), []byte(expected)) != 1 {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid webhook authorization"})
		return
	}

	var event struct {
		ID    string `json:"id"`
		Event string `json:"event"`
		Data  struct {
			PaymentID string `json:"paymentId"`
		} `json:"data"`
	}
	if err := c.ShouldBindJSON(&event); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid webhook payload"})
		return
	}
	if strings.TrimSpace(event.Data.PaymentID) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "missing paymentId"})
		return
	}

	// Only payment-confirming events activate a plan; acknowledge others so Nexi
	// does not retry them.
	switch event.Event {
	case "payment.checkout.completed", "payment.charge.created", "payment.charge.created.v2":
		if _, err := s.billingCore.ConfirmCheckoutByPaymentID(c.Request.Context(), event.Data.PaymentID); err != nil {
			log.Printf("nexi webhook %s payment=%s activation failed: %v", event.Event, event.Data.PaymentID, err)
			// 502 so Nexi retries a transient failure; the operation is idempotent.
			c.JSON(http.StatusBadGateway, gin.H{"error": "activation failed"})
			return
		}
	default:
		// Acknowledge unhandled events (e.g. refund, cancel) without acting.
	}

	c.JSON(http.StatusOK, gin.H{"received": true})
}
