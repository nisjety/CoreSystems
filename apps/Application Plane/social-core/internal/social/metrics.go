package social

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strconv"
	"strings"
	"time"
)

// metaInsightsDatePreset pins the Marketing API insights window that each
// snapshot covers. It is recorded in every metric row's dimensions so rows
// stay self-describing and the dedup key stays stable across re-runs.
const metaInsightsDatePreset = "yesterday"

// SnapshotProviderMetrics collects provider metrics for every connected
// social account and persists them as idempotent daily snapshot rows.
// Passing an empty orgID snapshots every org that has synced accounts.
//
// Provider coverage (verified against integration-corev2
// internal/actions/service.go — the actions surface is the only allowed path
// to provider APIs from here):
//   - meta family: ads.adaccounts -> ads.insights (impressions/reach/clicks/
//     spend per campaign row). Organic Instagram media insights
//     (instagram.insights) are media-level and need per-media ids — out of
//     scope for this worker; follow-up.
//   - linkedin: ads.accounts -> ads.campaigns inventory only. The actions
//     surface exposes no LinkedIn reporting/adAnalytics operation, so
//     campaign presence is snapshotted as metric_name="campaign.status".
//   - snapchat / tiktok / x / google: no actions exist — logged and skipped.
//
// Per-account failures are recorded on the summary and never abort the run.
func (s *Service) SnapshotProviderMetrics(ctx context.Context, orgID string) (*MetricsSnapshotSummary, error) {
	if s.actionExecutor == nil {
		return nil, fmt.Errorf("action executor is not configured")
	}
	if s.metricsStore == nil {
		return nil, fmt.Errorf("metrics store is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	orgIDs := []string{orgID}
	if orgID == "" {
		ids, err := s.metricsStore.ListAccountOrgIDs(ctx)
		if err != nil {
			return nil, fmt.Errorf("list metric orgs: %w", err)
		}
		orgIDs = ids
	}
	snapshotDate := s.now().UTC().Truncate(24 * time.Hour)
	summary := &MetricsSnapshotSummary{Failures: []string{}}
	for _, org := range orgIDs {
		org = strings.TrimSpace(org)
		if org == "" {
			continue
		}
		summary.Orgs++
		accounts, err := s.ListAccounts(ctx, org)
		if err != nil {
			summary.Failures = append(summary.Failures, fmt.Sprintf("org=%s: list accounts: %v", org, err))
			log.Printf("social-core: metrics snapshot: org=%s list accounts failed: %v", org, err)
			continue
		}
		for _, account := range accounts {
			s.snapshotAccountMetrics(ctx, account, snapshotDate, summary)
		}
	}
	return summary, nil
}

func (s *Service) snapshotAccountMetrics(ctx context.Context, account Account, snapshotDate time.Time, summary *MetricsSnapshotSummary) {
	providerKey := normalizePlatform(account.ProviderKey)
	if account.Status != AccountStatusConnected {
		summary.Skipped++
		return
	}
	if strings.TrimSpace(account.ConnectionID) == "" {
		summary.Skipped++
		log.Printf("social-core: metrics snapshot: org=%s account=%s provider=%s has no connection id; skipping",
			account.OrgID, account.ID, providerKey)
		return
	}

	var (
		metrics []ProviderMetric
		err     error
	)
	switch {
	case isMetaFamilyProvider(providerKey):
		if !hasAnyCapability(account, "social.ads.manage", "social.analytics.read") {
			summary.Skipped++
			log.Printf("social-core: metrics snapshot: org=%s account=%s provider=%s lacks ads/analytics capability; skipping",
				account.OrgID, account.ID, providerKey)
			return
		}
		metrics, err = s.collectMetaAdsMetrics(ctx, account, snapshotDate)
	case providerKey == "linkedin":
		if !hasAnyCapability(account, "social.ads.read", "social.ads.manage") {
			summary.Skipped++
			log.Printf("social-core: metrics snapshot: org=%s account=%s provider=linkedin lacks ads capability; skipping",
				account.OrgID, account.ID)
			return
		}
		metrics, err = s.collectLinkedInCampaignMetrics(ctx, account, snapshotDate)
	default:
		// snapchat, tiktok, x (and anything else) have no operations in the
		// integration-corev2 actions surface today — skip, never error.
		summary.Skipped++
		log.Printf("social-core: metrics snapshot: provider %s has no metrics operations in the actions surface (org=%s account=%s); skipping",
			providerKey, account.OrgID, account.ID)
		return
	}
	if err != nil {
		summary.Failures = append(summary.Failures,
			fmt.Sprintf("org=%s account=%s provider=%s: %v", account.OrgID, account.ID, providerKey, err))
		log.Printf("social-core: metrics snapshot failed for org=%s account=%s provider=%s: %v",
			account.OrgID, account.ID, providerKey, err)
		return
	}

	persisted, err := s.metricsStore.UpsertProviderMetrics(ctx, metrics)
	if err != nil {
		summary.Failures = append(summary.Failures,
			fmt.Sprintf("org=%s account=%s provider=%s: persist metrics: %v", account.OrgID, account.ID, providerKey, err))
		log.Printf("social-core: metrics snapshot persist failed for org=%s account=%s provider=%s: %v",
			account.OrgID, account.ID, providerKey, err)
		return
	}
	summary.Accounts++
	summary.Metrics += persisted
	s.publish(ctx, SubjectMetricsSnapshotted, account.OrgID, "", nil, nil, nil, map[string]any{
		"orgId":        account.OrgID,
		"accountId":    account.ID,
		"providerKey":  providerKey,
		"snapshotDate": snapshotDate.Format("2006-01-02"),
		"metricCount":  persisted,
	})
}

// collectMetaAdsMetrics resolves the connection's ad accounts via
// "ads.adaccounts", then reads "ads.insights" per ad account and persists
// impressions/reach/clicks/spend per campaign row from the Graph `data`
// array. Graph returns metric values as strings; parsing is defensive.
func (s *Service) collectMetaAdsMetrics(ctx context.Context, account Account, snapshotDate time.Time) ([]ProviderMetric, error) {
	adAccountsResult, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "ads.adaccounts",
		Params:       map[string]any{"limit": 100},
	})
	if err != nil {
		return nil, fmt.Errorf("ads.adaccounts: %w", err)
	}
	providerKey := normalizePlatform(account.ProviderKey)
	metrics := []ProviderMetric{}
	for _, adAccount := range decodeActionRows(adAccountsResult) {
		adAccountID := stringField(adAccount, "id", "account_id")
		if adAccountID == "" {
			continue
		}
		insightsResult, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
			ConnectionID: account.ConnectionID,
			Operation:    "ads.insights",
			Params: map[string]any{
				"adAccountId": adAccountID,
				"datePreset":  metaInsightsDatePreset,
				"limit":       100,
			},
		})
		if err != nil {
			return nil, fmt.Errorf("ads.insights (%s): %w", adAccountID, err)
		}
		for _, row := range decodeActionRows(insightsResult) {
			campaignID := stringField(row, "campaign_id")
			campaignName := stringField(row, "campaign_name")
			level := "campaign"
			if campaignID == "" {
				level = "account"
			}
			for _, metricName := range []string{"impressions", "reach", "clicks", "spend"} {
				value, ok := numericField(row, metricName)
				if !ok {
					continue
				}
				metrics = append(metrics, ProviderMetric{
					OrgID:        account.OrgID,
					AccountID:    account.ID,
					ConnectionID: account.ConnectionID,
					ProviderKey:  providerKey,
					MetricName:   "ads." + metricName,
					MetricValue:  value,
					Dimensions: map[string]any{
						"ad_account_id": adAccountID,
						"campaign_id":   campaignID,
						"campaign_name": campaignName,
						"level":         level,
						"date_preset":   metaInsightsDatePreset,
					},
					SnapshotDate: snapshotDate,
				})
			}
		}
	}
	return metrics, nil
}

