package billing

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
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
	sharedPublisher SharedEventPublisher // cross-plane events on verevon-nats
	cache           *rediscache.Client   // optional, nil if Redis disabled
	httpClient      *http.Client
	orgServiceURL   string
	orgServiceToken string
}

func NewService(repo *Repository, paymentAdapter PaymentAdapter, invoiceAdapter InvoiceAdapter, cache ...*rediscache.Client) *Service {
	svc := &Service{
		repo:           repo,
		paymentAdapter: paymentAdapter,
		invoiceAdapter: invoiceAdapter,
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
		orgServiceURL:   strings.TrimRight(strings.TrimSpace(os.Getenv("ORG_SERVICE_URL")), "/"),
		orgServiceToken: strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_TOKEN")),
	}

	if svc.orgServiceURL == "" {
		svc.orgServiceURL = "http://org-core:8080"
	}

	if len(cache) > 0 {
		svc.cache = cache[0]
	}
	return svc
}

func (s *Service) SetPublisher(publisher EventPublisher) {
	s.publisher = publisher
}

// Ping checks database connectivity + authentication for the /health probe.
func (s *Service) Ping(ctx context.Context) error {
	return s.repo.Ping(ctx)
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
		if s.orgServiceToken != "" {
			req.Header.Set("X-Service-Id", "billing-core")
			req.Header.Set("X-Service-Token", s.orgServiceToken)
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

	// During an active onboarding trial the effective plan is elevated to Pro
	// so entitlements (e.g. feature.integrations) unlock connector setup before
	// the paywall. The elevated defaults take precedence over the org-core seed.
	entitlementPlan := seedPlan
	trialing := trialActive(next, time.Now().UTC())
	if trialing {
		entitlementPlan = TrialPlan
	}

	defaultEntitlements := seededEntitlements
	if trialing || len(defaultEntitlements) == 0 {
		defaultEntitlements = defaultEntitlementsForPlan(entitlementPlan)
	}
	for key, enabled := range defaultEntitlements {
		current, exists := next.Entitlements[key]
		if !exists || planChanged || current != enabled {
			next.Entitlements[key] = enabled
			changed = true
		}
	}

	defaultQuotas := defaultQuotaLimitsForPlan(entitlementPlan)
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

	if err := s.repo.SaveAccountStateCAS(ctx, account); err != nil {
		return err
	}

	// Invalidate cached account on write
	if s.cache != nil {
		_ = s.cache.Del(ctx, "billing:account:"+account.OrgID)
	}

	// Downstream consumers (org-core's plan mirror, cross-plane feature
	// gates) need the plan actually in effect right now, not the raw stored
	// value — during an active trial those differ (stored stays "free" until
	// checkout, but EffectivePlan elevates to TrialPlan so pre-paywall
	// connector setup works). Publishing account.Plan here was the root
	// cause of org-core's organizations.plan never reflecting a trial start.
	effectivePlan := EffectivePlan(account, time.Now())

	if s.publisher != nil {
		_ = s.publisher.Publish(ctx, "billing.account.updated", map[string]any{
			"org_id":             account.OrgID,
			"plan":               effectivePlan,
			"subscription_state": account.SubscriptionState,
			"timestamp":          time.Now().UTC().Format(time.RFC3339),
		})
	}

	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishAccountUpdated(ctx, account.OrgID, effectivePlan, 0)
	}

	return nil
}

