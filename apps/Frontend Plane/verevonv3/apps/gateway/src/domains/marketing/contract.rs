use axum::http::Method;
use serde::{
    de::{MapAccess, Visitor},
    Deserialize,
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Collection,
    Analysis,
    Research,
    Assignees,
    Resources,
    Competitors,
    Pulse,
    Search,
    Comparison,
    Ga4,
    Outcomes,
    Clarity,
    PageSpeed,
    Websites,
    Website,
    GoogleConnect,
    GoogleStatus,
    Work,
    Discovery,
    Audit,
    Monitoring,
    AuditInsights,
    WeeklyReport,
    Preparation,
    Sources,
    AnalyticsSync,
    PublicSignals,
    Trends,
    AutomaticTrends,
    WebVitals,
    SiteSources,
    AlgoliaMapping,
    AlgoliaCollection,
}
pub(super) struct Spec {
    pub kind: Kind,
    pub administrative: bool,
    command: &'static [&'static str],
}

fn identity(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
pub(super) fn route_spec(route: &str, method: &Method) -> Option<Spec> {
    let get = *method == Method::GET;
    let post = *method == Method::POST;
    let patch = *method == Method::PATCH;
    let put = *method == Method::PUT;
    let delete = *method == Method::DELETE;
    let parts: Vec<_> = route.split('/').collect();
    let (kind, administrative, command): (Kind, bool, &[&str]) = match parts.as_slice() {
        ["collection"] if get => (Kind::Collection, false, &[]),
        ["search","research"] if get || post => (Kind::Research,false,&["site","websiteId","websiteRevision"]),
 ["search","rules"] if post => (Kind::Research,false,&["site","websiteId","websiteRevision","operation","ruleId","name","entity","contains","minImpressions","maxPosition"]),
 ["analysis"] if get => (Kind::Analysis,false,&[]),
        ["competitors"] if get || post => (Kind::Competitors,false,&["site","websiteId","websiteRevision","target","operation","ownUrl","targetUrl"]),
        ["pulse"] if get => (Kind::Pulse, false, &[]),
        ["search"] if get => (Kind::Search, false, &[]),
        ["search", "compare"] if get => (Kind::Comparison, false, &[]),
        ["ga4"] if get => (Kind::Ga4, false, &[]),
        ["outcomes"] if get => (Kind::Outcomes, false, &[]),
        ["clarity"] if get => (Kind::Clarity, false, &[]),
        ["pagespeed"] if get || post => {
            (Kind::PageSpeed, false, &["site", "websiteId", "strategy"])
        }
        ["websites"] if get => (Kind::Websites, false, &[]),
        ["websites"] if post => (Kind::Website, true, &["site"]),
        ["websites", id] if identity(id, "web_") && get => (Kind::Website, false, &[]),
        ["websites", id] if identity(id, "web_") && patch => {
            (Kind::Website, true, &["site", "revision"])
        }
        ["websites", id, "mappings"] if identity(id, "web_") && patch => (
            Kind::Website,
            true,
            &["revision", "clarityProjectId", "ga4ReportingSite"],
        ),
        ["websites", id, "verify"]
            if identity(id, "web_") && post =>
        {
            (Kind::Website, true, &[])
        }
        ["websites",id,"google","resources"] if identity(id,"web_")&&get=>(Kind::Resources,true,&[]),
 ["websites",id,"google","resources"] if identity(id,"web_")&&post=>(Kind::Website,true,&["revision","resourceId"]),
 ["websites", id, "google", "connect"] if identity(id, "web_") && post => {
            (Kind::GoogleConnect, true, &[])
        }
        ["websites", id, "google", "status"] if identity(id, "web_") && get => {
            (Kind::GoogleStatus, true, &[])
        }
        ["work","review"] if post => (Kind::Work,false,&["site","websiteId","websiteRevision","ruleId","key"]),
 ["work","assignees"] if get => (Kind::Assignees,false,&[]),
 ["work",id,"planning"] if identity(id,"task_")&&patch => (Kind::Work,false,&["site","websiteId","websiteRevision","revision","assigneeId","dueDate","priority"]),
 ["work"] if get => (Kind::Work, false, &[]),
        ["work"] if post => (
            Kind::Work,
            false,
            &["site", "websiteId", "websiteRevision", "quarter", "query"],
        ),
        ["work", "technical"] if post => (
            Kind::Work,
            false,
            &[
                "site",
                "websiteId",
                "websiteRevision",
                "strategy",
                "checkId",
                "fetchTime",
            ],
        ),
        ["work", "audit"] if post => (
            Kind::Work,
            false,
            &[
                "site",
                "websiteId",
                "websiteRevision",
                "auditId",
                "findingId",
            ],
        ),
        ["work", id] if identity(id, "task_") && patch => (
            Kind::Work,
            false,
            &["site", "websiteId", "websiteRevision", "revision", "status"],
        ),
        ["work", id, "verify"] if identity(id, "task_") && post => (
            Kind::Work,
            false,
            &["site", "websiteId", "websiteRevision", "revision"],
        ),
        ["discovery"] if get || post => (
            Kind::Discovery,
            false,
            &["site", "websiteId", "websiteRevision"],
        ),
        ["audits"] if get || post => (
            Kind::Audit,
            false,
            &["site", "websiteId", "websiteRevision"],
        ),
        ["monitoring"] if get || patch => (
            Kind::Monitoring,
            false,
            &["site", "websiteId", "websiteRevision", "scheduleRevision", "enabled", "cadence"],
        ),
        ["audit-insights"] if get => (Kind::AuditInsights, false, &[]),
        ["reports", "weekly"] if get => (Kind::WeeklyReport, false, &[]),
        ["preparation"] if get || post => (Kind::Preparation, post, &["site", "websiteId", "websiteRevision"]),
        ["sources"] if get => (Kind::Sources, false, &[]),
        ["analytics-sync"] if get => (Kind::AnalyticsSync, false, &[]),
        ["analytics-sync"] if post => (Kind::AnalyticsSync, true, &["site", "websiteId", "websiteRevision", "quarter"]),
        ["analytics-sync"] if patch => (Kind::AnalyticsSync, true, &["site", "websiteId", "websiteRevision", "quarter", "enabled"]),
        ["public-signals"] if get => (Kind::PublicSignals, false, &[]),
        ["trends"] if post => (Kind::Trends, true, &["site", "websiteId", "websiteRevision", "terms", "geo", "timeframe"]),
        ["trends", "automatic"] if get => (Kind::AutomaticTrends, false, &[]),
        ["public-signals"] if post => (Kind::PublicSignals, true, &["site", "websiteId", "websiteRevision"]),
        ["public-signals"] if patch => (Kind::PublicSignals, true, &["site", "websiteId", "websiteRevision", "enabled"]),
        ["web-vitals"] if get => (Kind::WebVitals, false, &[]),
        ["web-vitals", "sync"] if post => (Kind::WebVitals, true, &["site", "websiteId", "websiteRevision", "environment"]),
        ["site-sources"] if get => (Kind::SiteSources, false, &[]),
        ["site-sources", "sync"] if post => (Kind::SiteSources, true, &["site", "websiteId", "websiteRevision", "provider"]),
        ["site-sources", "algolia-mapping"] if get => (Kind::AlgoliaMapping, false, &[]),
        ["site-sources", "algolia-mapping"] if put => (Kind::AlgoliaMapping, true, &["site", "websiteId", "websiteRevision", "connectionId", "applicationId", "indexName"]),
        ["site-sources", "algolia-mapping"] if delete => (Kind::AlgoliaMapping, true, &["site", "websiteId", "websiteRevision"]),
        ["site-sources", "collection"] if get => (Kind::AlgoliaCollection, false, &[]),
        ["site-sources", "collection"] if post => (Kind::AlgoliaCollection, true, &["site", "websiteId", "websiteRevision", "provider"]),
        _ => return None,
    };
    Some(Spec {
        kind,
        administrative,
        command,
    })
}

fn site(value: &str) -> bool {
    crate::public_url::normalize_public_http_url(value)
        .ok()
        .filter(|canonical| canonical == value)
        .and_then(|_| url::Url::parse(value).ok())
        .is_some_and(|url| {
            url.scheme() == "https"
                && url.port().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url
                    .host_str()
                    .is_some_and(|host| host.contains('.') && !host.ends_with('.'))
        })
}
fn field(key: &str, value: &Value) -> bool {
    match key {
        "ownUrl" | "targetUrl" => value.as_str().is_some_and(|v|v.len()<=2048&&crate::public_url::normalize_public_http_url(v).ok().as_deref()==Some(v)&&url::Url::parse(v).is_ok_and(|u|u.scheme()=="https"&&u.query().is_none()&&u.fragment().is_none())),
 "resourceId" => value.as_str().is_some_and(|v|identity(v,"res_")),
 "assigneeId" => value.as_str().is_some_and(|v|v.len()<=200&&v.bytes().all(|c|c.is_ascii_alphanumeric()||b"._:-".contains(&c))),
 "dueDate" => value.as_str().is_some_and(|v|v.is_empty()||v.len()==10&&chrono::NaiveDate::parse_from_str(v,"%Y-%m-%d").is_ok_and(|d|d.to_string()==v)),
 "priority" => value.as_str().is_some_and(|v|matches!(v,"low"|"normal"|"high"|"urgent")),
 "key" => value.as_str().is_some_and(|v|!v.is_empty()&&v.len()<=2048&&!v.chars().any(char::is_control)),
 "ruleId" => value.as_str().is_some_and(|v| !v.is_empty() && v.len()<=40 && v.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||b"_-".contains(&c))),
 "name" => value.as_str().is_some_and(|v|!v.is_empty()&&v.len()<=100&&!v.chars().any(char::is_control)),
 "entity" => value.as_str().is_some_and(|v|matches!(v,"query"|"page")),
 "contains" => value.as_str().is_some_and(|v|v.len()<=200&&!v.chars().any(char::is_control)),
 "minImpressions" => value.as_u64().is_some_and(|v|v<=1000000000),
 "maxPosition" => value.as_f64().is_some_and(|v|(0.0..=1000.0).contains(&v)),
 "terms" => value.as_array().is_some_and(|terms| {
            (1..=5).contains(&terms.len()) && terms.iter().all(|term| term.as_str().is_some_and(|term| !term.is_empty() && term.trim()==term && term.len()<=80 && !term.chars().any(char::is_control)))
                && terms.iter().enumerate().all(|(i,term)| !terms[..i].contains(term))
        }),
        "geo" => value.as_str().is_some_and(|v| v.is_empty() || v.len()==2 && v.bytes().all(|c| c.is_ascii_uppercase())),
        "timeframe" => value.as_str().is_some_and(|v| ["today 3-m", "today 12-m", "today 5-y"].contains(&v)),
        "operation" => value.as_str().is_some_and(|v|matches!(v,"refresh"|"remove"|"save_rule"|"remove_rule")),
        "target" | "site" | "ga4ReportingSite" => value.as_str().is_some_and(site),
        "websiteId" => value.as_str().is_some_and(|value| identity(value, "web_")),
        "auditId" => value
            .as_str()
            .is_some_and(|value| identity(value, "audit_")),
        "findingId" => value
            .as_str()
            .is_some_and(|value| identity(value, "finding_")),
        "revision" | "websiteRevision" => value
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 9_007_199_254_740_991),
        "scheduleRevision" => value.as_u64().is_some_and(|n| n <= 9_007_199_254_740_991),
        "enabled" => value.is_boolean(),
        "cadence" => value.as_str().is_some_and(|value| matches!(value, "daily" | "weekly")),
        "weekStart" => value.as_str().is_some_and(|value| {
            value.len() == 10 && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok_and(|date| {
                date.to_string() == value && chrono::Datelike::weekday(&date) == chrono::Weekday::Mon
                    && date <= chrono::Utc::now().date_naive()
            })
        }),
        "quarter" => value
            .as_str()
            .is_some_and(|value| matches!(value, "Q1" | "Q2" | "Q3" | "Q4" | "FY")),
        "strategy" => value
            .as_str()
            .is_some_and(|value| matches!(value, "mobile" | "desktop")),
        "environment" => value
            .as_str()
            .is_some_and(|value| matches!(value, "production" | "development")),
        "provider" => value
            .as_str()
            .is_some_and(|value| matches!(value, "cookiebot" | "algolia")),
        "dimension" => value
            .as_str()
            .is_some_and(|value| matches!(value, "query" | "page")),
        "connectionId" => value.as_str().is_some_and(|value| value.strip_prefix("conn_").is_some_and(|tail| !tail.is_empty() && tail.len() <= 100 && tail.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)))),
        "applicationId" => value.as_str().is_some_and(|value| !value.is_empty() && value.len() <= 64 && value.bytes().all(|c| c.is_ascii_alphanumeric())),
        "indexName" => value.as_str().is_some_and(|value| !value.is_empty() && value.len() <= 128 && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))),
        "rowLimit" => value.as_str() == Some("100"),
        "currentStart" | "currentEnd" | "previousStart" | "previousEnd" => {
            value.as_str().is_some_and(|value| {
                value.len() == 10
                    && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                        .is_ok_and(|date| date.to_string() == value)
            })
        }
        "status" => value
            .as_str()
            .is_some_and(|value| matches!(value, "todo" | "in_progress" | "done" | "dismissed")),
        "clarityProjectId" => value.as_str().is_some_and(|value| {
            (4..=32).contains(&value.len())
                && value
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }),
        "fetchTime" => value.as_str().is_some_and(|value| {
            value.len() <= 40 && chrono::DateTime::parse_from_rfc3339(value).is_ok()
        }),
        "query" => value.as_str().is_some_and(|value| {
            !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        }),
        "sessionId" => value.as_str().is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 200
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        }),
        "checkId" => value.as_str().is_some_and(|value| {
            [
                "document-title",
                "meta-description",
                "http-status-code",
                "is-crawlable",
                "robots-txt",
                "canonical",
                "hreflang",
                "link-text",
                "crawlable-anchors",
                "image-alt",
                "structured-data",
            ]
            .contains(&value)
        }),
        _ => false,
    }
}

