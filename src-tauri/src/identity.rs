/// Watches the WoW addon's SavedVariables file and emits PlayerIdentity updates.
///
/// The CombatCoach Lua addon writes a file like:
///   WTF/Account/<ACCOUNT>/SavedVariables/CombatCoach.lua
///
/// Its contents (the SavedVariables table) look like:
///   CombatCoachDB = {
///       ["playerGUID"] = "Player-1234-ABCDEF",
///       ["playerName"] = "Stonebraid",
///       ["realmName"]  = "Stormrage",
///       ["className"]  = "PALADIN",
///       ["specName"]   = "Retribution",
///       ["addonVersion"] = "0.1.0",
///   }
///
/// WoW only writes SavedVariables on logout or /reload, so identity updates
/// are infrequent. The engine falls back to inferring the player GUID from
/// combat log events if the file has not yet been written.
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerIdentity {
    pub guid:    String,
    pub name:    String,
    pub realm:   String,
    pub class:   String,
    pub spec:    String,
    pub version: String,
}

impl PlayerIdentity {
    pub fn unknown() -> Self {
        Self {
            guid:    String::new(),
            name:    "Unknown".into(),
            realm:   String::new(),
            class:   String::new(),
            spec:    String::new(),
            version: String::new(),
        }
    }

    pub fn is_known(&self) -> bool {
        !self.guid.is_empty()
    }
}

/// Extract a string value from a Lua SavedVariables table using simple line scanning.
/// Matches lines like:  ["playerGUID"] = "Player-1234-ABCDEF",
fn extract_lua_string<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("[\"{}\"]", key);
    let line = content.lines().find(|l| l.contains(&needle))?;

    // Find the value portion after the '=' sign
    let eq_pos = line.find('=')?;
    let after_eq = line[eq_pos + 1..].trim();

    // Value is wrapped in double quotes
    if !after_eq.starts_with('"') {
        return None;
    }
    let inner = &after_eq[1..];
    let end = inner.find('"')?;
    Some(&inner[..end])
}

fn parse_saved_variables(content: &str) -> Option<PlayerIdentity> {
    Some(PlayerIdentity {
        guid:    extract_lua_string(content, "playerGUID")?.to_owned(),
        name:    extract_lua_string(content, "playerName")?.to_owned(),
        realm:   extract_lua_string(content, "realmName").unwrap_or("").to_owned(),
        class:   extract_lua_string(content, "className").unwrap_or("").to_owned(),
        spec:    extract_lua_string(content, "specName").unwrap_or("").to_owned(),
        version: extract_lua_string(content, "addonVersion").unwrap_or("").to_owned(),
    })
}

pub async fn run(sv_path: PathBuf, tx: Sender<PlayerIdentity>) -> Result<()> {
    tracing::info!("Identity watcher starting: {:?}", sv_path);

    // Initial parse if file already exists (player was logged in previously)
    if sv_path.exists() {
        let content = std::fs::read_to_string(&sv_path)?;
        if let Some(id) = parse_saved_variables(&content) {
            tracing::info!("Identity loaded: {} ({}/{})", id.name, id.class, id.spec);
            let _ = tx.send(id).await;
        }
    } else {
        tracing::info!("Addon SavedVariables not found yet — waiting for first /reload");
    }

    // Watch the directory (more reliable than watching the file directly)
    let watch_dir = sv_path.parent().unwrap_or(sv_path.as_path()).to_path_buf();
    let (fs_tx, fs_rx) = std_mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(fs_tx, notify::Config::default())?;
    watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    loop {
        match fs_rx.recv() {
            Ok(Ok(Event { kind: EventKind::Modify(_), paths, .. })) => {
                if paths.iter().any(|p| p == &sv_path) {
                    match std::fs::read_to_string(&sv_path) {
                        Ok(content) => {
                            if let Some(id) = parse_saved_variables(&content) {
                                tracing::info!("Identity updated: {} ({}/{})", id.name, id.class, id.spec);
                                if tx.send(id).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(e) => tracing::warn!("Could not read SavedVariables: {}", e),
                    }
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::error!("Identity watcher error: {}", e),
            Err(_) => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
CombatCoachDB = {
    ["playerGUID"] = "Player-1234-ABCDEF",
    ["playerName"] = "Stonebraid",
    ["realmName"] = "Stormrage",
    ["className"] = "PALADIN",
    ["specName"] = "Retribution",
    ["addonVersion"] = "0.1.0",
}
"#;

    #[test]
    fn parses_identity() {
        let id = parse_saved_variables(SAMPLE).expect("should parse");
        assert_eq!(id.guid,  "Player-1234-ABCDEF");
        assert_eq!(id.name,  "Stonebraid");
        assert_eq!(id.realm, "Stormrage");
        assert_eq!(id.class, "PALADIN");
        assert_eq!(id.spec,  "Retribution");
    }

    #[test]
    fn returns_none_for_empty() {
        assert!(parse_saved_variables("").is_none());
    }
}