func (s *Service) syncPaymentCustomer(ctx context.Context, account Account) (Account, error) {
	if s.paymentAdapter == nil {
		return account, nil
	}

	orgSeed, _ := s.fetchOrgCoreSeed(ctx, account.OrgID)
	existingCustomerID := strings.TrimSpace(account.ProviderCustomerID["payment"])
	billingEmail, _ := account.Metadata["billing_email"].(string)
	orgName := strings.TrimSpace(orgSeed.Name)
	if orgName == "" {
		if metadataOrgName, ok := account.Metadata["org_name"].(string); ok {
			orgName = strings.TrimSpace(metadataOrgName)
		}
	}

	customerID, err := s.paymentAdapter.EnsureCustomer(ctx, CustomerInput{
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
		account.ProviderCustomerID["payment"] = customerID
	}
	if orgName != "" {
		account.Metadata["org_name"] = orgName
	}

	return account, nil
}

func (s *Service) UpsertAccount(ctx context.Context, account Account) error {
	tombstoned, err := s.repo.IsOrganizationTombstoned(ctx, account.OrgID)
	if err != nil {
		return err
	}
	if err := ensureBillingOrganizationActive(tombstoned); err != nil {
		return err
	}
	syncedAccount, err := s.syncPaymentCustomer(ctx, account)
	if err != nil {
		return err
	}

	return s.persistAccount(ctx, syncedAccount)
}

func ensureBillingOrganizationActive(tombstoned bool) error {
	if tombstoned {
		return ErrOrganizationDeleted
	}
	return nil
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

	// Start the 14-day Pro trial exactly once per organization. The stored plan
	// stays mirrored from org-core; the trial elevates the effective plan +
	// entitlements until the paywall choice (ApplyPlanChange) or expiry sweep.
	if started, _ := account.Metadata["trial_started"].(bool); !started {
		now := time.Now().UTC()
		ends := now.Add(time.Duration(trialDurationDays()) * 24 * time.Hour)
		account.SubscriptionState = SubscriptionStateTrialing
		account.TrialEndsAt = &ends
		account.Metadata["trial_started"] = true
		account.Metadata["trial_started_at"] = now.Format(time.RFC3339)
		account.Metadata["downgrade_to"] = "free"
	}

	hydrated, _ := s.hydrateAccountDefaults(ctx, account)
	return s.UpsertAccount(ctx, hydrated)
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
	// A plan choice (the paywall) ends the onboarding trial — settle into the
	// chosen plan and drop the trial window so the sweep won't touch it.
	wasTrialing := account.SubscriptionState == SubscriptionStateTrialing
	account.SubscriptionState = SubscriptionStateActive
	account.TrialEndsAt = nil
	if account.Metadata == nil {
		account.Metadata = map[string]interface{}{}
	}
	if wasTrialing {
		account.Metadata["trial_converted_at"] = time.Now().UTC().Format(time.RFC3339)
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
			_ = s.sharedPublisher.PublishPlanChanged(ctx, hydratedAccount.OrgID, previousPlan, hydratedAccount.Plan)
		}
	}

	return nil
}

