package http

import (
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
)

func (s *Server) health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":    "healthy",
		"service":   "billing-core",
		"timestamp": time.Now().UTC().Format(time.RFC3339),
	})
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
		EventID    string                 `json:"event_id,omitempty"`
		Metric     string                 `json:"metric" binding:"required"`
		Quantity   float64                `json:"quantity" binding:"required"`
		Source     string                 `json:"source,omitempty"`
		OccurredAt string                 `json:"occurred_at,omitempty"`
		Metadata   map[string]interface{} `json:"metadata,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "metric and quantity are required"})
		return
	}

	occurredAt := time.Now().UTC()
	if req.OccurredAt != "" {
		parsed, err := time.Parse(time.RFC3339, req.OccurredAt)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "occurred_at must be RFC3339 format"})
			return
		}
		occurredAt = parsed
	}

	if err := s.billingCore.RecordUsage(c.Request.Context(), billing.UsageEvent{
		EventID:    req.EventID,
		OrgID:      orgID,
		Metric:     req.Metric,
		Quantity:   req.Quantity,
		Source:     req.Source,
		OccurredAt: occurredAt,
		Metadata:   req.Metadata,
	}); err != nil {
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
	effectivePlan := billing.EffectivePlan(account, time.Now().UTC())
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
			c.JSON(http.StatusBadGateway, gin.H{"error": "failed to create stripe checkout session"})
		}
		return
	}

	c.JSON(http.StatusCreated, session)
}