// collectLinkedInCampaignMetrics snapshots campaign inventory only: the
// actions surface (executeLinkedIn) exposes ads.accounts / ads.campaigns but
// no reporting or adAnalytics operation. Until one exists in
// integration-corev2, campaign presence is persisted as
// metric_name="campaign.status" with value 1 and {status, name} dimensions.
func (s *Service) collectLinkedInCampaignMetrics(ctx context.Context, account Account, snapshotDate time.Time) ([]ProviderMetric, error) {
	adAccountsResult, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "ads.accounts",
		Params:       map[string]any{"pageSize": 100},
	})
	if err != nil {
		return nil, fmt.Errorf("ads.accounts: %w", err)
	}
	metrics := []ProviderMetric{}
	for _, adAccount := range decodeActionRows(adAccountsResult) {
		adAccountID := stringField(adAccount, "id")
		if adAccountID == "" {
			continue
		}
		campaignsResult, err := s.actionExecutor.ExecuteAction(ctx, ActionRequest{
			ConnectionID: account.ConnectionID,
			Operation:    "ads.campaigns",
			Params: map[string]any{
				"accountId": adAccountID,
				"pageSize":  100,
			},
		})
		if err != nil {
			return nil, fmt.Errorf("ads.campaigns (%s): %w", adAccountID, err)
		}
		for _, campaign := range decodeActionRows(campaignsResult) {
			campaignID := stringField(campaign, "id")
			if campaignID == "" {
				continue
			}
			metrics = append(metrics, ProviderMetric{
				OrgID:        account.OrgID,
				AccountID:    account.ID,
				ConnectionID: account.ConnectionID,
				ProviderKey:  "linkedin",
				MetricName:   "campaign.status",
				MetricValue:  1,
				Dimensions: map[string]any{
					"ad_account_id": adAccountID,
					"campaign_id":   campaignID,
					"name":          stringField(campaign, "name"),
					"status":        stringField(campaign, "status"),
				},
				SnapshotDate: snapshotDate,
			})
		}
	}
	return metrics, nil
}

