use crate::{
    backend::{Backend, Failure},
    catalog::{self, Params},
    config::AccessScope,
};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Destination {
    Local,
    Gcs,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionPlan {
    Accounting {
        entities: Vec<String>,
        #[serde(default = "incremental")]
        mode: String,
        #[serde(default)]
        from: Option<NaiveDate>,
        #[serde(default)]
        to: Option<NaiveDate>,
        #[serde(default)]
        as_of: Option<NaiveDate>,
        #[serde(default = "window")]
        window_days: i64,
        #[serde(default)]
        expand_details: bool,
        #[serde(default = "detail_cap")]
        max_detail_records: usize,
    },
    Extended {
        family: String,
        #[serde(default)]
        from: Option<NaiveDate>,
        #[serde(default)]
        to: Option<NaiveDate>,
        #[serde(default)]
        bank_account_ids: Vec<Uuid>,
        #[serde(default)]
        details: bool,
    },
    Operation {
        family: String,
        operation_id: String,
        #[serde(default)]
        path: Params,
        #[serde(default)]
        query: Params,
        #[serde(default)]
        header: Params,
    },
    Attachments {
        entity: String,
        parent_ids: Vec<Uuid>,
    },
}
fn incremental() -> String {
    "incremental".into()
}
fn window() -> i64 {
    3
}
fn detail_cap() -> usize {
    1000
}

pub fn sync_mode(
    mode: &str,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
) -> Result<xero_sync::SyncMode, Failure> {
    use xero_sync::SyncMode;
    match (mode,from,to,as_of) {
        ("incremental",None,None,None) => Ok(SyncMode::Incremental),
        ("master",None,None,None) => Ok(SyncMode::Master),
        ("backfill",Some(from),Some(to),None) if from <= to && (to-from).num_days() <= 366 => Ok(SyncMode::Backfill {from,to}),
        ("reports",None,None,Some(as_of)) => Ok(SyncMode::Reports {as_of}),
        _ => Err(Failure::invalid("Choose incremental/master without dates, backfill with ordered from/to (up to 366 days), or reports with as_of.")),
    }
}

impl ExtractionPlan {
    pub fn validate(&self) -> Result<(), Failure> {
        match self {
            Self::Accounting {
                entities,
                mode,
                from,
                to,
                as_of,
                window_days,
                max_detail_records,
                ..
            } => {
                if entities.is_empty()
                    || entities.len() > 10
                    || !(1..=366).contains(window_days)
                    || !(1..=10_000).contains(max_detail_records)
                {
                    return Err(Failure::invalid("Select 1..10 distinct entities, window_days 1..366, and max_detail_records 1..10000."));
                }
                let parsed = entities
                    .iter()
                    .map(|e| e.parse::<xero_common::EntityType>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| Failure::invalid("Unknown Accounting entity."))?;
                if entities
                    .iter()
                    .enumerate()
                    .any(|(i, e)| entities[..i].contains(e))
                {
                    return Err(Failure::invalid("Duplicate entities are not allowed."));
                }
                if parsed.iter().any(|e| e.is_report() != (mode == "reports")) {
                    return Err(Failure::invalid("Report entities require reports mode; use only report entities in reports mode."));
                }
                sync_mode(mode, *from, *to, *as_of)?;
            }
            Self::Extended {
                family,
                from,
                to,
                bank_account_ids,
                details,
            } => {
                if family == "identity" {
                    return Err(Failure::invalid("Application-wide Identity extraction is disabled; use the tenant-filtered getConnections query."));
                }
                let family = family
                    .parse()
                    .map_err(|_| Failure::invalid("Unknown extended API family."))?;
                if bank_account_ids.len() > 25 {
                    return Err(Failure::invalid(
                        "At most 25 bank accounts may be selected.",
                    ));
                }
                xero_sync::extended::ExtendedOptions {from:*from,to:*to,bank_account_ids:bank_account_ids.clone(),details:*details}.validate(&[family]).map_err(|_| Failure::invalid("Invalid extended extraction options; Finance requires ordered from/to dates."))?;
                if from.zip(*to).is_some_and(|(a, b)| (b - a).num_days() > 366) {
                    return Err(Failure::invalid(
                        "Finance extraction is limited to 366 days per job.",
                    ));
                }
            }
            Self::Operation {
                family,
                operation_id,
                path,
                query,
                header,
            } => {
                if family != "accounting" {
                    return Err(Failure::invalid(
                        "Use extended extraction for extended warehouse datasets.",
                    ));
                }
                catalog::operation(family, operation_id)?.prepare(path, query, header)?;
            }
            Self::Attachments { entity, parent_ids } => {
                let entity = entity
                    .parse()
                    .map_err(|_| Failure::invalid("Unknown attachment parent type."))?;
                if !xero_client::attachments::supports_attachments(&entity)
                    || parent_ids.is_empty()
                    || parent_ids.len() > 100
                {
                    return Err(Failure::invalid(
                        "Select a supported attachment parent type and 1..100 parent UUIDs.",
                    ));
                }
                if parent_ids
                    .iter()
                    .enumerate()
                    .any(|(i, p)| parent_ids[..i].contains(p))
                {
                    return Err(Failure::invalid(
                        "Duplicate attachment parents are not allowed.",
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn sensitive(&self) -> Result<bool, Failure> {
        Ok(match self {
            Self::Extended { family, .. } => {
                matches!(family.as_str(), "bankfeeds" | "finance" | "identity")
            }
            Self::Operation {
                family,
                operation_id,
                ..
            } => catalog::operation(family, operation_id)?.sensitive(),
            _ => false,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractArgs {
    pub tenant_id: String,
    pub destination: Destination,
    pub idempotency_key: String,
    pub plan: ExtractionPlan,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Job {
    pub job_id: Uuid,
    pub client_id: String,
    pub tenant_id: String,
    pub status: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub destination: Destination,
    pub sensitive: bool,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub idempotency_digest: String,
    pub plan_digest: String,
}
struct ActiveJob {
    job: Job,
    cancellation: CancellationToken,
}

pub struct JobRegistry {
    entries: Mutex<HashMap<Uuid, ActiveJob>>,
    slots: Arc<Semaphore>,
    root: PathBuf,
}
impl JobRegistry {
    pub fn new(root: PathBuf) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(2)),
            root,
        }
    }

    pub async fn start(
        self: &Arc<Self>,
        backend: Arc<dyn Backend>,
        scope: &AccessScope,
        args: ExtractArgs,
    ) -> Result<Job, Failure> {
        args.plan.validate()?;
        scope.authorize(&args.tenant_id, args.plan.sensitive()?, true)?;
        if args.idempotency_key.is_empty()
            || args.idempotency_key.len() > 128
            || !args
                .idempotency_key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        {
            return Err(Failure::invalid(
                "idempotency_key must contain 1..128 letters, digits, '.', ':', '-' or '_'.",
            ));
        }
        let digest = hash(
            format!(
                "{}:{}:{}",
                scope.client_id, args.tenant_id, args.idempotency_key
            )
            .as_bytes(),
        );
        let plan_digest = hash(
            &serde_json::to_vec(&(args.destination, &args.plan))
                .map_err(|_| Failure::invalid("Cannot encode extraction plan."))?,
        );
        let mut entries = self.entries.lock().await;
        // Reload persisted receipts before accepting a retry after restart.
        let idempotency_path = self.root.join("jobs").join(format!("{digest}.json"));
        let in_memory = entries
            .values()
            .find(|e| e.job.idempotency_digest == digest)
            .map(|e| e.job.clone());
        let prior = match in_memory {
            Some(job) => Some(job),
            None => match std::fs::read(&idempotency_path) {
                Ok(bytes) => {
                    Some(serde_json::from_slice::<Job>(&bytes).map_err(|_| storage_error())?)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(storage_error()),
            },
        };
        let recoverable = if let Some(mut prior) = prior {
            if prior.idempotency_digest != digest
                || prior.client_id != scope.client_id
                || prior.tenant_id != args.tenant_id
            {
                return Err(storage_error());
            }
            if prior.plan_digest != plan_digest {
                return Err(Failure::new(
                    "idempotency_conflict",
                    "This idempotency key was already used with a different extraction plan.",
                    false,
                ));
            }
            if entries.contains_key(&prior.job_id) {
                return Ok(prior);
            }
            if prior.status == "queued" {
                Some(prior)
            } else {
                if prior.status == "running" {
                    prior.status = "interrupted".into();
                    prior.error = Some(
                        json!({"code":"server_restarted","message":"The server restarted before this job's completion; inspect the raw manifest before starting a new job."}),
                    );
                    prior.finished_at = Some(Utc::now().to_rfc3339());
                    self.persist(&prior)?;
                }
                return Ok(prior);
            }
        } else {
            None
        };
        if entries.len() >= 128 {
            entries.retain(|_, entry| matches!(entry.job.status.as_str(), "queued" | "running"));
        }
        let permit = self.slots.clone().try_acquire_owned().map_err(|_| Failure::new("extraction_busy","Two extraction jobs are already active; retry later with the same idempotency key.",true))?;
        let job = recoverable.unwrap_or(Job {
            job_id: Uuid::new_v4(),
            client_id: scope.client_id.clone(),
            tenant_id: args.tenant_id.clone(),
            status: "queued".into(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
            destination: args.destination,
            sensitive: args.plan.sensitive()?,
            result: None,
            error: None,
            idempotency_digest: digest,
            plan_digest,
        });
        let id = job.job_id;
        self.persist(&job)?;
        let cancellation = CancellationToken::new();
        entries.insert(
            id,
            ActiveJob {
                job: job.clone(),
                cancellation: cancellation.clone(),
            },
        );
        let registry = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if !registry.update(id, "running", None, None).await {
                return;
            }
            let result = tokio::select! {
                _=cancellation.cancelled()=>Err(Failure::new("job_cancelled","Extraction cancelled; raw objects already written may remain and must be reconciled before reuse.",false)),
                result=backend.extract(id,&args.tenant_id,&args.plan,args.destination,&registry.root)=>result,
            };
            let (status, data, error) = match result {
                Ok(data) => (
                    if data["complete"] == true {
                        "completed"
                    } else {
                        "failed"
                    },
                    Some(data),
                    None,
                ),
                Err(e) => (
                    if e.code == "job_cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    },
                    None,
                    Some(json!(e)),
                ),
            };
            registry.update(id, status, data, error).await;
        });
        Ok(job)
    }

    async fn update(
        &self,
        id: Uuid,
        status: &str,
        result: Option<Value>,
        error: Option<Value>,
    ) -> bool {
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.get_mut(&id) {
            entry.job = Job {
                status: status.into(),
                result,
                error,
                finished_at: (!matches!(status, "queued" | "running"))
                    .then(|| Utc::now().to_rfc3339()),
                ..entry.job.clone()
            };
            if self.persist(&entry.job).is_err() {
                entry.job = Job {
                    status: "receipt_unavailable".into(),
                    error: Some(json!(Failure::new("job_receipt_unavailable", "The latest job state could not be persisted; reconcile the private raw manifest before retrying.", false))),
                    finished_at: Some(Utc::now().to_rfc3339()),
                    ..entry.job.clone()
                };
                tracing::error!(job_id=%id,"Could not persist MCP job receipt");
                return false;
            }
            return true;
        }
        false
    }
    fn persist(&self, job: &Job) -> Result<(), Failure> {
        private_dir(&self.root).map_err(|_| storage_error())?;
        let dir = self.root.join("jobs");
        private_dir(&dir).map_err(|_| {
            Failure::new(
                "job_storage_unavailable",
                "Cannot persist the extraction receipt.",
                true,
            )
        })?;
        let path = dir.join(format!("{}.json", job.idempotency_digest));
        let bytes =
            serde_json::to_vec(job).map_err(|_| Failure::invalid("Cannot encode job receipt."))?;
        let index = dir.join("by-id");
        private_dir(&index).map_err(|_| storage_error())?;
        let reference = index.join(format!("{}.ref", job.job_id));
        match std::fs::read_to_string(&reference) {
            Ok(digest) if digest == job.idempotency_digest => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                atomic_private_write(&reference, job.idempotency_digest.as_bytes())
                    .map_err(|_| storage_error())?
            }
            _ => return Err(storage_error()),
        }
        // The receipt is the commit record; an orphan index cannot start a job.
        atomic_private_write(&path, &bytes).map_err(|_| storage_error())
    }
    pub async fn get(&self, scope: &AccessScope, id: Uuid, cancel: bool) -> Result<Job, Failure> {
        let entries = self.entries.lock().await;
        if let Some(entry) = entries.get(&id) {
            self.authorize_job(scope, &entry.job)?;
            if cancel {
                entry.cancellation.cancel();
            }
            return Ok(entry.job.clone());
        }
        // Keep cold receipt reconciliation serialized with same-key recovery in start.
        let dir = self.root.join("jobs");
        let digest = std::fs::read_to_string(dir.join("by-id").join(format!("{id}.ref"))).map_err(
            |error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    job_not_found()
                } else {
                    storage_error()
                }
            },
        )?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(storage_error());
        }
        let bytes =
            std::fs::read(dir.join(format!("{digest}.json"))).map_err(|_| storage_error())?;
        let job: Job = serde_json::from_slice(&bytes).map_err(|_| storage_error())?;
        if job.job_id != id || job.idempotency_digest != digest {
            return Err(storage_error());
        }
        self.authorize_job(scope, &job)?;
        if cancel && job.status == "queued" {
            let cancelled = Job {
                status: "cancelled".into(),
                finished_at: Some(Utc::now().to_rfc3339()),
                error: Some(
                    json!({"code":"job_cancelled","message":"Queued extraction cancelled before execution."}),
                ),
                ..job
            };
            self.persist(&cancelled)?;
            Ok(cancelled)
        } else if job.status == "running" {
            let interrupted = Job {
                status: "interrupted".into(),
                finished_at: Some(Utc::now().to_rfc3339()),
                error: Some(
                    json!({"code":"server_restarted","message":"Inspect the private raw manifest before starting a replacement job."}),
                ),
                ..job
            };
            self.persist(&interrupted)?;
            Ok(interrupted)
        } else {
            Ok(job)
        }
    }
    fn authorize_job(&self, scope: &AccessScope, job: &Job) -> Result<(), Failure> {
        if scope.client_id != job.client_id {
            return Err(Failure::new(
                "job_not_found",
                "This job is unavailable to the current client.",
                false,
            ));
        }
        scope.authorize(&job.tenant_id, job.sensitive, true)
    }
}

fn storage_error() -> Failure {
    Failure::new("job_storage_unavailable", "The extraction receipt is unreadable or invalid; reconcile the private run manifests before retrying.", false)
}
fn job_not_found() -> Failure {
    Failure::new(
        "job_not_found",
        "This job is unavailable to the current client.",
        false,
    )
}

fn atomic_private_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    private_write(&temporary, bytes)?;
    std::fs::rename(temporary, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    let created = path
        .ancestors()
        .take_while(|directory| !directory.as_os_str().is_empty() && !directory.exists())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        for directory in created.iter().rev() {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
            std::fs::File::open(directory)?.sync_all()?;
            if let Some(parent) = directory
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::File::open(parent)?.sync_all()?;
            }
        }
    }
    Ok(())
}
fn private_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
