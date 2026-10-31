use clap::{Parser, ValueEnum};
use std::sync::Arc;
use xero_mcp::{
    backend::LiveBackend,
    config::Settings,
    oauth::{OAuthConfig, OAuthState},
    oauth_store::OAuthStore,
    server::XeroMcp,
    writes::WriteStore,
};

#[derive(Clone, Copy, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[derive(Parser)]
#[command(
    version,
    about = "Xero MCP tools, resources, prompts and extraction jobs"
)]
struct Args {
    #[arg(long, value_enum, default_value = "stdio")]
    transport: Transport,
    /// Explicit environment file; missing or malformed files fail startup.
    #[arg(long)]
    env_file: Option<std::path::PathBuf>,
    /// Print the capability manifest without opening a listener or authenticating.
    #[arg(long)]
    print_manifest: bool,
    /// Approve a prepared write from this host after reviewing its MCP preview.
    #[arg(long, conflicts_with = "inspect_write")]
    approve_write: Option<uuid::Uuid>,
    /// Print the exact pending request from the private approval database to this terminal.
    #[arg(long, conflicts_with = "approve_write")]
    inspect_write: Option<uuid::Uuid>,
    #[arg(long, requires = "approve_write")]
    confirm_client: Option<String>,
    #[arg(long, requires = "approve_write")]
    confirm_tenant: Option<String>,
    #[arg(long, requires = "approve_write")]
    confirm_action: Option<String>,
    #[arg(long, requires = "approve_write")]
    confirm_body_sha256: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.print_manifest {
        println!(
            "{}",
            serde_json::to_string_pretty(&xero_mcp::catalog::manifest())?
        );
        return Ok(());
    }
    if let Some(path) = args.env_file {
        dotenvy::from_path(&path)
            .map_err(|_| anyhow::anyhow!("MCP environment file is missing or invalid"))?;
    }
    if let Some(id) = args.inspect_write {
        let root = std::env::var("XERO_MCP_STORAGE_ROOT").unwrap_or_else(|_| "./out/mcp".into());
        let preview =
            WriteStore::open(std::path::Path::new(&root).join("writes.sqlite"))?.inspect(id)?;
        println!("{}", serde_json::to_string_pretty(&preview)?);
        return Ok(());
    }
    if let Some(id) = args.approve_write {
        let tenant = args
            .confirm_tenant
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--confirm-tenant is required"))?;
        let action = args
            .confirm_action
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--confirm-action is required"))?;
        let client = args
            .confirm_client
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--confirm-client is required"))?;
        let body_sha256 = args
            .confirm_body_sha256
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--confirm-body-sha256 is required"))?;
        let root = std::env::var("XERO_MCP_STORAGE_ROOT").unwrap_or_else(|_| "./out/mcp".into());
        WriteStore::open(std::path::Path::new(&root).join("writes.sqlite"))?.approve(
            id,
            client,
            tenant,
            action,
            body_sha256,
        )?;
        println!("write approval recorded for {id}");
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let settings = Settings::from_env()?;
    if matches!(args.transport, Transport::Stdio) {
        anyhow::ensure!(
            !settings.local_scope.tenant_ids.is_empty(),
            "stdio MCP requires XERO_MCP_TENANTS"
        );
    }
    let oauth_config = if matches!(args.transport, Transport::Http) {
        OAuthConfig::from_env()?
    } else {
        None
    };
    let shared_store = if oauth_config.is_some() {
        Some(xero_store::open_from_env().await?)
    } else {
        None
    };
    let backend = Arc::new(LiveBackend::from_env_with_store(shared_store.clone()).await?);
    let mut server = XeroMcp::new(
        backend,
        settings.local_scope.clone(),
        settings.storage_root.clone(),
    );
    server.compact = settings.compact;
    if settings.writes_enabled {
        server.enable_writes(&settings.storage_root)?;
    }
    match args.transport {
        Transport::Stdio => xero_mcp::transport::stdio(server).await,
        Transport::Http => {
            let oauth = match (oauth_config, shared_store) {
                (Some(config), Some(store)) => Some(OAuthState::with_store(
                    config,
                    OAuthStore::open(settings.storage_root.join("oauth.sqlite"))?,
                    store,
                )),
                _ => None,
            };
            xero_mcp::transport::http_with_oauth(server, settings, oauth).await
        }
    }
}
