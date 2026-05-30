package billing

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"os"
	"strings"
	"time"

	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/redis"
)

const (
	// billingCacheTTL is how long billing account data is cached in Redis.
	// CanUseFeature and GetQuotaStatus both call GetAccount per request — caching
	// for 5m reduces DB load without meaningful freshness impact.
	billingCacheTTL = 5 * time.Minute
	// metricUsageCacheTTL caches GetMetricUsage aggregates for a short window.
	// Usage totals change infrequently within seconds; 30s keeps quota checks fast
	// without meaningfully delaying enforcement.
	metricUsageCacheTTL = 30 * time.Second
)

type Service struct {
	repo            *Repository
	paymentAdapter  PaymentAdapter
	invoiceAdapter  InvoiceAdapter
	publisher       EventPublisher
	sharedPublisher SharedEventPublisher // cross-plane events on velion-nats
	cache           *rediscache.Client   // optional, nil if Redis disabled
	httpClient      *http.Client
	orgServiceURL   string
	internalAPIKey  string
}

func NewService(repo *Repository, paymentAdapter PaymentAdapter, invoiceAdapter InvoiceAdapter, cache ...*rediscache.Client) *Service {
	svc := &Service{
		repo:           repo,
		paymentAdapter: paymentAdapter,
		invoiceAdapter: invoiceAdapter,
		httpClient:     &http.Client{Timeout: 5 * time.Second},
		orgServiceURL:  strings.TrimRight(strings.TrimSpace(os.Getenv("ORG_SERVICE_URL")), "/"),
		internalAPIKey: strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
	}

	if svc.orgServiceURL == "" {
		svc.orgServiceURL = "http://org-core:8080"
	}

	if svc.internalAPIKey == "" {
		svc.internalAPIKey = strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET"))
	}

	if len(cache) > 0 {
		svc.cache = cache[0]
	}
	return svc
}

func (s *Service) SetPublisher(publisher EventPublisher) {
	s.publisher = publisher
}

// SetSharedPublisher wires the cross-plane NATS publisher for controlplane.billing.* subjects.
// SharedEventPublisher is defined in this package to avoid an import cycle.
func (s *Service) SetSharedPublisher(sp SharedEventPublisher) {
	s.sharedPublisher = sp
}

type orgCoreOrganizationSeed struct {
	Name string `json:"name"`
	Plan string `json:"plan"`
}

type orgCoreEntitlementSeed struct {
	Key     string `json:"key"`
	Enabled bool   `json:"enabled"`
}

type orgCoreEntitlementsSeedResponse struct {
	Entitlements []orgCoreEntitlementSeed `json:"entitlements"`
}

func (s *Service) fetchOrgCoreSeed(
	ctx context.Context,
	orgID string,
) (orgCoreOrganizationSeed, map[string]bool) {
	seed := orgCoreOrganizationSeed{}
	entitlements := map[string]bool{}

	if s.orgServiceURL == "" || orgID == "" {
		return seed, entitlements
	}

	buildRequest := func(path string) (*http.Request, error) {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.orgServiceURL+path, nil)
		if err != nil {
			return nil, err
		}
		req.Header.Set("Content-Type", "application/json")
		if s.internalAPIKey != "" {
			req.Header.Set("X-Internal-Api-Key", s.internalAPIKey)
		}
		return req, nil
	}

	orgReq, err := buildRequest("/orgs/" + orgID)
	if err == nil {
		if orgResp, doErr := s.httpClient.Do(orgReq); doErr == nil {
			defer orgResp.Body.Close()
			if orgResp.StatusCode == http.StatusOK {
				var orgPayload orgCoreOrganizationSeed
				if decodeErr := json.NewDecoder(orgResp.Body).Decode(&orgPayload); decodeErr == nil {
					seed = orgPayload
					seed.Plan = normalizePlan(orgPayload.Plan)
				}
			}
		}
	}

	entReq, err := buildRequest("/orgs/" + orgID + "/entitlements")
	if err == nil {
		if entResp, doErr := s.httpClient.Do(entReq); doErr == nil {
			defer entResp.Body.Close()
			if entResp.StatusCode == http.StatusOK {
				var entPayload orgCoreEntitlementsSeedResponse
				if decodeErr := json.NewDecoder(entResp.Body).Decode(&entPayload); decodeErr == nil {
					for _, entitlement := range entPayload.Entitlements {
						entitlements[entitlement.Key] = entitlement.Enabled
					}
				}
			}
		}
	}

	return seed, entitlements
}

