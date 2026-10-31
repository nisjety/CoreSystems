package insights

type ConnectorSlotOptions struct {
	GoogleAnalyticsAPIBaseURL     string
	GoogleSearchConsoleAPIBaseURL string
	TokenLeaseAudience            string
}

func DefaultConnectorSlots(opts ConnectorSlotOptions) []ConnectorSlot {
	tokenAudience := fallback(opts.TokenLeaseAudience, "insight-core")
	gaBaseURL := fallback(opts.GoogleAnalyticsAPIBaseURL, "https://analyticsdata.googleapis.com")
	searchConsoleBaseURL := fallback(opts.GoogleSearchConsoleAPIBaseURL, "https://www.googleapis.com/webmasters/v3")
	marketingGrounding := marketingEvidenceGrounding()

	return []ConnectorSlot{
		{
			Type:          "social-core",
			DisplayName:   "Verevon Social",
			Surface:       SurfaceSocial,
			Status:        ConnectorStatusNative,
			Authorization: "internal_application_plane_event",
			Contracts: []ConnectorContract{{
				Name:             "social_metric_event",
				EndpointTemplate: "/internal/insight-events",
				Method:           "POST",
				RequestShape:     "IngestMetricEventInput with surface=social",
				ResponseShape:    "MetricEvent",
			}},
		},
		{
			Type:          "conversation-core",
			DisplayName:   "Verevon Inbox",
			Surface:       SurfaceInbox,
			Status:        ConnectorStatusNative,
			Authorization: "internal_application_plane_event",
			Contracts: []ConnectorContract{{
				Name:             "inbox_metric_event",
				EndpointTemplate: "/internal/insight-events",
				Method:           "POST",
				RequestShape:     "IngestMetricEventInput with surface=inbox",
				ResponseShape:    "MetricEvent",
			}},
		},
		{
			Type:          "model-plane-agents",
			DisplayName:   "Verevon Agents",
			Surface:       SurfaceAgents,
			Status:        ConnectorStatusNative,
			Authorization: "model_plane_jetstream_event",
			Contracts: []ConnectorContract{{
				Name:             "agent_metric_event",
				EndpointTemplate: "/internal/insight-events",
				Method:           "POST",
				RequestShape:     "IngestMetricEventInput with surface=agents",
				ResponseShape:    "MetricEvent",
			}},
		},
		{
			Type:          "model-gateway-chat",
			DisplayName:   "Verevon Chat",
			Surface:       SurfaceChat,
			Status:        ConnectorStatusNative,
			Authorization: "model_plane_jetstream_event",
			Contracts: []ConnectorContract{{
				Name:             "global_chat_turn_started",
				EndpointTemplate: "mp.v1.run.*.event",
				Method:           "NATS",
				RequestShape:     "Model Plane Envelope RUN_STARTED from model-gateway with request resource_ref",
				ResponseShape:    "MetricEvent with surface=chat",
			}},
		},
		{
			Type:          "ingestion-plane",
			DisplayName:   "Verevon Ingestion",
			Surface:       SurfaceIngestion,
			Status:        ConnectorStatusNative,
			Authorization: "application_plane_jetstream_event",
			Contracts: []ConnectorContract{{
				Name:             "ingestion_lifecycle",
				EndpointTemplate: "verevon.ingestion.>",
				Method:           "NATS",
				RequestShape:     "Content-free import/crawl lifecycle event",
				ResponseShape:    "MetricEvent with surface=ingestion",
			}},
		},
		{
			Type:          "data-plane-knowledge",
			DisplayName:   "Verevon Knowledge",
			Surface:       SurfaceKnowledge,
			Status:        ConnectorStatusNative,
			Authorization: "data_plane_outbox_event",
			Contracts: []ConnectorContract{{
				Name:             "document_lifecycle_observation",
				EndpointTemplate: "verevon.ingestion.knowledge.document.*",
				Method:           "NATS",
				RequestShape:     "KnowledgeObservation (content-free, org-visible, non-ZDR)",
				ResponseShape:    "MetricEvent with surface=knowledge",
				RequiredScopes:   []string{"active organization", "verified actor when present"},
				MetricExamples:   []string{"documents_created", "documents_updated", "documents_deleted"},
			}},
		},
		{
			Type:          "campaign-core",
			DisplayName:   "Verevon Campaigns",
			Surface:       SurfaceCampaigns,
			Status:        ConnectorStatusPlanned,
			Authorization: "internal_application_plane_event",
			Contracts: []ConnectorContract{{
				Name:             "campaign_metric_event",
				EndpointTemplate: "/internal/insight-events",
				Method:           "POST",
				RequestShape:     "IngestMetricEventInput with surface=campaigns",
				ResponseShape:    "MetricEvent",
			}},
		},
		{
			Type:                 "google_analytics_4",
			DisplayName:          "Google Analytics 4",
			Surface:              SurfaceExternalAnalytics,
			Status:               ConnectorStatusRequiresGovernedAction,
			Authorization:        "integration_core_governed_action",
			TokenLeaseAudience:   tokenAudience,
			RequiredCapabilities: []string{"analytics.read"},
			RequiredEnv:          []string{"INTEGRATION_CORE_URL"},
			ReferenceURLs: []string{
				"https://developers.google.com/analytics/devguides/reporting/data/v1/rest/v1beta/properties/runReport",
				"https://developers.google.com/analytics/devguides/reporting/data/v1/api-schema",
			},
			Contracts: []ConnectorContract{{
				Name:             "properties.runReport",
				EndpointTemplate: gaBaseURL + "/v1beta/{property=properties/*}:runReport",
				Method:           "POST",
				RequestShape:     "dimensions[], metrics[], dateRanges[], dimensionFilter, metricFilter, orderBys, offset, limit",
				ResponseShape:    "RunReportResponse rows with requested dimension and metric values",
				RequiredScopes: []string{
					"https://www.googleapis.com/auth/analytics.readonly",
					"https://www.googleapis.com/auth/analytics",
				},
				DimensionExamples: []string{"date", "sessionDefaultChannelGroup", "campaignName", "city"},
				MetricExamples:    []string{"activeUsers", "eventCount", "sessions", "conversions"},
			}},
			ModelGrounding: marketingGrounding,
		},
		{
			Type:                 "google_search_console",
			DisplayName:          "Google Search Console",
			Surface:              SurfaceExternalAnalytics,
			Status:               ConnectorStatusRequiresGovernedAction,
			Authorization:        "integration_core_governed_action",
			TokenLeaseAudience:   tokenAudience,
			RequiredCapabilities: []string{"search_console.read"},
			RequiredEnv:          []string{"INTEGRATION_CORE_URL"},
			ReferenceURLs: []string{
				"https://developers.google.com/webmaster-tools/v1/searchanalytics/query",
			},
			Contracts: []ConnectorContract{{
				Name:             "searchanalytics.query",
				EndpointTemplate: searchConsoleBaseURL + "/sites/{siteUrl}/searchAnalytics/query",
				Method:           "POST",
				RequestShape:     "siteUrl param; startDate, endDate, dimensions[], rowLimit, startRow (allowlisted, bounded)",
				ResponseShape:    "SearchAnalyticsQueryResponse rows grouped by requested dimensions",
				RequiredScopes: []string{
					"https://www.googleapis.com/auth/webmasters.readonly",
				},
				DimensionExamples: []string{"query", "page", "country", "device", "date"},
				MetricExamples:    []string{"clicks", "impressions", "ctr", "position"},
			}},
			ModelGrounding:           marketingGrounding,
			IntegrationSteps:         googleSearchConsoleIntegrationSteps(),
			AdvisoryInstructionPacks: searchConsoleAdvisoryInstructionPacks(),
		},
		{
			Type:                 "google_ads",
			DisplayName:          "Google Ads",
			Surface:              SurfaceExternalAnalytics,
			Status:               ConnectorStatusRequiresGovernedAction,
			Authorization:        "integration_core_governed_action",
			TokenLeaseAudience:   tokenAudience,
			RequiredCapabilities: []string{"ads.manage"},
			RequiredEnv:          []string{"INTEGRATION_CORE_URL"},
			ReferenceURLs: []string{
				"https://developers.google.com/google-ads/api/docs/start",
			},
			Contracts: []ConnectorContract{{
				Name:              "google_ads.campaign_performance",
				EndpointTemplate:  "integration-core governed action (planned fixed campaign performance report)",
				Method:            "POST",
				RequestShape:      "customer binding, bounded date range, fixed reporting dimensions",
				ResponseShape:     "Normalized campaign performance observations with provenance",
				RequiredScopes:    []string{"https://www.googleapis.com/auth/adwords"},
				DimensionExamples: []string{"campaign", "date", "device"},
				MetricExamples:    []string{"cost_micros", "clicks", "impressions", "conversions"},
			}},
			ModelGrounding: marketingGrounding,
		},
		{
			Type:                 "meta_ads",
			DisplayName:          "Meta Ads",
			Surface:              SurfaceExternalAnalytics,
			Status:               ConnectorStatusNative,
			Authorization:        "social_core_metric_event",
			RequiredCapabilities: []string{"social.analytics.read", "social.ads.manage"},
			ReferenceURLs: []string{
				"https://developers.facebook.com/docs/marketing-apis/insights/",
			},
			Contracts: []ConnectorContract{{
				Name:              "meta_ads.metrics.snapshot",
				EndpointTemplate:  "social-core metrics.snapshotted event -> GET /api/v1/social/metrics -> /internal/insight-events",
				Method:            "NATS -> HTTP",
				RequestShape:      "Organization-scoped Meta metric snapshot fetched from social-core by account and snapshot date",
				ResponseShape:     "MetricEvent with source=social-core and connector_type=meta",
				RequiredScopes:    []string{"ads_management", "business_management", "ads_read", "read_insights"},
				DimensionExamples: []string{"account", "campaign", "adset", "date"},
				MetricExamples:    []string{"spend", "impressions", "clicks", "actions"},
			}},
			ModelGrounding:   marketingGrounding,
			IntegrationSteps: metaAdsIntegrationSteps(),
		},
		{
			Type:          "semrush",
			DisplayName:   "SEMrush",
			Surface:       SurfaceExternalAnalytics,
			Status:        ConnectorStatusRequiresCredentialBroker,
			Authorization: "integration_core_api_key_lease",
			ReferenceURLs: []string{
				"https://developer.semrush.com/api/analytics/",
			},
			Contracts: []ConnectorContract{{
				Name:              "semrush.domain_overview",
				EndpointTemplate:  "integration-core managed API-key adapter (planned)",
				Method:            "POST",
				RequestShape:      "domain binding, database, bounded locale and date window",
				ResponseShape:     "Normalized organic-domain and position observations with provenance",
				DimensionExamples: []string{"domain", "database", "date"},
				MetricExamples:    []string{"organic_keywords", "organic_traffic", "authority_score"},
			}},
			ModelGrounding: marketingGrounding,
		},
		{
			Type:          "first_party_seo",
			DisplayName:   "First-party SEO evidence",
			Surface:       SurfaceExternalAnalytics,
			Status:        ConnectorStatusRequiresEvidenceAdapter,
			Authorization: "ingestion_data_plane_evidence",
			Contracts: []ConnectorContract{{
				Name:              "first_party_seo.observation",
				EndpointTemplate:  "Quarry/Data Plane evidence contract (planned)",
				Method:            "NATS/API",
				RequestShape:      "Authorized crawl or sitemap observation with source_trace_ref and retention policy",
				ResponseShape:     "MetricEvent or evidence reference; raw crawl content remains outside insight-core",
				DimensionExamples: []string{"site", "url", "crawl_run", "observed_at"},
				MetricExamples:    []string{"indexability", "sitemap_url_count", "content_freshness"},
			}},
			ModelGrounding: marketingGrounding,
		},
	}
}

