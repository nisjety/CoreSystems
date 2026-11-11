//! The saved recipe a browser run replays, read from capability-core (plan 3.4 E4.2 step 3).
//!
//! A recipe run takes its procedure from a [`RecipeSource`]. [`GrpcRecipeSource`] reads it through
//! capability-core's `GetProcedureVersion` (3c), with a `capability:read` token minted for the run's
//! org: the credential the skill bundle mounts already use. The answer must be the procedure that was
//! asked for, and its body must hash to the digest the answer names, before anything parses a byte of
//! it. A refused read is never parsed and is not retried here.
//!
//! The reader writes nothing and keeps nothing. A saved procedure is org configuration rather than
//! conversation content, so a zero-retention run may read one, and its bytes live only as long as the
//! run that holds them.
//!
//! The runner is off unless `EXECUTION_CORE_RECIPE_RUNNER` is the literal `true`. A recipe run is an
//! ordinary browser run: it has its own broker grant, its approvals and budgets are the existing gates,
//! and no stored approval is ever reused.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mp_contracts::model_plane::v1::capability_core_client::CapabilityCoreClient;
use mp_contracts::model_plane::v1::GetProcedureVersionRequest;
use sha2::{Digest, Sha256};
use tonic::transport::{Channel, Endpoint};
use tonic::{Request, Status};

use crate::capability_policy::ServiceTokenProvider;

/// The switch that turns the recipe runner on. Only the literal `true` turns it on.
pub const RECIPE_RUNNER_SWITCH: &str = "EXECUTION_CORE_RECIPE_RUNNER";
/// The longest procedure identifier a run may name. `proc_` and a UUID is 41 bytes.
pub const MAX_PROCEDURE_ID_BYTES: usize = 64;
/// The longest tag a run may name. capability-core refuses a longer one.
pub const MAX_TAG_BYTES: usize = 32;
/// The longest person identifier a run may name.
pub const MAX_FOR_USER_ID_BYTES: usize = 128;
/// capability-core's address, read the way `grpc.rs` reads it for its own capability channel.
const CAPABILITY_CORE_ADDR: &str = "CAPABILITY_CORE_ADDR";
const DEFAULT_CAPABILITY_CORE_ADDR: &str = "http://capability-core:9097";
/// The connect bound every outbound channel in this crate uses.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// The reason a read token is minted for. Auth Core audits it.
const TOKEN_REASON: &str = "saved procedure read for a recipe browser run";

/// Whether the recipe runner is switched on. Strict: only the literal `true` enables it, so an unset,
/// empty or differently spelled value leaves it off.
#[must_use]
pub fn recipe_runner_enabled() -> bool {
    matches!(std::env::var(RECIPE_RUNNER_SWITCH).as_deref(), Ok("true"))
}

/// One saved procedure version as read: its digest, its name and its canonical body.
pub struct FetchedRecipe {
    /// Lowercase hex SHA-256 of `body`, which the reader has checked.
    pub digest: String,
    /// The procedure's name.
    pub name: String,
    /// The canonical procedure JSON the digest was taken over.
    pub body: Vec<u8>,
}

impl std::fmt::Debug for FetchedRecipe {
    // The body is a person's saved steps, so Debug shows its size and never its content.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchedRecipe")
            .field("digest", &self.digest)
            .field("name", &self.name)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// Where a run's saved procedure is read from: capability-core in production, a fake in tests.
#[tonic::async_trait]
pub trait RecipeSource: Send + Sync {
    /// The saved procedure `procedure_id` of `org_id`, read for the person `for_user_id`. A zero or
    /// absent `version` reads the version `tag` names, or the newest version when no tag is named.
    /// Naming both a version and a tag is refused by the reader.
    ///
    /// # Errors
    ///
    /// The reader's status when it refuses or cannot answer. `data_loss` when the answer is not the
    /// procedure asked for, or its body does not hash to the digest it names.
    async fn get(
        &self,
        org_id: &str,
        procedure_id: &str,
        version: Option<u32>,
        tag: Option<&str>,
        for_user_id: &str,
    ) -> Result<FetchedRecipe, Status>;
}

/// capability-core over gRPC. Each read carries a `capability:read` token minted for the run's org.
pub struct GrpcRecipeSource {
    channel: Channel,
    tokens: ServiceTokenProvider,
}

impl GrpcRecipeSource {
    /// Builds the source from `CAPABILITY_CORE_ADDR` (or its default) and execution-core's
    /// service-token configuration. The channel connects on first use.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is not a valid endpoint, or when the service-token
    /// configuration is missing or invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        let address = std::env::var(CAPABILITY_CORE_ADDR)
            .unwrap_or_else(|_| DEFAULT_CAPABILITY_CORE_ADDR.to_owned());
        let channel = Endpoint::from_shared(address)?
            .connect_timeout(CONNECT_TIMEOUT)
            .connect_lazy();
        Ok(Self {
            channel,
            tokens: ServiceTokenProvider::from_env()?,
        })
    }

