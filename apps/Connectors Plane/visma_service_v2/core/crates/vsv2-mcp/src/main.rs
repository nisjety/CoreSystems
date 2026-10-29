use anyhow::Context;
use rmcp::{transport::stdio, ServiceExt};
use vsv2_mcp::{tools, Catalog, McpConfig, McpServer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout carries the protocol; every log line must go to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // `vsv2-mcp login | logout | whoami` manage the sign-in; they do not start the server.
    let mut raw = std::env::args().skip(1);
    if let Some(first) = raw.next() {
        if vsv2_mcp::signin_cli::COMMANDS.contains(&first.as_str()) {
            return vsv2_mcp::signin_cli::run(&first, raw.collect()).await;
        }
    }
    let print_manifest = parse_args(std::env::args().skip(1))?;
    // Embedded data is validated before anything is served: a broken skill or known-issue file
    // must stop startup, not surprise the first client that asks for it.
    tracing::info!(
        skills = vsv2_mcp::skills::SkillIndex::embedded().all().len(),
        known_issues = vsv2_mcp::known_issues::all().len(),
        "embedded guidance loaded"
    );

    let cfg = McpConfig::from_env().context("invalid configuration")?;
    let catalog = Catalog::load(&cfg.endpoint_dir).with_context(|| {
        format!(
            "loading endpoint configs from {}",
            cfg.endpoint_dir.display()
        )
    })?;
    let (registry, status) = tools::build_registry(&cfg, catalog).await;
    for n in &status.notes {
        tracing::warn!("{n}");
    }
    if print_manifest {
        println!(
            "{}",
            serde_json::to_string_pretty(&vsv2_mcp::manifest::manifest(&registry, &status))?
        );
        return Ok(());
    }
    tracing::info!(tools = ?registry.names(), "vsv2-mcp starting");

    let server = McpServer::new(registry);
    let service = server
        .serve(stdio())
        .await
        .context("starting MCP stdio service")?;
    service.waiting().await?;
    Ok(())
}

const USAGE: &str = "usage: vsv2-mcp [--print-manifest]\n       vsv2-mcp login | logout | whoami   (see `vsv2-mcp login --help`)\n\n\
    Without arguments: serve MCP over stdio (configured from the environment and .env).\n\
    --print-manifest: print what THIS instance, with its current configuration, would expose (tools with \
    schemas and skill lists, the prompts it offers, the skills, capability status) as JSON and exit. A tool \
    whose credentials or settings are missing is absent and named in `status`; run it with the production \
    environment to see the full surface.\n\n\
    login signs you in to Visma, so the server acts as you instead of with the integration's service \
    credentials; logout undoes it; whoami shows what is connected.";

/// `Ok(true)` for `--print-manifest`, `Ok(false)` for none. Anything else is an error: a mistyped
/// flag must not silently start a server that waits on stdin.
fn parse_args(args: impl Iterator<Item = String>) -> anyhow::Result<bool> {
    let mut print_manifest = false;
    for a in args {
        match a.as_str() {
            "--print-manifest" => print_manifest = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    Ok(print_manifest)
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn only_the_documented_flag_is_accepted() {
        assert!(!parse_args(args(&[])).unwrap());
        assert!(parse_args(args(&["--print-manifest"])).unwrap());
        let e = parse_args(args(&["--print-manifst"]))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("unknown argument") && e.contains("usage: vsv2-mcp"),
            "{e}"
        );
    }
}