func marketingEvidenceGrounding() ModelGroundingContract {
	return ModelGroundingContract{
		Contract:                  "marketing_evidence.v1",
		RequiredFields:            []string{"connector_type", "metric", "time_window", "observed_at", "source_trace_ref", "retention_class"},
		ExcludesZeroDataRetention: true,
	}
}

func googleSearchConsoleIntegrationSteps() []IntegrationStep {
	return []IntegrationStep{
		{
			ID:                   "authorize_read_access",
			Label:                "Authorize Search Console reporting",
			Purpose:              "Connect a Google account with read-only Search Console reporting access.",
			RequiredCapabilities: []string{"search_console.read"},
		},
		{
			ID:                   "test_read_only_access",
			Label:                "Test read-only access",
			Purpose:              "Verify the connection with one fixed read-only property-list request.",
			EndpointTemplate:     "/api/v1/connections/{connectionId}/readiness",
			Method:               "POST",
			FixedOperation:       "gsc.sites.list",
			RequiredCapabilities: []string{"search_console.read"},
		},
		{
			ID:                   "bind_verified_property",
			Label:                "Bind a verified property",
			Purpose:              "Choose an already-authorized Search Console property for bounded reporting requests.",
			RequiredCapabilities: []string{"search_console.read"},
		},
	}
}

