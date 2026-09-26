mod aprs;
mod config;
mod control;
mod dashboard;
mod federation;
mod fsnet;
mod monitor;
mod position;
mod protocol;
mod router;
mod server;
mod sip;
mod state;
mod store;
mod telemetry;
mod transcode;

use config::Config;
use state::AppState;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "brew_server=info".into()))
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "brew-server.toml".to_owned());
    let config = Config::load(&path)?;
    let (state, aprs_rx) = AppState::new(config, std::path::PathBuf::from(&path));
    let state = Arc::new(state);

    // Watch the config file; when it changes, restart the whole process so the
    // new configuration takes effect from a clean state.
    tokio::spawn(config_watcher(path.clone()));

    if state.config.max_call_duration_seconds > 0 || state.config.call_inactivity_timeout_seconds > 0 {
        tokio::spawn(router::run_call_duration_sweep(state.clone()));
    }

    tokio::spawn(federation::run(state.clone()));
    tokio::spawn(aprs::run(state.clone(), aprs_rx));

    tokio::try_join!(
        server::run(state.clone()),
        telemetry::run(state.clone()),
        control::run(state.clone()),
        dashboard::run(state.clone()),
        sip::run(state.clone()),
    )?;
    Ok(())
}

/// Polls the config file's modification time; on any change, restarts the
/// process by re-executing the same binary with the same arguments. This is a
/// deliberately blunt "drop everything and start again" reload: the OS reclaims
/// all sockets and in-memory state is rebuilt from the freshly parsed config.
async fn config_watcher(path: String) {
    let mtime = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut last_mtime: Option<SystemTime> = mtime(&path);
    let mut ticker = tokio::time::interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;
        let current = mtime(&path);
        if current == last_mtime {
            continue;
        }
        last_mtime = current;

        // Validate the new file first: a broken edit should not restart into a
        // crash loop. If it doesn't parse, keep running and wait for a fix.
        match Config::load(&path) {
            Ok(_) => {
                tracing::warn!(config = %path, "configuration changed - restarting process");
                restart_process();
            }
            Err(e) => {
                tracing::error!(config = %path, error = %e, "config changed but failed to parse; not restarting");
            }
        }
    }
}

/// Re-executes the current binary with the original arguments, replacing this
/// process. On success this never returns; on failure we log and exit non-zero
/// so a process supervisor (systemd, Docker restart policy) can bring us back.
fn restart_process() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "cannot determine current executable; exiting for supervisor restart");
            std::process::exit(1);
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Give the log line a moment to flush before we replace the image.
    std::thread::sleep(Duration::from_millis(100));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&exe).args(&args).exec();
        // exec only returns on failure.
        tracing::error!(error = %err, "exec restart failed; exiting for supervisor restart");
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        match std::process::Command::new(&exe).args(&args).spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                tracing::error!(error = %e, "spawn restart failed; exiting for supervisor restart");
                std::process::exit(1);
            }
        }
    }
}