func (s *Service) hydrateAccountDefaults(ctx context.Context, account Account) (Account, bool) {
	next := account
	changed := false

	orgSeed, seededEntitlements := s.fetchOrgCoreSeed(ctx, account.OrgID)
	seedPlan := orgSeed.Plan
	if seedPlan == "" {
		seedPlan = normalizePlan(next.Plan)
	}

	planChanged := next.Plan != seedPlan
	if planChanged {
		next.Plan = seedPlan
		changed = true
	}

	if next.FeatureFlags == nil {
		next.FeatureFlags = map[string]bool{}
	}
	if next.Products == nil {
		next.Products = map[string]bool{}
	}
	if next.ProviderCustomerID == nil {
		next.ProviderCustomerID = map[string]string{}
	}
	if next.Metadata == nil {
		next.Metadata = map[string]interface{}{}
	}
	if next.Entitlements == nil {
		next.Entitlements = map[string]bool{}
	}
	if next.QuotaLimits == nil {
		next.QuotaLimits = map[string]float64{}
	}

	defaultEntitlements := seededEntitlements
	if len(defaultEntitlements) == 0 {
		defaultEntitlements = defaultEntitlementsForPlan(seedPlan)
	}
	for key, enabled := range defaultEntitlements {
		current, exists := next.Entitlements[key]
		if !exists || planChanged || current != enabled {
			next.Entitlements[key] = enabled
			changed = true
		}
	}

	defaultQuotas := defaultQuotaLimitsForPlan(seedPlan)
	for key, limit := range defaultQuotas {
		current, exists := next.QuotaLimits[key]
		if !exists || planChanged || current != limit {
			next.QuotaLimits[key] = limit
			changed = true
		}
	}

	return next, changed
}

func (s *Service) persistAccount(ctx context.Context, account Account) error {
	if account.OrgID == "" {
		return fmt.Errorf("org_id is required")
	}

	if account.Plan == "" {
		account.Plan = "free"
	}

	if account.SubscriptionState == "" {
		account.SubscriptionState = SubscriptionStateActive
	}

	if account.Products == nil {
		account.Products = map[string]bool{}
	}
	if account.FeatureFlags == nil {
		account.FeatureFlags = map[string]bool{}
	}
	if account.Entitlements == nil {
		account.Entitlements = map[string]bool{}
	}
	if account.QuotaLimits == nil {
		account.QuotaLimits = map[string]float64{}
	}
	if account.ProviderCustomerID == nil {
		account.ProviderCustomerID = map[string]string{}
	}
	if account.Metadata == nil {
		account.Metadata = map[string]interface{}{}
	}

	if err := s.repo.UpsertAccount(ctx, account); err != nil {
		return err
	}

	// Invalidate cached account on write
	if s.cache != nil {
		_ = s.cache.Del(ctx, "billing:account:"+account.OrgID)
	}

	if s.publisher != nil {
		_ = s.publisher.Publish(ctx, "billing.account.updated", map[string]any{
			"org_id":             account.OrgID,
			"plan":               account.Plan,
			"subscription_state": account.SubscriptionState,
			"timestamp":          time.Now().UTC().Format(time.RFC3339),
		})
	}

	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishAccountUpdated(ctx, account.OrgID, account.Plan, 0)
	}

	return nil
}

func (s *Service) syncStripeCustomer(ctx context.Context, account Account) (Account, error) {
	if s.paymentAdapter == nil {
		return account, nil
	}

	orgSeed, _ := s.fetchOrgCoreSeed(ctx, account.OrgID)
	existingCustomerID := strings.TrimSpace(account.ProviderCustomerID["stripe"])
	billingEmail, _ := account.Metadata["billing_email"].(string)
	orgName := strings.TrimSpace(orgSeed.Name)
	if orgName == "" {
		if metadataOrgName, ok := account.Metadata["org_name"].(string); ok {
			orgName = strings.TrimSpace(metadataOrgName)
		}
	}

	customerID, err := s.paymentAdapter.EnsureCustomer(ctx, StripeCustomerInput{
		OrgID:              account.OrgID,
		OrganizationName:   orgName,
		BillingEmail:       strings.TrimSpace(billingEmail),
		ExistingCustomerID: existingCustomerID,
		Metadata: map[string]string{
			"plan": account.Plan,
		},
	})
	if err != nil {
		return account, err
	}

	if customerID != "" && customerID != existingCustomerID {
		account.ProviderCustomerID["stripe"] = customerID
	}
	if orgName != "" {
		account.Metadata["org_name"] = orgName
	}

	return account, nil
}

