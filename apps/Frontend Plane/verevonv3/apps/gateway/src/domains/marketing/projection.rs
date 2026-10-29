//! Rebuild public contracts recursively. Unknown provider/credential fields are
//! discarded at every depth, not merely hidden later by the browser renderer.
use super::contract::{Kind, Spec};
use serde_json::{json, Map, Value};

fn object(keys: &str) -> Value {
    Value::Object(
        keys.split_whitespace()
            .map(|key| (key.to_owned(), Value::Null))
            .collect(),
    )
}
fn with(base: &str, nested: &[(&str, Value)]) -> Value {
    let mut schema = object(base).as_object().unwrap().clone();
    schema.extend(
        nested
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone())),
    );
    Value::Object(schema)
}
fn website() -> Value {
    with(
        "id site revision createdAt updatedAt primary",
        &[
            ("ownership", object("status checkedAt")),
            (
                "integrations",
                json!({"searchConsole":object("status checkedAt property accessLevel"),"ga4":object("status checkedAt propertyId streamId measurementId reportingSite"),"clarity":object("status projectId"),"pageSpeed":object("status")}),
            ),
        ],
    )
}
fn seo() -> Value {
 with("version headingsPartial noindex nofollow robotsStatus wordCount invalidAlternates alternatesPartial canonicalOutsideSite", &[
 ("headings",json!([object("level text")])),
 ("alternates",json!([object("lang url")])),
 ("redirects",json!([object("url status target")])),
 ("structuredData",with("documents invalid partial", &[("types",json!([Value::Null]))]))
 ])
}
fn schema(kind: Kind) -> Value {
    let period = object("startDate endDate");
    let row = object("key clicks impressions ctr position");
    let ga4metrics = "users sessions engagedSessions pageViews keyEvents engagementSeconds";
    match kind {
        Kind::Collection => with("websiteId site websiteRevision source asOf quarter", &[
            ("summary", object("available empty pending stale attention unsupported")),
            ("datasets", json!([with("key provider category status reason coverage partial records unit lastSuccessAt lastAttemptAt nextSyncAt reportingResource history collectionMode", &[("period", object("startDate endDate"))])])),
        ]),
        Kind::Resources=>with("websiteId site websiteRevision status partial checkedAt", &[("choices",json!([object("id provider label")]))]),
        Kind::Assignees=>json!({"members":[object("id name")]}),
        Kind::Research => { let aggregate=object("key clicks impressions position days");let rule=object("id name entity contains minImpressions maxPosition");with("websiteId site websiteRevision source status reason observedAt lastAttemptAt nextRunAt stale truncated partial", &[("period",period),("rows",json!([object("date query page clicks impressions position")])),("queries",json!([aggregate.clone()])),("pages",json!([aggregate.clone()])),("weeks",json!([aggregate.clone()])),("relationships",json!([with("query overlapDays candidate", &[("pages",json!([aggregate.clone()]))])])),("groups",json!([with("total truncated", &[("rule",rule.clone()),("items",json!([aggregate]))])])),("rules",json!([rule]))]) },
        Kind::Analysis => with("websiteId site websiteRevision source generatedAt", &[
            ("period",period),
            ("coverage",object("searchStatus searchAt searchTruncated auditStatus auditAt auditId checked failed discoveryStatus partial linkLimit")),
            ("pages",json!([with("url title description canonical auditStatus httpStatus observedAt clicks impressions ctr position issues inbound outbound depth discovered", &[("seo",seo())])])),
            ("groups",json!([with("key entity", &[("items",json!([object("key url clicks impressions ctr position")]))])])),
            ("findings",json!([object("id ruleId pageUrl severity")])),
 ("edges",json!([object("from to status")]))
        ]),
        Kind::Competitors => {
            let page=with("url finalUrl title description canonical httpStatus internalLinks observedAt runId fingerprint", &[("seo",seo())]);
            with("websiteId site websiteRevision source limit", &[("entries",json!([with("websiteId site websiteRevision target ownUrl targetUrl status requestedAt lastAttemptAt nextRunAt reason", &[("own",page.clone()),("competitor",page.clone()),("history",json!([with("observedAt changed", &[("own",page.clone()),("competitor",page)])]))])]))])
        },
        Kind::AutomaticTrends => with("websiteId site websiteRevision source status reason updatedAt nextSyncAt expiresAt", &[("seed",with("source selection observedAt", &[("terms",json!([Value::Null])),("period",object("startDate endDate"))])), ("report",with("source scale status observedAt cached retryAt geo timeframe", &[("terms", json!([Value::Null])), ("rows", json!([with("date partial", &[("values", json!([Value::Null]))])]))]))]),
        Kind::Trends => with("websiteId site websiteRevision", &[("report", with("source scale status observedAt cached retryAt geo timeframe", &[("terms", json!([Value::Null])), ("rows", json!([with("date partial", &[("values", json!([Value::Null]))])]))]))]),
        Kind::Pulse => with("site source status propertyScope reportingProperty updatedAt reason", &[("period",period),("totals",object("clicks impressions")),("days",json!([object("date clicks impressions")]))]),
        Kind::Search => with("site source status propertyScope reportingProperty updatedAt reason rowLimit queriesAtLimit pagesAtLimit", &[("period",period),("queries",json!([row])),("pages",json!([row])),("opportunities",json!([object("key clicks impressions ctr position type")]))]),
        Kind::Comparison => with("site source status propertyScope reportingProperty updatedAt reason dimension rowLimit rowsAtLimit comparableRows unmatchedRows", &[("currentPeriod",period.clone()),("previousPeriod",period),("rows",json!([with("key state clicksChange impressionsChange positionChange clicksChangePercent", &[("current",row.clone()),("previous",row)])]))]),
        Kind::Ga4 => with("site source status updatedAt reason timeZone reportingSite", &[("period",period),("totals",object(ga4metrics)),("days",json!([object(&format!("date {ga4metrics}"))])),("quality",object("thresholded sampled dataLoss"))]),
        Kind::Clarity => with("site source status reason projectId scope updatedAt", &[("window",object("numOfDays timeZone observedAt")),("metrics",with("sessions botSessions users pagesPerSession", &[("ux",object("deadClickPercent rageClickPercent excessiveScrollPercent quickBackPercent scriptErrorPercent errorClickPercent averageScrollDepth activeSeconds totalSeconds"))]))]),
        Kind::PageSpeed => with("site source strategy status requestedAt completedAt reason", &[("cache",object("stale refreshStatus lastSuccessAt lastAttemptAt nextRefreshAt refreshReason")), ("report",with("performance accessibility bestPractices seo lcpMs cls tbtMs fcpMs speedIndexMs fetchTime lighthouseVersion reportingOrigin", &[("checks",json!([object("id status score")]))]))]),
        // primarySource says where the primary website came from: org-core, or why it could not be read.
        Kind::Websites => json!({"websites":[website()],"primarySource":Value::Null}),
        Kind::Website => website(),
        Kind::GoogleConnect => object("sessionId authorizationUrl expiresAt"),
        Kind::GoogleStatus => object("status"),
        Kind::Discovery => with("websiteId site websiteRevision source status limit partial discoveredAt reason", &[("coverage",object("sitemapFiles sitemapValid sitemapPartial candidates robotsFiltered truncated")),("pages",json!([object("url title")]))]),
        Kind::Audit => {
            let page = with("url finalUrl status httpStatus title description canonical observedAt runId fingerprint reason extractionPartial", &[("seo",seo()),("links",json!([Value::Null])),("checks",json!([object("ruleId status")]))]);
            let finding = with("id ruleId severity pageUrl observedAt runId fingerprint", &[("relatedUrls",json!([Value::Null]))]);
            with("id websiteId site websiteRevision source ruleVersion status limit partial startedAt completedAt reason", &[("coverage",with("discovered attempted checked failed pending limitReached inventoryTruncated extractionPartial stopReason", &[("sitemap",object("sitemapFiles sitemapValid sitemapPartial candidates robotsFiltered truncated"))])),("pages",json!([page])),("findings",json!([finding])),("history",json!([object("id status startedAt completedAt pageCount findingCount")]))])
        }
        Kind::Monitoring => with("websiteId site websiteRevision source revision configured enabled cadence status nextRunAt updatedAt lastAuditId lastOutcome failureCount reason", &[("history",json!([object("auditId scheduledFor startedAt completedAt status attempt checkedPages findingCount reason")]))]),
        Kind::AuditInsights => with("websiteId site websiteRevision source asOf trackingSince partial historyLimited truncated comparisonAvailable latestAuditId latestStatus", &[
            ("summary",object("new unchanged resolved regressed unknown")),
            ("items",json!([with("pageUrl ruleId status checkStatus severity proofAuditId observedAt runId fingerprint", &[("relatedUrls",json!([Value::Null]))])])),
            ("alerts",json!([object("kind auditId pageUrl ruleId reason observedAt")])),
            ("availableWeeks",json!([Value::Null])),
        ]),
        Kind::WeeklyReport => with("websiteId site websiteRevision source asOf trackingSince partial historyLimited truncated weekStart endExclusive status observed failedChecks tasksAsOf", &[
            ("audits",object("total complete failed")),
            ("pages",object("attempted checked failed")),
            ("changes",object("new unchanged resolved regressed unknown")),
            ("receipts",json!([object("auditId status startedAt completedAt checkedPages failedPages findingCount reason")])),
            ("availableWeeks",json!([Value::Null])),
            ("work",object("open inProgress done dismissed total completionPercent")),
        ]),
        Kind::Preparation => with("websiteId site websiteRevision source status jobId queuedAt startedAt completedAt", &[("steps",json!([object("key status reason observedAt")]))]),
        Kind::Outcomes => with("websiteId site websiteRevision source environment status reason quarter timeZone lastReceivedAt asOf", &[("period",object("startDate endDate")),("metrics",object("inquiries leads bookings"))]),
        Kind::AnalyticsSync => with("websiteId site websiteRevision source quarter enabled status jobId queuedAt startedAt completedAt nextSyncAt expiresAt", &[("steps",json!([object("key status reason observedAt lastSuccessAt")]))]),
        Kind::PublicSignals => {
            let report = with("site source observedAt", &[
                ("crux",json!([with("formFactor status reason reportingOrigin", &[("period",object("startDate endDate")),("metrics",object("lcpMs inpMs cls"))])])),
                ("commonCrawl",with("status reason index limit truncated reportingHost", &[("captures",json!([object("url capturedAt httpStatus")]))])),
            ]);
            with("websiteId site websiteRevision source enabled status reason jobId queuedAt startedAt completedAt nextSyncAt expiresAt", &[("report",report.clone()),("cachedReport",report)])
        },
        Kind::Sources => with("websiteId site websiteRevision source", &[("items",json!([object("key implemented mode status refresh")]))]),
        Kind::WebVitals => with("websiteId site websiteRevision source environment status reportingOrigin asOf retentionDays sampleCount", &[
            ("window",object("startDate endDate")),
            ("groups",json!([with("viewport", &[("metrics",with("", &[("lcp",object("count p75")),("inp",object("count p75")),("cls",object("count p75"))]))])])),
        ]),
        Kind::SiteSources => with("websiteId site websiteRevision provider status observedAt", &[
            ("window",object("startDate endDate")),
            ("cookiebot",with("reportingDomain", &[
                ("consents",with("status reportedDays", &[("totals",object("optIn optOut preferences statistics marketing"))])),
                ("inventory",with("status scannedAt total thirdParty secure httpOnly", &[("categories",object("necessary preferences statistics marketing unclassified"))])),
            ])),
            ("algolia",with("indexName region", &[("analytics",object("status reportedDays searches noResultSearches noResultRate")),("topSearches",with("status limit", &[("rows",json!([object("query count averageResults")]))])),("noResultTerms",with("status limit", &[("rows",json!([object("query count averageResults")]))]))])),
        ]),
        Kind::AlgoliaMapping => object("websiteId site websiteRevision status connectionId applicationId indexName"),
        Kind::AlgoliaCollection => object("websiteId site websiteRevision provider status nextRunAt lastAttemptAt lastSuccessAt reason"),
        Kind::Work => {
            let verification = with("status score observedAt lighthouseVersion recordedAt auditId findingId pageUrl ruleId runId fingerprint", &[("review",object("ruleId key present matches"))]);
            let evidence = with("kind key opportunityType clicks impressions ctr position observedAt strategy checkStatus checkScore lighthouseVersion auditId findingId pageUrl severity runId fingerprint", &[("period",period),("review",with("", &[("rule",object("id name entity contains minImpressions maxPosition"))])),("relatedUrls",json!([Value::Null]))]);
            let task = with("id websiteId site revision status createdAt updatedAt", &[("evidence",evidence),("planning",object("assigneeId dueDate priority")),("verification",verification.clone())]);
            let activity = with("id taskId kind status occurredAt", &[("planning",object("assigneeId dueDate priority")),("verification",verification)]);
            with("websiteId site", &[("tasks",json!([task])),("activity",json!([activity])),("summary",object("open inProgress done dismissed total completionPercent"))])
        }
    }
}
fn project(value: &Value, schema: &Value, depth: usize, array_limit: usize) -> Option<Value> {
    if depth > 12 {
        return None;
    }
    if value.is_null() {
        return Some(Value::Null);
    }
    match schema {
        Value::Null => match value {
            Value::String(value) if value.len() <= 8192 => Some(Value::String(value.clone())),
            Value::Bool(_) | Value::Number(_) => Some(value.clone()),
            _ => None,
        },
        Value::Array(schemas) => {
            let values = value.as_array()?;
            if values.len() > array_limit {
                return None;
            }
            Some(Value::Array(
                values
                    .iter()
                    .map(|value| project(value, &schemas[0], depth + 1,array_limit))
                    .collect::<Option<_>>()?,
            ))
        }
        Value::Object(fields) => {
            let values = value.as_object()?;
            let mut result = Map::new();
            for (key, schema) in fields {
                if let Some(value) = values.get(key) {
                    result.insert(key.clone(), project(value, schema, depth + 1,array_limit)?);
                }
            }
            Some(Value::Object(result))
        }
        _ => None,
    }
}
pub(super) fn sanitize(spec: &Spec, value: &Value) -> Option<Value> {
    if !value.is_object() {
        return None;
    }
    if matches!(spec.kind, Kind::WebVitals)
        && value.get("groups").is_some_and(|groups| groups.as_array().is_none_or(|groups| groups.len() > 2))
    {
        return None;
    }
    if matches!(spec.kind, Kind::GoogleConnect) {
        let url = url::Url::parse(value.get("authorizationUrl")?.as_str()?).ok()?;
        if url.scheme() != "https"
            || url.host_str() != Some("accounts.google.com")
            || url.path() != "/o/oauth2/v2/auth"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let allowed = [
            "openid",
            "email",
            "profile",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
            "https://www.googleapis.com/auth/webmasters.readonly",
            "https://www.googleapis.com/auth/analytics.readonly",
        ];
        let scopes = url
            .query_pairs()
            .find(|(key, _)| key == "scope")?
            .1
            .into_owned();
        if scopes
            .split_whitespace()
            .any(|scope| !allowed.contains(&scope))
        {
            return None;
        }
    }
    if matches!(spec.kind, Kind::SiteSources) {
        if let Some(algolia) = value.get("algolia") {
            for key in ["topSearches", "noResultTerms"] { if let Some(terms) = algolia.get(key) { validate_terms(terms)?; } }
        }
    }
    if matches!(spec.kind, Kind::AlgoliaCollection) {
        if value.get("provider")?.as_str()? != "algolia" || !["idle","queued","syncing","ready","needs_attention","reconnect_required"].contains(&value.get("status")?.as_str()?) { return None; }
        for key in ["nextRunAt","lastAttemptAt","lastSuccessAt"] { if let Some(value)=value.get(key) { let stamp=value.as_str()?; if stamp.len()>40 || chrono::DateTime::parse_from_rfc3339(stamp).is_err() { return None; } } }
        if let Some(reason)=value.get("reason") { if !["partial_report","provider_unavailable","not_configured","reconnect_required","context_changed"].contains(&reason.as_str()?) { return None; } }
    }
    project(value, &schema(spec.kind), 0, match spec.kind { Kind::Analysis|Kind::Research=>10000,Kind::Audit|Kind::AuditInsights=>2100,_=>1000 })
}

