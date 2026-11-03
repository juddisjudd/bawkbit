use anyhow::Context as _;
use bawkbit_core::Core;
use bawkbit_core::settings::default_download_dir;
use bawkbit_web::{Status, generate_password, hash_password};

const HELP: &str = "bawkbitd runs bawkbit without a window: the torrent engine and the web UI.

usage: bawkbitd [--new-password]

  --new-password   print a new web UI password and keep running

Settings and the session live in BAWKBIT_HOME (default: your config folder/bawkbit).
BAWKBIT_DOWNLOAD_DIR sets the download folder on first start.
BAWKBIT_PORT sets the port peers connect to, on every start.
RUST_LOG sets how much is logged, for example RUST_LOG=debug.";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{HELP}");
        return Ok(());
    }
    if let Some(unknown) = args.iter().find(|arg| *arg != "--new-password") {
        anyhow::bail!("unknown option {unknown}\n\n{HELP}");
    }
    let new_password = args.iter().any(|arg| arg == "--new-password");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "bawkbitd=info,bawkbit_web=info,bawkbit_core=info,librqbit=warn".into()
            }),
        )
        .init();

    let home = bawkbit_core::data_dir();
    let core = Core::start(home.clone())
        .await
        .context("cannot start bawkbit")?;
    tracing::info!("bawkbit {} using {}", bawkbit_core::VERSION, home.display());

    let mut settings = core.settings();
    let mut changed = false;
    if !settings.web.enabled {
        settings.web.enabled = true;
        changed = true;
    }
    if let Some(port) = std::env::var("BAWKBIT_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        && settings.listen_port != port
    {
        settings.listen_port = port;
        changed = true;
    }
    if let Some(dir) = std::env::var_os("BAWKBIT_DOWNLOAD_DIR")
        && settings.download_dir == default_download_dir().to_string_lossy()
    {
        settings.download_dir = dir.to_string_lossy().into_owned();
        changed = true;
    }
    if changed {
        settings = core.update_settings(settings).await?;
    }
    if new_password || core.settings().web.password_hash.is_empty() {
        let password = generate_password();
        core.set_web_password_hash(hash_password(&password)?)?;
        println!(
            "web UI login: {} / {password}\nchange it under settings in the web UI.",
            settings.web.username
        );
    }

    let web = bawkbit_web::spawn(core.clone());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    if let Status::Failed(err) = web.status() {
        tracing::error!("{err}");
    }

    wait_for_stop().await;
    tracing::info!("stopping");
    core.shutdown().await;
    Ok(())
}

#[cfg(unix)]
async fn wait_for_stop() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("can listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_stop() {
    let _ = tokio::signal::ctrl_c().await;
}