func (s *Service) UpsertAccount(ctx context.Context, account Account) error {
	syncedAccount, err := s.syncStripeCustomer(ctx, account)
	if err != nil {
		return err
	}

	return s.persistAccount(ctx, syncedAccount)
}

func (s *Service) SyncOrganization(ctx context.Context, orgID, orgName string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("org_id is required")
	}

	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return err
	}
	if account.Metadata == nil {
		account.Metadata = map[string]interface{}{}
	}
	if trimmedName := strings.TrimSpace(orgName); trimmedName != "" {
		account.Metadata["org_name"] = trimmedName
	}

	return s.UpsertAccount(ctx, account)
}

func (s *Service) ApplyPlanChange(ctx context.Context, orgID, orgName, newPlan string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("org_id is required")
	}

	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return err
	}

	previousPlan := normalizePlan(account.Plan)
	account.Plan = normalizePlan(newPlan)
	if account.Metadata == nil {
		account.Metadata = map[string]interface{}{}
	}
	if trimmedName := strings.TrimSpace(orgName); trimmedName != "" {
		account.Metadata["org_name"] = trimmedName
	}

	hydratedAccount, _ := s.hydrateAccountDefaults(ctx, account)
	if err := s.UpsertAccount(ctx, hydratedAccount); err != nil {
		return err
	}

	if previousPlan != hydratedAccount.Plan {
		if s.publisher != nil {
			_ = s.publisher.Publish(ctx, "billing.plan.changed", map[string]any{
				"org_id":        hydratedAccount.OrgID,
				"previous_plan": previousPlan,
				"new_plan":      hydratedAccount.Plan,
				"timestamp":     time.Now().UTC().Format(time.RFC3339),
			})
		}
		if s.sharedPublisher != nil {
			s.sharedPublisher.PublishPlanChanged(ctx, hydratedAccount.OrgID, previousPlan, hydratedAccount.Plan)
		}
	}

	return nil
}

func (s *Service) DeactivateOrganization(ctx context.Context, orgID, reason string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("org_id is required")
	}

	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return err
	}
	account.SubscriptionState = SubscriptionStateCanceled
	if account.Metadata == nil {
		account.Metadata = map[string]interface{}{}
	}
	if trimmedReason := strings.TrimSpace(reason); trimmedReason != "" {
		account.Metadata["deactivated_reason"] = trimmedReason
	}

	return s.persistAccount(ctx, account)
}

func (s *Service) GetAccount(ctx context.Context, orgID string) (Account, error) {
	if orgID == "" {
		return Account{}, fmt.Errorf("org_id is required")
	}

	// Cache-aside: this is called by every quota/entitlement check
	if s.cache != nil {
		key := "billing:account:" + orgID
		if cached, err := s.cache.Get(ctx, key); err == nil {
			var acc Account
			if json.Unmarshal([]byte(cached), &acc) == nil {
				if hydratedAccount, changed := s.hydrateAccountDefaults(ctx, acc); changed {
					if upsertErr := s.persistAccount(ctx, hydratedAccount); upsertErr == nil {
						return hydratedAccount, nil
					}
				}
				return acc, nil
			}
		}
	}

	account, err := s.repo.GetAccount(ctx, orgID)
	if err == ErrNotFound {
		defaultAccount, _ := s.hydrateAccountDefaults(ctx, NewDefaultAccount(orgID))
		if upsertErr := s.persistAccount(ctx, defaultAccount); upsertErr != nil {
			return Account{}, upsertErr
		}
		return defaultAccount, nil
	}

	if err != nil {
		return Account{}, err
	}

	if hydratedAccount, changed := s.hydrateAccountDefaults(ctx, account); changed {
		if upsertErr := s.persistAccount(ctx, hydratedAccount); upsertErr != nil {
			return Account{}, upsertErr
		}
		account = hydratedAccount
	}

	if s.cache != nil {
		if b, merr := json.Marshal(account); merr == nil {
			_ = s.cache.Set(ctx, "billing:account:"+orgID, string(b), billingCacheTTL)
		}
	}

	return account, nil
}