pub(super) fn validate_query(spec: &Spec, query: &str) -> Result<(), ()> {
    let expected: &[&str] = match spec.kind {
        Kind::Collection | Kind::Analysis | Kind::Pulse | Kind::Search | Kind::Ga4 | Kind::AnalyticsSync => &["quarter", "site", "websiteId"],
        Kind::Comparison => &[
            "site",
            "websiteId",
            "dimension",
            "currentStart",
            "currentEnd",
            "previousStart",
            "previousEnd",
            "rowLimit",
        ],
        Kind::Clarity | Kind::Work | Kind::Discovery | Kind::Monitoring | Kind::AuditInsights | Kind::Preparation | Kind::Sources | Kind::PublicSignals | Kind::AutomaticTrends => &["site", "websiteId"],
        Kind::Audit => &["site", "websiteId", "auditId"],
        Kind::WeeklyReport => &["site", "websiteId", "weekStart"],
        Kind::PageSpeed => &["site", "websiteId", "strategy"],
        Kind::WebVitals => &["site", "websiteId", "environment"],
        Kind::Outcomes => &["site", "websiteId", "quarter", "environment"],
        Kind::SiteSources | Kind::AlgoliaCollection => &["site", "websiteId", "provider"],
        Kind::Assignees | Kind::Research | Kind::AlgoliaMapping | Kind::Competitors => &["site", "websiteId"],
        Kind::GoogleStatus => &["sessionId"],
        _ => &[],
    };
    let mut fields = BTreeMap::new();
    if query.len() > 8192 {
        return Err(());
    }
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if !expected.contains(&key.as_ref())
            || !field(&key, &Value::String(value.to_string()))
            || fields.insert(key.to_string(), value.to_string()).is_some()
        {
            return Err(());
        }
    }
    let optional_selector = matches!(spec.kind, Kind::Audit) && !fields.contains_key("auditId")
        || matches!(spec.kind, Kind::WeeklyReport) && !fields.contains_key("weekStart");
    if fields.len() != expected.len() - usize::from(optional_selector)
        || matches!(spec.kind, Kind::Audit | Kind::WeeklyReport)
            && (!fields.contains_key("site") || !fields.contains_key("websiteId"))
    {
        return Err(());
    }
    if matches!(spec.kind, Kind::AlgoliaCollection) && fields.get("provider").map(String::as_str) != Some("algolia") { return Err(()); }
    if matches!(spec.kind, Kind::Comparison) {
        let date = |key: &str| {
            chrono::NaiveDate::parse_from_str(fields.get(key).ok_or(())?, "%Y-%m-%d")
                .map_err(|_| ())
        };
        let start = date("currentStart")?;
        let end = date("currentEnd")?;
        let previous_start = date("previousStart")?;
        let previous_end = date("previousEnd")?;
        let days = (end - start).num_days() + 1;
        if !(1..=90).contains(&days)
            || end >= chrono::Utc::now().date_naive()
            || previous_end.succ_opt() != Some(start)
            || end - start != previous_end - previous_start
        {
            return Err(());
        }
    }
    Ok(())
}