// ApplyOrganizationPlanChange is the canonical Org event path. The repository
// compares and writes the revision in the same transaction as the account
// mutation, so concurrent, duplicate, and reordered deliveries cannot regress
// Billing state. Checkout/provider flows continue to use ApplyPlanChange.
func (s *Service) ApplyOrganizationPlanChange(
	ctx context.Context,
	orgID, orgName, newPlan string,
	revision int64,
) (bool, error) {
	if strings.TrimSpace(orgID) == "" {
		return false, fmt.Errorf("org_id is required")
	}
	if revision < 1 {
		return false, fmt.Errorf("plan revision must be positive")
	}
	canonicalPlan := strings.ToLower(strings.TrimSpace(newPlan))
	switch canonicalPlan {
	case "free", "trial", "hobby", "standard", "pro", "enterprise":
	default:
		return false, fmt.Errorf("invalid canonical organization plan")
	}

	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return false, err
	}
	if account.PlanRevision > revision {
		return false, nil
	}
	if account.PlanRevision == revision {
		if normalizePlan(account.Plan) != canonicalPlan {
			return false, fmt.Errorf("plan revision %d conflicts with existing plan", revision)
		}
		if account.SubscriptionState == SubscriptionStateCanceled {
			return false, ErrOrganizationDeleted
		}
		if s.sharedPublisher == nil {
			return false, fmt.Errorf("billing shared plan event publisher is unavailable")
		}
		if err := s.sharedPublisher.PublishPlanChanged(
			ctx, orgID, canonicalPlan, canonicalPlan, revision,
		); err != nil {
			return false, err
		}
		return false, nil
	}
	next := cloneAccount(account)
	next.Plan = canonicalPlan
	next.PlanRevision = revision
	next.SubscriptionState = SubscriptionStateActive
	next.TrialEndsAt = nil
	if trimmedName := strings.TrimSpace(orgName); trimmedName != "" {
		next.Metadata["org_name"] = trimmedName
	}
	for key, enabled := range defaultEntitlementsForPlan(canonicalPlan) {
		next.Entitlements[key] = enabled
	}
	for key, limit := range defaultQuotaLimitsForPlan(canonicalPlan) {
		next.QuotaLimits[key] = limit
	}

	applied, err := s.repo.ApplyOrganizationPlanRevision(ctx, next, revision)
	if err != nil || !applied {
		return applied, err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "billing:account:"+orgID)
	}
	if s.publisher != nil {
		if err := s.publisher.Publish(ctx, "billing.plan.changed", map[string]any{
			"org_id":        orgID,
			"previous_plan": account.Plan,
			"new_plan":      canonicalPlan,
			"revision":      revision,
			"timestamp":     time.Now().UTC().Format(time.RFC3339),
		}); err != nil {
			log.Printf("billing-core failed to publish derived billing.plan.changed for org %s revision %d: %v", orgID, revision, err)
		}
	}
	if s.sharedPublisher == nil {
		return true, fmt.Errorf("billing shared plan event publisher is unavailable")
	}
	if err := s.sharedPublisher.PublishPlanChanged(
		ctx, orgID, normalizePlan(account.Plan), canonicalPlan, revision,
	); err != nil {
		return true, err
	}
	return true, nil
}

func cloneAccount(account Account) Account {
	next := account
	next.Products = cloneMap(account.Products)
	next.FeatureFlags = cloneMap(account.FeatureFlags)
	next.Entitlements = cloneMap(account.Entitlements)
	next.QuotaLimits = cloneMap(account.QuotaLimits)
	next.ProviderCustomerID = cloneMap(account.ProviderCustomerID)
	next.Metadata = cloneMap(account.Metadata)
	return next
}

func cloneMap[K comparable, V any](source map[K]V) map[K]V {
	cloned := make(map[K]V, len(source))
	for key, value := range source {
		cloned[key] = value
	}
	return cloned
}

func (s *Service) DeactivateOrganization(ctx context.Context, orgID, reason string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("org_id is required")
	}

	trimmedReason := strings.TrimSpace(reason)
	if trimmedReason == "" {
		trimmedReason = "organization_deleted"
	}
	if err := s.repo.TombstoneOrganization(ctx, orgID, trimmedReason); err != nil {
		return err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "billing:account:"+orgID)
	}
	return nil
}