func (s *Service) CreateCheckoutSession(
	ctx context.Context,
	orgID, plan, successURL, cancelURL string,
) (CheckoutSession, error) {
	if strings.TrimSpace(orgID) == "" {
		return CheckoutSession{}, fmt.Errorf("org_id is required")
	}
	plan = billablePlan(plan)
	if plan == "free" {
		return CheckoutSession{}, fmt.Errorf("checkout is only supported for paid plans")
	}
	if strings.TrimSpace(successURL) == "" || strings.TrimSpace(cancelURL) == "" {
		return CheckoutSession{}, fmt.Errorf("success and cancel urls are required")
	}

	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return CheckoutSession{}, err
	}

	account, err = s.syncStripeCustomer(ctx, account)
	if err != nil {
		return CheckoutSession{}, err
	}
	if err := s.persistAccount(ctx, account); err != nil {
		return CheckoutSession{}, err
	}

	orgName, _ := account.Metadata["org_name"].(string)
	session, err := s.paymentAdapter.CreateCheckoutSession(ctx, StripeCheckoutParams{
		OrgID:        orgID,
		Plan:         plan,
		CustomerID:   strings.TrimSpace(account.ProviderCustomerID["stripe"]),
		SuccessURL:   successURL,
		CancelURL:    cancelURL,
		Organization: strings.TrimSpace(orgName),
		Metadata: map[string]string{
			"plan": plan,
		},
	})
	if err != nil {
		return CheckoutSession{}, err
	}

	return session, nil
}

func (s *Service) RecordUsage(ctx context.Context, usage UsageEvent) error {
	if usage.OrgID == "" || usage.Metric == "" {
		return fmt.Errorf("org_id and metric are required")
	}
	if usage.OccurredAt.IsZero() {
		usage.OccurredAt = time.Now().UTC()
	}
	if usage.EventID == "" {
		usage.EventID = s.newUsageEventID(usage)
	}
	if usage.Source == "" {
		usage.Source = "unknown"
	}
	if usage.Metadata == nil {
		usage.Metadata = map[string]interface{}{}
	}

	isNew, err := s.repo.ReserveUsageEvent(ctx, usage)
	if err != nil {
		return err
	}
	if !isNew {
		return nil
	}

	if err := s.repo.SaveUsage(ctx, usage); err != nil {
		return err
	}

	if err := s.invoiceAdapter.ReportUsage(ctx, usage); err != nil {
		if enqueueErr := s.enqueueUsageRetry(ctx, usage); enqueueErr != nil {
			return fmt.Errorf("report usage failed and enqueue retry failed: %w", enqueueErr)
		}
	}

	if s.publisher != nil {
		_ = s.publisher.Publish(ctx, "billing.usage.recorded", map[string]any{
			"event_id":    usage.EventID,
			"org_id":      usage.OrgID,
			"metric":      usage.Metric,
			"quantity":    usage.Quantity,
			"source":      usage.Source,
			"occurred_at": usage.OccurredAt.Format(time.RFC3339),
		})
	}

	return nil
}

func (s *Service) newUsageEventID(usage UsageEvent) string {
	raw := fmt.Sprintf("%s|%s|%f|%s|%d", usage.OrgID, usage.Metric, usage.Quantity, usage.Source, usage.OccurredAt.UnixNano())
	sum := sha256.Sum256([]byte(raw))
	return "evt_" + hex.EncodeToString(sum[:])
}

func (s *Service) CanUseFeature(ctx context.Context, orgID, feature string) (bool, Account, error) {
	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return false, Account{}, err
	}

	if feature == "" {
		return false, account, nil
	}

	if account.Entitlements[feature] {
		return true, account, nil
	}
	if account.FeatureFlags[feature] {
		return true, account, nil
	}
	if account.Products[feature] {
		return true, account, nil
	}

	return false, account, nil
}

