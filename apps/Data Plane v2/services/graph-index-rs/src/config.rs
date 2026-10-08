use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_database_url")]
    pub database_url: String,

    #[serde(default = "default_nats_url")]
    pub nats_url: String,

    #[serde(default = "default_admin_port")]
    pub admin_port: u16,

    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    // Extraction backend selector: "model_plane" is the only backend (route LLM
    // extraction through inference-core's Infer RPC, honoring the no-independent-LLM-
    // outside-Model-Plane rule). The legacy direct-Azure backend is deleted; the
    // field stays so a deployment that still asks for it is a startup error that
    // names what to remove, not a quiet change of behaviour.
    #[serde(
        default = "default_extraction_provider",
        rename = "graph_extraction_provider",
        alias = "extraction_provider"
    )]
    pub extraction_provider: String,

    // Model Plane inference-core gRPC endpoint + routing hints for extraction.
    #[serde(default = "default_model_plane_grpc_url")]
    pub model_plane_ai_core_grpc_url: String,
    #[serde(default = "default_extraction_model")]
    pub model_plane_extraction_model: String,
    #[serde(default = "default_extraction_provider_hint")]
    pub model_plane_extraction_provider: String,
    #[serde(default = "default_extraction_timeout_ms")]
    pub model_plane_extraction_timeout_ms: u64,
    // Auth Core service-principal contract used for the inference-core gRPC
    // hop. The API key is deliberately empty by default so production startup
    // fails closed unless a deployment secret is supplied.
    #[serde(default = "default_model_plane_inference_token_url")]
    pub model_plane_inference_token_url: String,
    #[serde(default = "default_model_plane_inference_token_issuer")]
    pub model_plane_inference_token_issuer: String,
    #[serde(default = "default_model_plane_inference_service_id")]
    pub model_plane_inference_service_id: String,
    /// Retention posture auth-core is configured to mint for this service's
    /// `inference-core` audience: `persistent` (zdr:false), `zdr` (zdr:true) or
    /// `tenant` (zdr is the ORGANIZATION's own setting, resolved at the mint;
    /// either value is accepted and the bearer carries it, and the token lives
    /// 60 s). Must match the principal's `retentionByAudience` entry in the
    /// principal registry, or every mint fails closed; a `tenant` client works
    /// against every entry, which is why clients ship first and the registry
    /// follows. Read from `MODEL_PLANE_INFERENCE_RETENTION_POSTURE`. An unknown
    /// value is a startup error.
    #[serde(default = "default_model_plane_inference_retention_posture")]
    pub model_plane_inference_retention_posture: String,
    #[serde(default)]
    pub model_plane_inference_service_api_key: String,

    #[serde(default = "default_max_entities_per_chunk")]
    pub max_entities_per_chunk: usize,

    /// The poison cap (CR-T12): the attempts after which a chunk is `failed_permanent` and
    /// nothing retries it until an operator requeues it. Read from
    /// `GRAPH_EXTRACTION_MAX_ATTEMPTS`; `1` to `50`, default `6`; anything else is a startup
    /// error (see [`Config::validate`]).
    #[serde(default = "default_graph_extraction_max_attempts")]
    pub graph_extraction_max_attempts: u32,

    /// The deployment-wide background LLM budget (owner default D-E: configurable per
    /// organization and globally, alert-only by default, no figure built in). Blank is unset.
    /// `BACKGROUND_BUDGET_MODE` is `alert_only` (default), `throttle` or `stop`: what an
    /// organization that does not set its own `budget_mode` gets. See `budget`.
    #[serde(default)]
    pub background_budget_mode: String,
    /// `BACKGROUND_BUDGET_MONTHLY_USD`: the figure an organization without one of its own is held
    /// to. Unset means none.
    #[serde(default)]
    pub background_budget_monthly_usd: String,
    /// `BACKGROUND_PRICE_INPUT_USD_PER_MTOK` and `BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK`: what a
    /// million tokens cost, which turns the ledger's tokens into the dollars a budget is stated
    /// in. Both or neither; never defaulted.
    #[serde(default)]
    pub background_price_input_usd_per_mtok: String,
    #[serde(default)]
    pub background_price_output_usd_per_mtok: String,

    /// `COMMUNITY_SUMMARY_ENABLED` and `COMMUNITY_SUMMARY_MAX_PER_RUN`: the paid community
    /// summaries (off by default, five a run). They are read again where they are used
    /// (`community`); they are held here so that a value that cannot work is refused at boot
    /// instead of leaving a paid switch in a position its owner did not choose.
    #[serde(default)]
    pub community_summary_enabled: String,
    #[serde(default)]
    pub community_summary_max_per_run: String,

    // Minimum member count for a derived community. Consumed by the
    // post-extraction refresh and the /v1/graph/communities/rebuild endpoint.
    #[serde(default = "default_community_min_size")]
    pub community_min_size: usize,

    // Neo4j graph read-model (Phase 2). Disabled by default so the change is
    // additive: when off, graph traversal falls back to the Postgres BFS path.
    // When `neo4j_enabled` is true, `main` fails startup closed if the password
    // is empty (secret must be deployment-supplied).
    #[serde(default)]
    pub neo4j_enabled: bool,
    #[serde(default = "default_neo4j_url")]
    pub neo4j_url: String,
    #[serde(default = "default_neo4j_user")]
    pub neo4j_user: String,
    #[serde(default)]
    pub neo4j_password: String,
    #[serde(default = "default_neo4j_database")]
    pub neo4j_database: String,
    // Boot connect retry count (3s apart). Must outlast a Neo4j cold start so
    // NEO4J_ENABLED=true doesn't silently lose the startup race. Default 30
    // (≈90s). Env: NEO4J_BOOT_ATTEMPTS.
    #[serde(default = "default_neo4j_boot_attempts")]
    pub neo4j_boot_attempts: u32,
    // Multi-hop traversal bounds (Phase 4). Requests are clamped to these; the
    // hop count is additionally ceilinged at `neo4j::MAX_HOPS_CEILING`.
    #[serde(default = "default_graph_max_hops")]
    pub graph_max_hops: u8,
    #[serde(default = "default_graph_traverse_max_entities")]
    pub graph_traverse_max_entities: usize,

    #[serde(default)]
    pub embedding_event_public_key_path: String,
    #[serde(default)]
    pub index_event_public_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
    /// The private key that signs graph-index's usage events (CR-T10): issuer
    /// `service:graph-index-rs`, key id `graph-events-v1`, scope `events:graph:publish`.
    /// Its public half is mounted into data-orchestrator. Production keys are provisioned at an
    /// absolute path by the secret manager, never under `.secrets/event-keys`. Read only when
    /// `USAGE_EMIT_ENABLED` is on.
    #[serde(default)]
    pub graph_event_private_key_path: String,
    /// The usage ledger (CR-T10): off by default. See `usage`. Read from `USAGE_EMIT_ENABLED`;
    /// `1` or `true` turns it on. Turned on, graph-index refuses to start unless signed events
    /// are enabled, the key above is readable and the durable stream can be created or verified.
    /// What graph-index records with it (CR-T12): one event per paid call, features
    /// `graph-extraction` and `graph-community-summary`, counters only.
    #[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]
    pub usage_emit_enabled: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config: Self = envy::from_env().map_err(|e| anyhow::anyhow!("config: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    /// The boot-time check of every setting CR-T12 added: a value that cannot work is a startup
    /// error that names the variable, not a service that runs with a cap it silently ignores.
    pub fn validate(&self) -> anyhow::Result<()> {
        let cap = self.graph_extraction_max_attempts;
        anyhow::ensure!(
            (1..=crate::state::MAX_ATTEMPTS_CEILING as u32).contains(&cap),
            "GRAPH_EXTRACTION_MAX_ATTEMPTS must be from 1 to {}, got {cap}: it is the poison cap, \
             and a cap of nothing or of everything is no cap",
            crate::state::MAX_ATTEMPTS_CEILING
        );
        self.background_budget()?;
        crate::community::parse_summaries_enabled(&self.community_summary_enabled)?;
        crate::community::parse_max_summaries_per_run(&self.community_summary_max_per_run)?;
        Ok(())
    }

    /// The deployment-wide background budget, parsed and checked.
    pub fn background_budget(&self) -> anyhow::Result<crate::budget::GlobalBudget> {
        Ok(crate::budget::GlobalBudget::parse(
            &self.background_budget_mode,
            &self.background_budget_monthly_usd,
            &self.background_price_input_usd_per_mtok,
            &self.background_price_output_usd_per_mtok,
        )?)
    }
}

fn default_database_url() -> String {
    "postgres://dataplane:dataplane@localhost:5442/dataplane".into()
}

fn default_nats_url() -> String {
    "nats://localhost:4232".into()
}

fn default_admin_port() -> u16 {
    9203
}

fn default_grpc_port() -> u16 {
    // Matches the Model Plane gateway's DATAPLANE_GRAPH_ADDR default (:50053).
    50053
}

fn default_extraction_provider() -> String {
    "model_plane".into()
}

fn default_model_plane_grpc_url() -> String {
    "http://inference-core:9092".into()
}

fn default_extraction_model() -> String {
    "gpt-4o".into()
}

fn default_extraction_provider_hint() -> String {
    "azure_openai".into()
}

fn default_extraction_timeout_ms() -> u64 {
    60_000
}

fn default_model_plane_inference_token_url() -> String {
    "http://auth-core:3011/api/inference-core/internal-token".into()
}

fn default_model_plane_inference_token_issuer() -> String {
    "http://localhost:3011/api/convex-auth".into()
}

/// Matches the deployed registry, which pins `graph-index` -> `inference-core`
/// to `persistent`. Note this is the opposite of auth-core's own default for an
/// *unconfigured* audience (`zdr`), so tightening the registry requires setting
/// this variable too. The enablement step sets it to `tenant` after the Control
/// Plane image has the tenant code (see the contextual retrieval runbook);
/// `tenant` also works against the `persistent` entry, so nothing breaks if the
/// variable is set first.
fn default_model_plane_inference_retention_posture() -> String {
    "persistent".into()
}
fn default_model_plane_inference_service_id() -> String {
    "graph-index".into()
}

fn default_max_entities_per_chunk() -> usize {
    20
}

fn default_graph_extraction_max_attempts() -> u32 {
    crate::state::DEFAULT_MAX_ATTEMPTS as u32
}

fn default_neo4j_boot_attempts() -> u32 {
    30
}
fn default_community_min_size() -> usize {
    3
}

fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

fn default_neo4j_url() -> String {
    "bolt://neo4j:7687".into()
}

fn default_neo4j_user() -> String {
    "neo4j".into()
}

fn default_neo4j_database() -> String {
    "neo4j".into()
}

fn default_graph_max_hops() -> u8 {
    3
}

fn default_graph_traverse_max_entities() -> usize {
    100
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn graph_provider_uses_the_deployment_environment_contract() {
        let config: Config = envy::from_iter([(
            "GRAPH_EXTRACTION_PROVIDER".to_owned(),
            "azure_openai".to_owned(),
        )])
        .expect("config");

        assert_eq!(config.extraction_provider, "azure_openai");

        let legacy: Config =
            envy::from_iter([("EXTRACTION_PROVIDER".to_owned(), "azure_openai".to_owned())])
                .expect("legacy config");
        assert_eq!(legacy.extraction_provider, "azure_openai");
    }

    #[test]
    fn neo4j_defaults_off_with_internal_connection_defaults() {
        let cfg: Config = envy::from_iter([]).expect("config");
        assert!(
            !cfg.neo4j_enabled,
            "neo4j must be off by default (additive)"
        );
        assert_eq!(cfg.neo4j_url, "bolt://neo4j:7687");
        assert_eq!(cfg.neo4j_user, "neo4j");
        assert_eq!(cfg.neo4j_database, "neo4j");
        assert!(cfg.neo4j_password.is_empty());
    }

    /// The usage ledger (CR-T10) is off unless the switch is turned on, and `1` turns it on (the
    /// compose default is `0`, so both spellings of a boolean have to read right).
    #[test]
    fn the_usage_ledger_is_off_by_default_and_reads_one_and_true() {
        let off: Config = envy::from_iter([]).expect("config");
        assert!(!off.usage_emit_enabled);
        assert!(off.graph_event_private_key_path.is_empty());
        for (value, expected) in [("1", true), ("true", true), ("0", false), ("false", false)] {
            let cfg: Config =
                envy::from_iter([("USAGE_EMIT_ENABLED".to_owned(), value.to_owned())])
                    .expect("config");
            assert_eq!(
                cfg.usage_emit_enabled, expected,
                "USAGE_EMIT_ENABLED={value}"
            );
        }
        let keyed: Config = envy::from_iter([(
            "GRAPH_EVENT_PRIVATE_KEY_PATH".to_owned(),
            "/run/event-keys/graph-events.pem".to_owned(),
        )])
        .expect("config");
        assert_eq!(
            keyed.graph_event_private_key_path,
            "/run/event-keys/graph-events.pem"
        );
    }

    fn from(pairs: &[(&str, &str)]) -> Config {
        envy::from_iter(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        )
        .expect("config")
    }

    /// Every switch CR-T12 added has a default, and the defaults are the safe ones: the cap the
    /// design names, the budget alert-only with no figure and no prices (owner default D-E).
    #[test]
    fn the_new_settings_default_to_the_designs_cap_and_an_alert_only_budget() {
        let config = from(&[]);
        assert_eq!(config.graph_extraction_max_attempts, 6);
        let budget = config.background_budget().expect("the defaults are valid");
        assert_eq!(budget, crate::budget::GlobalBudget::default());
        config.validate().expect("an empty environment boots");
        // What compose passes when nothing is set: blank values, which mean unset.
        let blank = from(&[
            ("BACKGROUND_BUDGET_MODE", ""),
            ("BACKGROUND_BUDGET_MONTHLY_USD", ""),
            ("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", ""),
            ("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", ""),
            ("GRAPH_EXTRACTION_MAX_ATTEMPTS", "6"),
        ]);
        blank.validate().expect("blank is unset, not an error");
        assert_eq!(
            blank.background_budget().expect("valid"),
            crate::budget::GlobalBudget::default()
        );
    }

    #[test]
    fn the_poison_cap_is_checked_at_boot() {
        for good in ["1", "6", "50"] {
            from(&[("GRAPH_EXTRACTION_MAX_ATTEMPTS", good)])
                .validate()
                .unwrap_or_else(|error| panic!("{good}: {error}"));
        }
        for bad in ["0", "51", "1000"] {
            let error = from(&[("GRAPH_EXTRACTION_MAX_ATTEMPTS", bad)])
                .validate()
                .expect_err(bad);
            assert!(
                error.to_string().contains("GRAPH_EXTRACTION_MAX_ATTEMPTS"),
                "{bad}: {error}"
            );
        }
        // A cap that is not a number does not even parse: also a startup error.
        assert!(envy::from_iter::<_, Config>([(
            "GRAPH_EXTRACTION_MAX_ATTEMPTS".to_owned(),
            "many".to_owned()
        )])
        .is_err());
    }

    /// The paid community summaries have a default (off, five a run) and a boot check: what
    /// compose passes when nothing is set boots, and a typo stops the service, naming the variable.
    #[test]
    fn the_summary_switches_have_defaults_and_are_checked_at_boot() {
        from(&[
            ("COMMUNITY_SUMMARY_ENABLED", "0"),
            ("COMMUNITY_SUMMARY_MAX_PER_RUN", "5"),
        ])
        .validate()
        .expect("the compose defaults boot");
        from(&[
            ("COMMUNITY_SUMMARY_ENABLED", ""),
            ("COMMUNITY_SUMMARY_MAX_PER_RUN", ""),
        ])
        .validate()
        .expect("blank is unset");
        from(&[("COMMUNITY_SUMMARY_ENABLED", "true")])
            .validate()
            .expect("on is a valid choice");
        for (pairs, names) in [
            (
                vec![("COMMUNITY_SUMMARY_ENABLED", "ture")],
                "COMMUNITY_SUMMARY_ENABLED",
            ),
            (
                vec![("COMMUNITY_SUMMARY_MAX_PER_RUN", "0")],
                "COMMUNITY_SUMMARY_MAX_PER_RUN",
            ),
            (
                vec![("COMMUNITY_SUMMARY_MAX_PER_RUN", "lots")],
                "COMMUNITY_SUMMARY_MAX_PER_RUN",
            ),
        ] {
            let error = from(&pairs).validate().expect_err(names);
            assert!(error.to_string().contains(names), "{names}: {error}");
        }
    }

    #[test]
    fn a_budget_that_cannot_work_is_a_startup_error_naming_the_variable() {
        for (pairs, names) in [
            (
                vec![("BACKGROUND_BUDGET_MODE", "sometimes")],
                "BACKGROUND_BUDGET_MODE",
            ),
            (
                vec![("BACKGROUND_BUDGET_MONTHLY_USD", "-3")],
                "BACKGROUND_BUDGET_MONTHLY_USD",
            ),
            (
                vec![("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", "0.15")],
                "set together or not at all",
            ),
            (
                vec![
                    ("BACKGROUND_BUDGET_MODE", "stop"),
                    ("BACKGROUND_BUDGET_MONTHLY_USD", "25"),
                ],
                "the stop would never fire",
            ),
        ] {
            let error = from(&pairs).validate().expect_err(names);
            assert!(error.to_string().contains(names), "{names}: {error}");
        }
        from(&[
            ("BACKGROUND_BUDGET_MODE", "stop"),
            ("BACKGROUND_BUDGET_MONTHLY_USD", "25"),
            ("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", "0.15"),
            ("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", "0.6"),
        ])
        .validate()
        .expect("a stop with a figure and prices is a budget that can fire");
    }

    #[test]
    fn neo4j_enabled_parses_from_env_bool() {
        let cfg: Config = envy::from_iter([
            ("NEO4J_ENABLED".to_owned(), "true".to_owned()),
            ("NEO4J_URL".to_owned(), "bolt://neo4j-test:7687".to_owned()),
            ("NEO4J_PASSWORD".to_owned(), "secret".to_owned()),
        ])
        .expect("config");
        assert!(cfg.neo4j_enabled);
        assert_eq!(cfg.neo4j_url, "bolt://neo4j-test:7687");
        assert_eq!(cfg.neo4j_password, "secret");
    }
}