// JSON's usual map decoder accepts duplicate keys. Reject them at this boundary
// so the BFF and Go backend can never interpret different commands.
struct UniqueObject(Map<String, Value>);
impl<'de> Deserialize<'de> for UniqueObject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = UniqueObject;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a unique command object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut result = Map::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if result.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate command field"));
                    }
                }
                Ok(UniqueObject(result))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}
pub(super) fn validate_command(spec: &Spec, bytes: &[u8]) -> Result<Value, ()> {
    if bytes.len() > 8192 {
        return Err(());
    }
    let UniqueObject(command) = serde_json::from_slice(bytes).map_err(|_| ())?;
    if matches!(spec.kind, Kind::AlgoliaCollection) && command.get("provider").and_then(Value::as_str) != Some("algolia") { return Err(()); }
    let mapping = spec.command.contains(&"clarityProjectId");
    let expected_len = if mapping { 2 } else if matches!(spec.kind,Kind::Competitors)&&!command.contains_key("ownUrl")&&!command.contains_key("targetUrl"){5}else { spec.command.len() };
    if command.len() != expected_len
        || command
            .iter()
            .any(|(key, value)| !spec.command.contains(&key.as_str()) || !field(key, value))
        || (mapping && !command.contains_key("revision"))
    {
        return Err(());
    }
    Ok(Value::Object(command))
}

pub(super) fn retired_dns_route(route: &str) -> bool {
    matches!(route.split('/').collect::<Vec<_>>().as_slice(), ["websites", id, "ownership", "challenge" | "verify"] if identity(id,"web_"))
}