// ExpireTrials reverts organizations whose 14-day Pro trial has elapsed back to
// their base (free) plan: it flips subscription_state trialing→active, clears
// the trial window (so the next GetAccount hydrate drops entitlements to the
// base plan), and emits billing.plan.changed{reason: trial_expired}. Intended
// to be called periodically by a background sweep. Returns the number expired.
func (s *Service) ExpireTrials(ctx context.Context) (int, error) {
	now := time.Now().UTC()
	orgIDs, err := s.repo.ListExpiredTrials(ctx, now, 200)
	if err != nil {
		return 0, err
	}

	expired := 0
	for _, orgID := range orgIDs {
		account, err := s.GetAccount(ctx, orgID)
		if err != nil {
			continue
		}
		account.SubscriptionState = SubscriptionStateActive
		account.TrialEndsAt = nil
		if account.Metadata == nil {
			account.Metadata = map[string]interface{}{}
		}
		account.Metadata["trial_expired_at"] = now.Format(time.RFC3339)

		hydrated, _ := s.hydrateAccountDefaults(ctx, account)
		if err := s.UpsertAccount(ctx, hydrated); err != nil {
			continue
		}

		// The effective plan dropped from the trial tier (Pro) to the base plan.
		if s.publisher != nil {
			_ = s.publisher.Publish(ctx, "billing.plan.changed", map[string]any{
				"org_id":        hydrated.OrgID,
				"previous_plan": TrialPlan,
				"new_plan":      hydrated.Plan,
				"reason":        "trial_expired",
				"timestamp":     now.Format(time.RFC3339),
			})
		}
		if s.sharedPublisher != nil {
			_ = s.sharedPublisher.PublishPlanChanged(ctx, hydrated.OrgID, TrialPlan, hydrated.Plan)
		}
		expired++
	}

	return expired, nil
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
		return s.repo.GetAccount(ctx, orgID)
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

	account, err = s.syncPaymentCustomer(ctx, account)
	if err != nil {
		return CheckoutSession{}, err
	}
	if err := s.persistAccount(ctx, account); err != nil {
		return CheckoutSession{}, err
	}

	orgName, _ := account.Metadata["org_name"].(string)
	session, err := s.paymentAdapter.CreateCheckoutSession(ctx, CheckoutParams{
		OrgID:        orgID,
		Plan:         plan,
		CustomerID:   strings.TrimSpace(account.ProviderCustomerID["payment"]),
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

func (s *Service) ConfirmCheckoutSession(
	ctx context.Context,
	orgID, plan, paymentID, clientSecret string,
) (CheckoutStatus, error) {
	if strings.TrimSpace(orgID) == "" {
		return CheckoutStatus{}, fmt.Errorf("org_id is required")
	}
	plan = billablePlan(plan)
	if plan == "free" {
		return CheckoutStatus{}, fmt.Errorf("checkout is only supported for paid plans")
	}
	if s.paymentAdapter == nil {
		return CheckoutStatus{}, fmt.Errorf("payment adapter is not configured")
	}

	status, err := s.paymentAdapter.RetrieveCheckoutSession(ctx, CheckoutLookupParams{
		PaymentID:    paymentID,
		ClientSecret: clientSecret,
	})
	if err != nil {
		return CheckoutStatus{}, err
	}

	if strings.TrimSpace(status.OrgID) != "" && strings.TrimSpace(status.OrgID) != strings.TrimSpace(orgID) {
		return CheckoutStatus{}, fmt.Errorf("checkout session org mismatch")
	}
	if strings.TrimSpace(status.Plan) != "" && billablePlan(status.Plan) != plan {
		return CheckoutStatus{}, fmt.Errorf("checkout session plan mismatch")
	}
	if !checkoutStatusActivatesPlan(status.Status) {
		return status, nil
	}

	if err := s.ApplyPlanChange(ctx, orgID, "", plan); err != nil {
		return CheckoutStatus{}, err
	}
	return status, nil
}

// ConfirmCheckoutByPaymentID activates a plan from a provider webhook, where the
// caller only knows the provider paymentId (org + plan are derived from the
// payment itself, not trusted from the request). It retrieves the payment via
// the adapter, and only activates when the payment status indicates the
// customer actually paid. Idempotent: re-applying the same plan is a no-op, so
// duplicate webhook deliveries are safe.
func (s *Service) ConfirmCheckoutByPaymentID(ctx context.Context, paymentID string) (CheckoutStatus, error) {
	if strings.TrimSpace(paymentID) == "" {
		return CheckoutStatus{}, fmt.Errorf("payment_id is required")
	}
	if s.paymentAdapter == nil {
		return CheckoutStatus{}, fmt.Errorf("payment adapter is not configured")
	}

	status, err := s.paymentAdapter.RetrieveCheckoutSession(ctx, CheckoutLookupParams{PaymentID: paymentID})
	if err != nil {
		return CheckoutStatus{}, err
	}
	if !checkoutStatusActivatesPlan(status.Status) {
		return status, nil
	}

	orgID := strings.TrimSpace(status.OrgID)
	plan := billablePlan(status.Plan)
	if orgID == "" || plan == "free" {
		return CheckoutStatus{}, fmt.Errorf("payment %s missing org/plan reference", paymentID)
	}
	if err := s.ApplyPlanChange(ctx, orgID, "", plan); err != nil {
		return CheckoutStatus{}, err
	}
	return status, nil
}

func (s *Service) RecordUsage(ctx context.Context, usage UsageEvent) error {
	if err := ValidateUsageEvent(usage); err != nil {
		return err
	}
	if usage.Source == "" {
		usage.Source = "unknown"
	}
	if usage.Metadata == nil {
		usage.Metadata = map[string]interface{}{}
	}

	isNew, err := s.repo.RecordUsage(ctx, usage)
	if err != nil {
		return err
	}

	if isNew && s.publisher != nil {
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

func (s *Service) CanUseFeature(ctx context.Context, orgID, feature string) (bool, Account, error) {
	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return false, Account{}, err
	}

	if feature == "" {
		return false, account, nil
	}
	if account.SubscriptionState == SubscriptionStateCanceled {
		return false, account, nil
	}
	// A past_due subscription keeps access during a grace window, then is
	// suspended: after PastDueGracePeriod (60 days) elapsed since it entered
	// past_due, all feature access is denied.
	if pastDueGraceExpired(account, time.Now().UTC()) {
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

	// D-A billing-group inheritance, checked LAST so it can only ever widen
	// access, never withdraw it. Entitlements are a stored map refreshed on plan
	// change, so inheriting a tier does not populate them; the inherited tier's
	// default entitlements are resolved here at read time instead. Keeping this
	// derived rather than writing the host's entitlements onto the member means a
	// revoked grant needs no undo -- it simply stops resolving.
	if inherited := s.ResolveEffectivePlan(ctx, account, time.Now().UTC()); inherited != EffectivePlan(account, time.Now().UTC()) {
		if defaultEntitlementsForPlan(inherited)[feature] {
			return true, account, nil
		}
	}

	return false, account, nil
}

func (s *Service) GetQuotaStatus(ctx context.Context, orgID, metric string) (QuotaStatus, error) {
	account, err := s.GetAccount(ctx, orgID)
	if err != nil {
		return QuotaStatus{}, err
	}
	allowances, err := s.Allowances(ctx, orgID)
	if err != nil {
		return QuotaStatus{}, err
	}
	for _, report := range allowances {
		if report.Policy.Metric != metric {
			continue
		}
		// A scheduled policy must not take away the currently active plan's
		// quota before its start. Ended or paused policies cannot authorize use
		// of a historical remaining balance.
		if report.AsOf.Before(report.Policy.StartsAt) {
			continue
		}
		if !report.Active {
			return QuotaStatus{OrgID: orgID, Metric: metric, Used: float64(report.Current.Used), IsExceeded: true, Utilization: 1}, nil
		}
		period := report.Current
		limit := float64(period.Used - period.Overage + period.Remaining)
		utilization := 0.0
		if limit > 0 {
			utilization = math.Min(float64(period.Used)/limit, 1)
		}
		return QuotaStatus{OrgID: orgID, Metric: metric, Limit: limit, Used: float64(period.Used), Remaining: float64(period.Remaining), IsExceeded: period.Overage > 0, Utilization: utilization}, nil
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

func usageRetryPayload(usage UsageEvent) map[string]interface{} {
	return map[string]interface{}{
		"event_id":    usage.EventID,
		"org_id":      usage.OrgID,
		"metric":      usage.Metric,
		"quantity":    usage.Quantity,
		"source":      usage.Source,
		"occurred_at": usage.OccurredAt.UTC().Format(time.RFC3339),
		"metadata":    usage.Metadata,
	}
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

// --- Quarry limits snapshot (W2-CP-L1, the Control half of U43 / WS6.D) ---
//
// One read-only answer per organization for the Quarry edge's `billing_core`
// limits source: the effective plan, the three metered Quarry allowances, and
// the plan capacity caps (`quarry.cap.*`). Three rules the edge relies on:
//
//   - Unconfigured is not zero. A metric with no allowance policy and no
//     account quota key is `configured: false` with `remaining: null`; only a
//     configured allowance that ran out is `remaining: 0`.
//   - A cap the plan says nothing about is `null` (the edge keeps its floor).
//     Placeholder plan defaults are served only when
//     BILLING_QUARRY_PLAN_CAP_DEFAULTS=1 (default off); an explicit per-org
//     `quarry.cap.*` key in the account's quota limits always applies.
//   - The read never writes an account: an organization without a billing
//     account is reported as `account: "absent"` instead of being created.

const (
	QuarryMetricCrawlPage          = "quarry.crawl.page"
	QuarryMetricBatchURL           = "quarry.batch.url"
	QuarryMetricBrowserSecondAgent = "quarry.browser.second.agent"

	QuarryCapAgentRuns       = "quarry.cap.agent_runs"
	QuarryCapInteractiveRuns = "quarry.cap.interactive_runs"
	QuarryCapRenderPermits   = "quarry.cap.render_permits"
	QuarryCapCrawlJobs       = "quarry.cap.crawl_jobs"
	QuarryCapJobBacklog      = "quarry.cap.job_backlog"
	QuarryCapRateMultiplier  = "quarry.cap.rate_multiplier"

	// QuarryPlanCapDefaultsEnv turns the placeholder plan defaults on. Off
	// (unset or anything but 1/true/on) serves only explicit per-org keys.
	QuarryPlanCapDefaultsEnv = "BILLING_QUARRY_PLAN_CAP_DEFAULTS"

	QuarryAccountPresent = "present"
	QuarryAccountAbsent  = "absent"

	QuarryAllowanceSourcePolicy    = "allowance_policy"
	QuarryAllowanceSourcePlanQuota = "plan_quota"
	QuarryAllowanceSourceNone      = "none"
)

// QuarryLimitMetrics are the metered allowances the snapshot always reports.
var QuarryLimitMetrics = []string{QuarryMetricCrawlPage, QuarryMetricBatchURL, QuarryMetricBrowserSecondAgent}

// quarryPlanCapDefaultsByPlan holds PLACEHOLDER numbers from the build plan
// (WS6.D file table); Control Plane owns the real values. Order per plan:
// agent_runs, interactive_runs, render_permits, crawl_jobs, job_backlog,
// rate_multiplier. The stored plan "trial" takes free's numbers (the plan's
// "trial = free"); an active trial resolves to TrialPlan and takes that plan's.
var quarryPlanCapDefaultsByPlan = map[string][6]int64{
	"free":       {1, 2, 1, 1, 10, 1},
	"trial":      {1, 2, 1, 1, 10, 1},
	"hobby":      {2, 4, 1, 2, 50, 2},
	"standard":   {4, 6, 2, 3, 200, 4},
	"pro":        {8, 8, 2, 5, 1000, 10},
	"enterprise": {16, 16, 4, 10, 5000, 50},
}

// QuarryAllowance is one metered Quarry allowance as the edge sees it.
type QuarryAllowance struct {
	Metric     string `json:"metric"`
	Configured bool   `json:"configured"`
	// Source is allowance_policy, plan_quota or none.
	Source string `json:"source"`
	// Active is false for an ended or paused policy (it then refuses use).
	Active    bool       `json:"active"`
	Unlimited bool       `json:"unlimited"`
	Limit     *int64     `json:"limit"`
	Used      int64      `json:"used"`
	Remaining *int64     `json:"remaining"`
	Exhausted bool       `json:"exhausted"`
	PeriodEnd *time.Time `json:"period_end,omitempty"`
}

// QuarryPlanCaps are the plan capacity caps. nil means "the plan says nothing".
type QuarryPlanCaps struct {
	AgentRuns       *int64 `json:"agent_runs"`
	InteractiveRuns *int64 `json:"interactive_runs"`
	RenderPermits   *int64 `json:"render_permits"`
	CrawlJobs       *int64 `json:"crawl_jobs"`
	JobBacklog      *int64 `json:"job_backlog"`
	RateMultiplier  *int64 `json:"rate_multiplier"`
}

// QuarryLimits is the org-bound snapshot served to the quarry-edge principal.
type QuarryLimits struct {
	OrgID             string                     `json:"org_id"`
	Account           string                     `json:"account"`
	Plan              string                     `json:"plan"`
	SubscriptionState SubscriptionState          `json:"subscription_state,omitempty"`
	Allowances        map[string]QuarryAllowance `json:"allowances"`
	Caps              QuarryPlanCaps             `json:"caps"`
	PlanCapDefaults   bool                       `json:"plan_cap_defaults"`
	AsOf              time.Time                  `json:"as_of"`
}

func quarryPlanCapDefaultsEnabled() bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv(QuarryPlanCapDefaultsEnv))) {
	case "1", "true", "on":
		return true
	default:
		return false
	}
}

// QuarryLimits reads the snapshot for one organization. It never creates a
// billing account. A tombstoned organization returns ErrOrganizationDeleted;
// an invalid organization id returns ErrAllowanceInvalid.
func (s *Service) QuarryLimits(ctx context.Context, orgID string, now time.Time) (QuarryLimits, error) {
	if err := validateUsageDimension("org_id", orgID, 128, false); err != nil {
		return QuarryLimits{}, fmt.Errorf("%w: invalid organization", ErrAllowanceInvalid)
	}
	now = now.UTC()
	snapshot := QuarryLimits{
		OrgID:           orgID,
		Account:         QuarryAccountPresent,
		Allowances:      make(map[string]QuarryAllowance, len(QuarryLimitMetrics)),
		PlanCapDefaults: quarryPlanCapDefaultsEnabled(),
		AsOf:            now,
	}
	account, err := s.repo.GetAccount(ctx, orgID)
	switch {
	case errors.Is(err, ErrNotFound):
		// Read-only: describe the default account without persisting it.
		account = NewDefaultAccount(orgID)
		snapshot.Account = QuarryAccountAbsent
		snapshot.Plan = EffectivePlan(account, now)
	case err != nil:
		return QuarryLimits{}, err
	default:
		snapshot.Plan = s.ResolveEffectivePlan(ctx, account, now)
	}
	snapshot.SubscriptionState = account.SubscriptionState

	// Allowances also checks the organization tombstone (ErrOrganizationDeleted).
	reports, err := s.repo.Allowances(ctx, orgID, now)
	if err != nil {
		return QuarryLimits{}, err
	}
	for _, metric := range QuarryLimitMetrics {
		if allowance, ok := QuarryAllowanceFromReports(reports, metric); ok {
			snapshot.Allowances[metric] = allowance
			continue
		}
		limit, ok := account.QuotaLimits[metric]
		if !ok {
			snapshot.Allowances[metric] = QuarryAllowanceUnconfigured(metric)
			continue
		}
		used := 0.0
		if snapshot.Account == QuarryAccountPresent {
			if used, err = s.repo.GetMetricUsage(ctx, orgID, metric); err != nil {
				return QuarryLimits{}, err
			}
		}
		snapshot.Allowances[metric] = QuarryAllowanceFromPlanQuota(metric, limit, used)
	}
	snapshot.Caps = QuarryPlanCapsFor(account.QuotaLimits, snapshot.Plan, snapshot.PlanCapDefaults)
	return snapshot, nil
}

// QuarryAllowanceUnconfigured is "no allowance for this metric": never zero.
func QuarryAllowanceUnconfigured(metric string) QuarryAllowance {
	return QuarryAllowance{Metric: metric, Configured: false, Source: QuarryAllowanceSourceNone}
}

// QuarryAllowanceFromReports applies GetQuotaStatus's policy rules to one
// metric: a policy that has not started yet is skipped (it must not take away
// the current answer before its start), an ended or paused one refuses use.
func QuarryAllowanceFromReports(reports []AllowanceReport, metric string) (QuarryAllowance, bool) {
	for _, report := range reports {
		if report.Policy.Metric != metric || report.AsOf.Before(report.Policy.StartsAt) {
			continue
		}
		period := report.Current
		allowance := QuarryAllowance{
			Metric:     metric,
			Configured: true,
			Source:     QuarryAllowanceSourcePolicy,
			Active:     report.Active,
			Used:       period.Used,
		}
		if !report.Active {
			zero := int64(0)
			allowance.Remaining = &zero
			allowance.Exhausted = true
			return allowance, true
		}
		limit := period.Used - period.Overage + period.Remaining
		remaining := period.Remaining
		if remaining < 0 {
			remaining = 0
		}
		allowance.Limit = &limit
		allowance.Remaining = &remaining
		allowance.Exhausted = remaining == 0
		if !period.To.IsZero() {
			end := period.To.UTC()
			allowance.PeriodEnd = &end
		}
		return allowance, true
	}
	return QuarryAllowance{}, false
}

// QuarryAllowanceFromPlanQuota reads an account quota key (all-time usage, as
// GetQuotaStatus does). A negative limit is the plan's "unlimited".
func QuarryAllowanceFromPlanQuota(metric string, limit, used float64) QuarryAllowance {
	allowance := QuarryAllowance{
		Metric:     metric,
		Configured: true,
		Source:     QuarryAllowanceSourcePlanQuota,
		Active:     true,
		Used:       int64(math.Ceil(math.Max(used, 0))),
	}
	if limit < 0 {
		allowance.Unlimited = true
		return allowance
	}
	bounded := int64(math.Floor(limit))
	remaining := bounded - allowance.Used
	if remaining < 0 {
		remaining = 0
	}
	allowance.Limit = &bounded
	allowance.Remaining = &remaining
	allowance.Exhausted = remaining == 0
	return allowance
}

// QuarryPlanCapsFor resolves each cap: an explicit non-negative per-org key in
// quotaLimits wins; otherwise the placeholder plan default when enabled;
// otherwise nil. A negative explicit key is treated as "says nothing" (nil):
// the edge has no "unlimited" capacity, only its process-wide cap.
func QuarryPlanCapsFor(quotaLimits map[string]float64, plan string, defaultsEnabled bool) QuarryPlanCaps {
	defaults, hasDefaults := quarryPlanCapDefaultsByPlan[normalizePlan(plan)]
	resolve := func(index int, key string) *int64 {
		if value, ok := quotaLimits[key]; ok {
			if value < 0 || math.IsNaN(value) || math.IsInf(value, 0) {
				return nil
			}
			bounded := int64(math.Floor(value))
			return &bounded
		}
		if defaultsEnabled && hasDefaults {
			value := defaults[index]
			return &value
		}
		return nil
	}
	return QuarryPlanCaps{
		AgentRuns:       resolve(0, QuarryCapAgentRuns),
		InteractiveRuns: resolve(1, QuarryCapInteractiveRuns),
		RenderPermits:   resolve(2, QuarryCapRenderPermits),
		CrawlJobs:       resolve(3, QuarryCapCrawlJobs),
		JobBacklog:      resolve(4, QuarryCapJobBacklog),
		RateMultiplier:  resolve(5, QuarryCapRateMultiplier),
	}
}
