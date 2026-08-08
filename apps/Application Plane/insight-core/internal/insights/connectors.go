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
			Type:               "google_analytics_4",
			DisplayName:        "Google Analytics 4",
			Surface:            SurfaceExternalAnalytics,
			Status:             ConnectorStatusRequiresTokenLease,
			Authorization:      "integration_core_token_lease",
			TokenLeaseAudience: tokenAudience,
			RequiredEnv:        []string{"INTEGRATION_CORE_URL", "INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE"},
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
		},
		{
			Type:               "google_search_console",
			DisplayName:        "Google Search Console",
			Surface:            SurfaceExternalAnalytics,
			Status:             ConnectorStatusRequiresTokenLease,
			Authorization:      "integration_core_token_lease",
			TokenLeaseAudience: tokenAudience,
			RequiredEnv:        []string{"INTEGRATION_CORE_URL", "INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE"},
			ReferenceURLs: []string{
				"https://developers.google.com/webmaster-tools/v1/searchanalytics/query",
			},
			Contracts: []ConnectorContract{{
				Name:             "searchanalytics.query",
				EndpointTemplate: searchConsoleBaseURL + "/sites/{siteUrl}/searchAnalytics/query",
				Method:           "POST",
				RequestShape:     "startDate, endDate, dimensions[], dimensionFilterGroups[], rowLimit, startRow",
				ResponseShape:    "SearchAnalyticsQueryResponse rows grouped by requested dimensions",
				RequiredScopes: []string{
					"https://www.googleapis.com/auth/webmasters.readonly",
					"https://www.googleapis.com/auth/webmasters",
				},
				DimensionExamples: []string{"query", "page", "country", "device", "date"},
				MetricExamples:    []string{"clicks", "impressions", "ctr", "position"},
			}},
		},
		{
			Type:               "seo_tool",
			DisplayName:        "SEO Tool",
			Surface:            SurfaceExternalAnalytics,
			Status:             ConnectorStatusDisabled,
			Authorization:      "integration_core_token_lease",
			TokenLeaseAudience: tokenAudience,
			RequiredEnv:        []string{"INTEGRATION_CORE_URL", "INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE"},
			Contracts: []ConnectorContract{{
				Name:             "seo_metric_event",
				EndpointTemplate: "/internal/insight-events",
				Method:           "POST",
				RequestShape:     "IngestMetricEventInput with surface=external_analytics",
				ResponseShape:    "MetricEvent",
			}},
		},
	}
}
