//! Editing the ISSI allowlist (`[allowlist]`) from the dashboard: which ISSIs
//! are admitted to the network, and whether that is enforced. The checks
//! themselves live in `AppState::is_blocked`, shared with the blacklist, so
//! every place that bars a blacklisted ISSI bars an unlisted one too.

use crate::state::AppState;
use std::sync::Arc;

pub enum ApplyError {
    Invalid,
    Save(anyhow::Error),
}

/// Adds or removes an ISSI on the running server at once and writes the config
/// file without a restart (a restart would drop every call). `by` names who
/// asked, for the log.
pub async fn apply_issi(state: &Arc<AppState>, issi: u32, allowed: bool, by: &str) -> Result<(), ApplyError> {
    if issi == 0 || issi > 0xFF_FFFF {
        return Err(ApplyError::Invalid);
    }
    state.set_allowed(issi, allowed);
    tracing::warn!(issi, allowed, by, "ISSI allowlist changed");
    save(state).await
}

/// Turns enforcement on or off at once and writes the config file.
pub async fn apply_enabled(state: &Arc<AppState>, enabled: bool, by: &str) -> Result<(), ApplyError> {
    state.set_allowlist_enabled(enabled);
    let (_, issis) = state.allowlist_snapshot();
    if enabled && issis.is_empty() {
        tracing::warn!(by, "ISSI allowlist enforced with an empty list: every radio is barred until one is added");
    } else {
        tracing::warn!(enabled, by, issis = issis.len(), "ISSI allowlist enforcement changed");
    }
    save(state).await
}

async fn save(state: &Arc<AppState>) -> Result<(), ApplyError> {
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = crate::dashboard::save_config(state, &cfg).await;
    if saved.is_err() {
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    saved.map_err(ApplyError::Save)
}

/// One line at startup saying what the allowlist does on this server, so a
/// closed network -- or an accidentally empty one -- is never a silent surprise.
pub fn log_startup(state: &AppState) {
    let (enabled, issis) = state.allowlist_snapshot();
    match (enabled, issis.len()) {
        (false, 0) => {}
        (false, n) => tracing::info!(issis = n, "ISSI allowlist configured but not enforced ([allowlist] enabled = false)"),
        (true, 0) => tracing::warn!("ISSI allowlist enforced with an EMPTY list: every radio is barred until one is added"),
        (true, n) => tracing::info!(issis = n, kick_unknown = state.config.allowlist.kick_unknown, "ISSI allowlist enforced: only listed ISSIs may communicate"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(enabled: bool, issis: &[u32]) -> (Arc<AppState>, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("brew-al-{}.toml", uuid::Uuid::new_v4()));
        let mut c = crate::config::Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.allowlist.enabled = enabled;
        c.allowlist.issis = issis.to_vec();
        (Arc::new(AppState::new(c, path.clone()).0), path)
    }

    #[test]
    fn a_disabled_allowlist_bars_nobody_and_an_enforced_one_bars_the_unlisted() {
        let (state, path) = state_with(false, &[4013]);
        assert!(!state.is_blocked(4013));
        assert!(!state.is_blocked(9999));
        assert_eq!(state.bar_reason(9999), None);

        state.set_allowlist_enabled(true);
        assert!(!state.is_blocked(4013));
        assert!(state.is_blocked(9999));
        assert_eq!(state.bar_reason(9999), Some("not on the allowlist"));
        assert!(state.allowlist_refuses(9999));

        // The blacklist still applies to a listed ISSI, and is named first.
        state.set_blocked(4013, true);
        assert!(state.is_blocked(4013));
        assert_eq!(state.bar_reason(4013), Some("blacklisted"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn edits_apply_live_and_are_written_to_the_config_without_losing_the_blacklist() {
        let (state, path) = state_with(true, &[]);
        assert!(state.is_blocked(4013), "an enforced empty list bars everyone");
        state.set_blocked(77, true);

        apply_issi(&state, 4013, true, "test").await.ok().expect("saved");
        assert!(!state.is_blocked(4013));
        let saved = crate::config::Config::load(path.to_str().unwrap()).unwrap();
        assert!(saved.allowlist.enabled);
        assert_eq!(saved.allowlist.issis, vec![4013]);
        assert_eq!(saved.blacklist.issis, vec![77], "the live blacklist is written too, not reverted");

        apply_enabled(&state, false, "test").await.ok().expect("saved");
        assert!(!state.is_blocked(9999));
        let saved = crate::config::Config::load(path.to_str().unwrap()).unwrap();
        assert!(!saved.allowlist.enabled);
        assert_eq!(saved.allowlist.issis, vec![4013], "turning enforcement off keeps the list");

        apply_issi(&state, 4013, false, "test").await.ok().expect("saved");
        assert!(crate::config::Config::load(path.to_str().unwrap()).unwrap().allowlist.issis.is_empty());
        assert!(matches!(apply_issi(&state, 0, true, "test").await, Err(ApplyError::Invalid)));
        assert!(matches!(apply_issi(&state, 0x100_0000, true, "test").await, Err(ApplyError::Invalid)));
        let _ = std::fs::remove_file(path);
    }
}