fn validate_terms(value: &Value) -> Option<()> {
    if value.get("limit")?.as_u64()? != 20 { return None; }
    match value.get("status")?.as_str()? {
        "unavailable" if value.get("rows").is_none() => Some(()),
        "connected" => {
            let rows = value.get("rows")?.as_array()?; if rows.len()>20 { return None; }
            let mut queries=std::collections::HashSet::new();
            for row in rows {
                let query=row.get("query")?.as_str()?;
                if !queries.insert(query) || query.len()>2048 || query.chars().count()>512 || query.chars().any(char::is_control) || row.get("count")?.as_u64()? > 9_007_199_254_740_991 { return None; }
                if let Some(average)=row.get("averageResults") { let average=average.as_f64()?; if !average.is_finite() || !(0.0..=9_007_199_254_740_991.0).contains(&average) { return None; } }
            }
            Some(())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::contract::route_spec;
    use super::*;
    use axum::http::Method;
    #[test]
    fn pagespeed_daily_cache_keeps_normalized_metadata_only() {
        let spec = route_spec("pagespeed", &Method::GET).unwrap();
        let safe = sanitize(&spec, &json!({"cache":{"stale":true,"refreshStatus":"failed","lastSuccessAt":"2026-10-28T10:00:00Z","refreshReason":"quota_limited","token":"private"},"report":{"performance":91,"fetchTime":"2026-10-28T10:00:00Z","details":{"secret":"private"}}})).unwrap();
        assert_eq!(safe["cache"],json!({"stale":true,"refreshStatus":"failed","lastSuccessAt":"2026-10-28T10:00:00Z","refreshReason":"quota_limited"}));
        assert!(!safe.to_string().contains("private"));
    }
    #[test]
    fn clarity_ux_keeps_aggregates_without_replay_or_credential_fields() {
        let spec = route_spec("clarity", &Method::GET).unwrap();
        let safe = sanitize(&spec, &json!({"projectId":"project123","metrics":{"sessions":22,"ux":{"deadClickPercent":0,"averageScrollDepth":30.27,"activeSeconds":37,"token":"private","recordings":[{"user":"private"}]}},"recordingUrl":"private"})).unwrap();
        assert_eq!(safe["metrics"]["ux"],json!({"deadClickPercent":0,"averageScrollDepth":30.27,"activeSeconds":37}));
        assert!(!safe.to_string().contains("private"));
    }
    #[test]
    fn strips_nested_provider_objects_and_credentials() {
        let spec = route_spec("ga4", &Method::GET).unwrap();
        let value = sanitize(&spec,&json!({"site":"https://example.com/","token":"secret","totals":{"users":12,"connectionId":"secret"},"days":[{"date":"2026-10-22","users":2,"raw":{"key":"secret"}}],"quality":{"sampled":false,"token":"secret"}})).unwrap();
        assert_eq!(
            value,
            json!({"site":"https://example.com/","totals":{"users":12},"days":[{"date":"2026-10-22","users":2}],"quality":{"sampled":false}})
        );
    }
    #[test]
    fn objects_cannot_hide_in_scalar_fields() {
        assert!(sanitize(
            &route_spec("pulse", &Method::GET).unwrap(),
            &json!({"site":{"token":"secret"}})
        )
        .is_none());
    }
}