// ListProviderMetrics reads persisted metric rows for an org — the read side
// of SnapshotProviderMetrics. insight-core calls this (via the internal HTTP
// API) after receiving a metrics.snapshotted event to fetch the real values,
// since that event intentionally carries only a summary count.
func (s *Service) ListProviderMetrics(ctx context.Context, filter ProviderMetricsFilter) ([]ProviderMetric, error) {
	if s.metricsStore == nil {
		return nil, fmt.Errorf("metrics store is not configured")
	}
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("org_id is required")
	}
	return s.metricsStore.ListProviderMetrics(ctx, filter)
}

func isMetaFamilyProvider(providerKey string) bool {
	switch providerKey {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return true
	default:
		return false
	}
}

func hasAnyCapability(account Account, keys ...string) bool {
	for _, capability := range account.Capabilities {
		capability = strings.ToLower(strings.TrimSpace(capability))
		for _, key := range keys {
			if capability == key {
				return true
			}
		}
	}
	return false
}

// decodeActionRows extracts list rows from a raw provider result without
// assuming a provider schema: Graph API lists live under `data`, LinkedIn
// Rest.li lists under `elements`. Anything else decodes to no rows.
func decodeActionRows(result *ActionResult) []map[string]any {
	if result == nil || len(result.Result) == 0 {
		return nil
	}
	var payload struct {
		Data     []map[string]any `json:"data"`
		Elements []map[string]any `json:"elements"`
	}
	if err := json.Unmarshal(result.Result, &payload); err != nil {
		return nil
	}
	if len(payload.Data) > 0 {
		return payload.Data
	}
	return payload.Elements
}

func stringField(row map[string]any, keys ...string) string {
	for _, key := range keys {
		switch value := row[key].(type) {
		case string:
			if trimmed := strings.TrimSpace(value); trimmed != "" {
				return trimmed
			}
		case float64:
			return strconv.FormatFloat(value, 'f', -1, 64)
		case json.Number:
			return value.String()
		}
	}
	return ""
}

func numericField(row map[string]any, key string) (float64, bool) {
	switch value := row[key].(type) {
	case float64:
		return value, true
	case string:
		parsed, err := strconv.ParseFloat(strings.TrimSpace(value), 64)
		if err != nil {
			return 0, false
		}
		return parsed, true
	case json.Number:
		parsed, err := value.Float64()
		if err != nil {
			return 0, false
		}
		return parsed, true
	default:
		return 0, false
	}
}