    async fn authorized<T>(&self, org_id: &str, value: T) -> Result<Request<T>, Status> {
        let bearer = self
            .tokens
            .token_with_scopes(org_id, &["capability:read"], TOKEN_REASON)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "recipe credential unavailable");
                Status::unavailable("recipe credential unavailable")
            })?;
        let mut request = Request::new(value);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}")
                .parse()
                .map_err(|_| Status::internal("recipe credential is not forwardable"))?,
        );
        Ok(request)
    }
}

#[tonic::async_trait]
impl RecipeSource for GrpcRecipeSource {
    async fn get(
        &self,
        org_id: &str,
        procedure_id: &str,
        version: Option<u32>,
        tag: Option<&str>,
        for_user_id: &str,
    ) -> Result<FetchedRecipe, Status> {
        let request = self
            .authorized(
                org_id,
                GetProcedureVersionRequest {
                    procedure_id: procedure_id.to_owned(),
                    version: version.unwrap_or(0),
                    tag: tag.unwrap_or_default().to_owned(),
                    for_user_id: for_user_id.to_owned(),
                },
            )
            .await?;
        let answer = CapabilityCoreClient::new(self.channel.clone())
            .get_procedure_version(request)
            .await?
            .into_inner();
        // The answer must be the procedure that was asked for, and its body must hash to the digest
        // it names. Anything else is refused here, before a caller parses the body.
        let asked_version = version.filter(|asked| *asked != 0);
        if answer.procedure_id != procedure_id
            || asked_version.is_some_and(|asked| answer.version != asked)
        {
            return Err(Status::data_loss("recipe_answer_mismatch"));
        }
        if sha256_hex(&answer.body_json) != answer.digest {
            return Err(Status::data_loss("recipe_digest_mismatch"));
        }
        Ok(FetchedRecipe {
            digest: answer.digest,
            name: answer.name,
            body: answer.body_json,
        })
    }
}

/// The process's recipe source, built from the deployment configuration on first use. `None` when
/// that configuration is absent or invalid, and a recipe run then fails closed.
#[must_use]
pub fn default_source() -> Option<Arc<dyn RecipeSource>> {
    static SOURCE: OnceLock<Option<Arc<dyn RecipeSource>>> = OnceLock::new();
    SOURCE
        .get_or_init(|| match GrpcRecipeSource::from_env() {
            Ok(source) => Some(Arc::new(source) as Arc<dyn RecipeSource>),
            Err(error) => {
                tracing::warn!(%error, "recipe source unavailable; recipe runs fail closed");
                None
            }
        })
        .clone()
}

/// Why a recipe request is refused before anything is read, in fixed text. `grant_owner` is the
/// person the run's broker grant was issued to: a recipe reads that person's procedures and no other.
#[must_use]
pub fn request_refusal(
    procedure_id: &str,
    version: Option<u32>,
    tag: Option<&str>,
    for_user_id: &str,
    grant_owner: &str,
) -> Option<&'static str> {
    if procedure_id.is_empty() || procedure_id.len() > MAX_PROCEDURE_ID_BYTES {
        return Some("recipe id is invalid");
    }
    if for_user_id.is_empty() || for_user_id.len() > MAX_FOR_USER_ID_BYTES {
        return Some("recipe for_user_id is invalid");
    }
    if for_user_id != grant_owner {
        return Some("recipe for_user_id does not match the run's grant");
    }
    if tag.is_some_and(|tag| tag.is_empty() || tag.len() > MAX_TAG_BYTES) {
        return Some("recipe tag is invalid");
    }
    if tag.is_some() && version.is_some_and(|version| version != 0) {
        return Some("recipe names a version or a tag, not both");
    }
    None
}

/// Lowercase hex SHA-256 of `bytes`, the form the digests are kept in.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
