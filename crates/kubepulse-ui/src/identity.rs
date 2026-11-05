//! How a cluster appears to the user. Settings are keyed by context name and
//! server URL, so a reused name on another server inherits nothing, including
//! a protected flag.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Colors that stay distinct from each other and readable in both themes.
pub const PALETTE: [u32; 8] = [
    0x2563eb, 0x059669, 0xd97706, 0xdc2626, 0x7c3aed, 0x0891b2, 0xdb2777, 0x65a30d,
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClusterKey {
    pub context: String,
    pub server: String,
}

impl ClusterKey {
    pub fn new(context: &str, server: Option<&str>) -> Self {
        Self {
            context: context.to_owned(),
            server: server.unwrap_or_default().to_owned(),
        }
    }

    /// FNV-1a, because the default hasher may change between Rust releases and
    /// a cluster's color must not.
    fn stable_hash(&self) -> u64 {
        let bytes = self.context.bytes().chain([0]).chain(self.server.bytes());
        bytes.fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }
}

/// What the user has chosen. Empty fields use the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterSettings {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterIdentity {
    pub name: String,
    pub color: u32,
    pub protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredCluster {
    key: ClusterKey,
    settings: ClusterSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentityStore(BTreeMap<ClusterKey, ClusterSettings>);

impl IdentityStore {
    pub fn identity(&self, key: &ClusterKey) -> ClusterIdentity {
        let settings = self.0.get(key).cloned().unwrap_or_default();
        let index = (key.stable_hash() % PALETTE.len() as u64) as usize;
        ClusterIdentity {
            name: settings.name.unwrap_or_else(|| key.context.clone()),
            color: settings.color.unwrap_or(PALETTE[index]),
            protected: settings.protected,
        }
    }

    pub fn settings(&self, key: &ClusterKey) -> ClusterSettings {
        self.0.get(key).cloned().unwrap_or_default()
    }

    pub fn update(&mut self, key: ClusterKey, change: impl FnOnce(&mut ClusterSettings)) {
        let settings = self.0.entry(key.clone()).or_default();
        change(settings);
        if *settings == ClusterSettings::default() {
            self.0.remove(&key);
        }
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        let stored: Vec<StoredCluster> = self
            .0
            .iter()
            .map(|(key, settings)| StoredCluster {
                key: key.clone(),
                settings: settings.clone(),
            })
            .collect();
        serde_json::to_string_pretty(&stored)
    }

    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        let stored: Vec<StoredCluster> = serde_json::from_str(text)?;
        Ok(Self(
            stored.into_iter().map(|c| (c.key, c.settings)).collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(context: &str, server: &str) -> ClusterKey {
        ClusterKey::new(context, Some(server))
    }

    #[test]
    fn defaults_use_the_context_name_and_a_stable_color() {
        let identity = IdentityStore::default().identity(&key("prod", "https://a"));
        assert_eq!(identity.name, "prod");
        assert_eq!(
            identity.color, 0xdb2777,
            "colors must not change between releases"
        );
        assert!(!identity.protected);
    }

    #[test]
    fn a_reused_context_name_on_another_server_inherits_nothing() {
        let mut store = IdentityStore::default();
        store.update(key("prod", "https://real"), |settings| {
            settings.name = Some("Production".into());
            settings.color = Some(0xff0000);
            settings.protected = true;
        });

        let same = store.identity(&key("prod", "https://real"));
        assert_eq!((same.name.as_str(), same.protected), ("Production", true));

        let other = store.identity(&key("prod", "https://other"));
        assert_eq!(other.name, "prod");
        assert_ne!(other.color, 0xff0000);
        assert!(!other.protected);
    }

    #[test]
    fn settings_survive_saving_and_loading() {
        let mut store = IdentityStore::default();
        store.update(key("prod", "https://a"), |settings| {
            settings.protected = true
        });
        let loaded = IdentityStore::from_json(&store.to_json().unwrap()).unwrap();
        assert_eq!(loaded, store);
    }

    #[test]
    fn clearing_every_setting_removes_the_entry() {
        let mut store = IdentityStore::default();
        store.update(key("a", "b"), |settings| settings.protected = true);
        store.update(key("a", "b"), |settings| settings.protected = false);
        assert_eq!(store, IdentityStore::default());
    }
}