func metaAdsIntegrationSteps() []IntegrationStep {
	return []IntegrationStep{
		{
			ID:                   "authorize_ads_access",
			Label:                "Authorize Meta Ads access",
			Purpose:              "Connect an approved Meta Ads account through the governed integration boundary.",
			RequiredCapabilities: []string{"social.ads.manage"},
		},
		{
			ID:                   "test_read_only_access",
			Label:                "Test read-only access",
			Purpose:              "Verify the connection with one bounded read-only ad-account request.",
			EndpointTemplate:     "/api/v1/connections/{connectionId}/readiness",
			Method:               "POST",
			FixedOperation:       "ads.adaccounts",
			RequiredCapabilities: []string{"social.ads.manage"},
		},
		{
			ID:                   "enable_reporting",
			Label:                "Enable ads reporting",
			Purpose:              "Grant analytics access only after an ad account passes the read-only test.",
			RequiredCapabilities: []string{"social.analytics.read"},
		},
	}
}

func searchConsoleAdvisoryInstructionPacks() []AdvisoryInstructionPack {
	const evidenceRule = "Use only traceable marketing evidence and cite its source trace."
	const windowRule = "Compare the same dimensions and time windows before recommending a change."
	const approvalRule = "Produce recommendations only; provider changes require the approved action workflow."

	return []AdvisoryInstructionPack{
		{
			ID:                "search_console_reporting",
			Mode:              "advisory",
			RequiresReadiness: true,
			SkillIDs:          []string{"seo-google", "google-search-console-tool", "google-search-console", "ds-seo-weekly"},
			Instructions:      []string{evidenceRule, windowRule, approvalRule},
		},
		{
			ID:                "search_console_opportunities",
			Mode:              "advisory",
			RequiresReadiness: true,
			SkillIDs:          []string{"seo-gsc-opportunities", "seo-gsc-content-decay", "seo-gsc-drops", "seo-gsc-brand-vs-nonbrand"},
			Instructions:      []string{evidenceRule, windowRule, approvalRule},
		},
		{
			ID:                "technical_seo",
			Mode:              "advisory",
			RequiresReadiness: true,
			SkillIDs:          []string{"seo-site-audit-pro", "seo-sitemap", "seo-audit", "seo-ops"},
			Instructions:      []string{evidenceRule, "Separate observed technical issues from proposed fixes.", approvalRule},
		},
		{
			ID:                "content_and_conversion",
			Mode:              "advisory",
			RequiresReadiness: true,
			SkillIDs:          []string{"blog-google", "conversion-optimization"},
			Instructions:      []string{evidenceRule, "Treat content and conversion ideas as hypotheses to validate.", approvalRule},
		},
	}
}