func (s *Service) GetQuotaStatus(ctx context.Context, orgID, metric string) (QuotaStatus, error) {
	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return QuotaStatus{}, err
	}

	limit := account.QuotaLimits[metric]

	// Cache usage aggregates for 30s — avoids a DB query on every quota check
	var used float64
	if s.cache != nil {
		usageKey := "billing:usage:" + orgID + ":" + metric
		if cached, err := s.cache.Get(ctx, usageKey); err == nil {
			_ = json.Unmarshal([]byte(cached), &used)
		} else {
			used, err = s.repo.GetMetricUsage(ctx, orgID, metric)
			if err != nil {
				return QuotaStatus{}, err
			}
			if b, merr := json.Marshal(used); merr == nil {
				_ = s.cache.Set(ctx, usageKey, string(b), metricUsageCacheTTL)
			}
		}
	} else {
		used, err = s.repo.GetMetricUsage(ctx, orgID, metric)
		if err != nil {
			return QuotaStatus{}, err
		}
	}

	remaining := limit - used
	if remaining < 0 {
		remaining = 0
	}

	utilization := 0.0
	if limit > 0 {
		utilization = math.Min(used/limit, 1.0)
	}

	return QuotaStatus{
		OrgID:       orgID,
		Metric:      metric,
		Limit:       limit,
		Used:        used,
		Remaining:   remaining,
		IsExceeded:  limit > 0 && used > limit,
		Utilization: utilization,
	}, nil
}

func (s *Service) CreateInvoice(ctx context.Context, invoice Invoice, autoCharge bool) error {
	if invoice.InvoiceID == "" {
		invoice.InvoiceID = fmt.Sprintf("inv_%d", time.Now().UnixNano()/1_000_000)
	}
	if invoice.IssuedAt.IsZero() {
		invoice.IssuedAt = time.Now().UTC()
	}
	if invoice.DueAt.IsZero() {
		invoice.DueAt = invoice.IssuedAt.Add(7 * 24 * time.Hour)
	}
	if invoice.Provider == "" {
		invoice.Provider = "lago"
	}
	if invoice.Currency == "" {
		invoice.Currency = "NOK"
	}
	if invoice.Status == "" {
		invoice.Status = "open"
	}
	if invoice.Metadata == nil {
		invoice.Metadata = map[string]interface{}{}
	}

	if err := s.repo.SaveInvoice(ctx, invoice); err != nil {
		return err
	}

	if autoCharge {
		if err := s.paymentAdapter.ChargeInvoice(ctx, invoice); err != nil {
			if enqueueErr := s.enqueueInvoiceChargeRetry(ctx, invoice); enqueueErr != nil {
				return fmt.Errorf("charge invoice failed and enqueue retry failed: %w", enqueueErr)
			}
		}
	}

	if s.publisher != nil {
		_ = s.publisher.Publish(ctx, "billing.invoice.created", map[string]any{
			"org_id":       invoice.OrgID,
			"invoice_id":   invoice.InvoiceID,
			"amount_cents": invoice.AmountCents,
			"currency":     invoice.Currency,
			"provider":     invoice.Provider,
		})
	}

	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishInvoiceCreated(ctx, invoice.OrgID, invoice.InvoiceID, invoice.AmountCents, invoice.Currency)
	}

	return nil
}

func (s *Service) enqueueUsageRetry(ctx context.Context, usage UsageEvent) error {
	payload := map[string]interface{}{
		"event_id":    usage.EventID,
		"org_id":      usage.OrgID,
		"metric":      usage.Metric,
		"quantity":    usage.Quantity,
		"source":      usage.Source,
		"occurred_at": usage.OccurredAt.UTC().Format(time.RFC3339),
		"metadata":    usage.Metadata,
	}
	dedupeKey := string(RetryJobKindLagoUsage) + ":" + usage.EventID
	return s.repo.EnqueueRetryJob(ctx, RetryJobKindLagoUsage, dedupeKey, payload, time.Now().UTC())
}

func (s *Service) enqueueInvoiceChargeRetry(ctx context.Context, invoice Invoice) error {
	payload := map[string]interface{}{
		"invoice_id":   invoice.InvoiceID,
		"org_id":       invoice.OrgID,
		"provider":     invoice.Provider,
		"amount_cents": float64(invoice.AmountCents),
		"currency":     invoice.Currency,
		"status":       invoice.Status,
		"issued_at":    invoice.IssuedAt.UTC().Format(time.RFC3339),
		"due_at":       invoice.DueAt.UTC().Format(time.RFC3339),
		"metadata":     invoice.Metadata,
	}
	dedupeKey := string(RetryJobKindStripeCharge) + ":" + invoice.InvoiceID
	return s.repo.EnqueueRetryJob(ctx, RetryJobKindStripeCharge, dedupeKey, payload, time.Now().UTC())
}
