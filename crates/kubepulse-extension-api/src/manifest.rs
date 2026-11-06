//! What an extension declares. The host reads this without running any of the
//! extension's code.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `major` changes break extensions. `minor` changes only add to the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractVersion {
    pub major: u32,
    pub minor: u32,
}

impl ContractVersion {
    /// Whether a host providing `self` can run an extension built for `required`.
    pub fn supports(self, required: ContractVersion) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl std::fmt::Display for ContractVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A resource kind. An empty `group` is the core API group.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KindRef {
    pub group: String,
    pub kind: String,
}

impl KindRef {
    pub fn new(group: &str, kind: &str) -> Self {
        Self {
            group: group.to_owned(),
            kind: kind.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verb {
    Get,
    List,
    Create,
    Update,
    Delete,
    Logs,
    Exec,
    PortForward,
}

/// Verbs an extension may use on one kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permission {
    pub kind: KindRef,
    pub verbs: Vec<Verb>,
}

/// What wakes an inactive extension.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "kebab-case")]
pub enum ActivationTrigger {
    Startup,
    /// The user opens this kind.
    Kind {
        kind: KindRef,
    },
    /// The user runs this command.
    Command {
        command: String,
    },
}

/// An icon from the set the host defines. Extensions name one and never supply
/// image data. A name this host does not know reads as `Generic`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewIcon {
    Box,
    Layers,
    Copy,
    Database,
    Server,
    Timer,
    Schedule,
    Network,
    Globe,
    Shield,
    Waypoints,
    SettingsFile,
    Key,
    Disk,
    StorageClass,
    Account,
    Role,
    Link,
    // Serde requires the catch-all to come last.
    #[default]
    #[serde(other)]
    Generic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceViewContribution {
    pub id: String,
    pub title: String,
    pub kind: KindRef,
    /// The navigation group, such as `Workloads`.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: ViewIcon,
}

/// Where a column's value comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "kebab-case")]
pub enum ColumnSource {
    /// A path into the object, such as `.status.phase`. The host evaluates it
    /// without calling the extension, so it cannot affect table scale.
    Path { path: String },
    /// Requested from the extension for visible rows only, with
    /// [`ExtensionRequest::ComputeColumn`](crate::ExtensionRequest::ComputeColumn).
    Computed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnContribution {
    pub id: String,
    pub title: String,
    pub kind: KindRef,
    pub source: ColumnSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailTabContribution {
    pub id: String,
    pub title: String,
    pub kind: KindRef,
}

/// Runs on one object, from its table row or detail view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionContribution {
    pub id: String,
    pub title: String,
    pub kind: KindRef,
}

/// Appears in the command palette.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
}

/// An entry in the navigation sidebar, opening one of the extension's views.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationContribution {
    pub id: String,
    pub title: String,
    pub view: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SettingKind {
    Toggle {
        default: bool,
    },
    Text {
        default: String,
    },
    Choice {
        options: Vec<String>,
        default: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingContribution {
    pub id: String,
    pub title: String,
    pub kind: SettingKind,
}

/// Everything an extension adds. Each list defaults to empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Contributions {
    pub resource_views: Vec<ResourceViewContribution>,
    pub columns: Vec<ColumnContribution>,
    pub detail_tabs: Vec<DetailTabContribution>,
    pub actions: Vec<ActionContribution>,
    pub commands: Vec<CommandContribution>,
    pub navigation: Vec<NavigationContribution>,
    pub settings: Vec<SettingContribution>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique across extensions, such as `example.pods`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The contract version this extension was built for.
    pub contract: ContractVersion,
    #[serde(default)]
    pub activation: Vec<ActivationTrigger>,
    #[serde(default)]
    pub contributions: Contributions,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Language tag to declared text to translation. The host uses these for
    /// contribution titles and categories.
    #[serde(default)]
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
}

/// The best of `available` for a language `tag`: the same tag, then the bare
/// primary language, then any variant of it. Tags compare without case.
pub fn best_language<'a>(
    tag: &str,
    available: impl Iterator<Item = &'a str> + Clone,
) -> Option<&'a str> {
    let tag = tag.replace('_', "-");
    let primary = |tag: &'a str| tag.split('-').next().unwrap_or(tag);
    let wanted = tag.split('-').next().unwrap_or(&tag);
    let find = |matches: &dyn Fn(&'a str) -> bool| available.clone().find(|c| matches(c));
    find(&|candidate| candidate.eq_ignore_ascii_case(&tag))
        .or_else(|| find(&|candidate| candidate.eq_ignore_ascii_case(wanted)))
        .or_else(|| find(&|candidate| primary(candidate).eq_ignore_ascii_case(wanted)))
}

#[cfg(test)]
mod language_tests {
    use super::best_language;

    fn best<'a>(tag: &str, available: &[&'a str]) -> Option<&'a str> {
        best_language(tag, available.iter().copied())
    }

    #[test]
    fn the_same_tag_wins() {
        assert_eq!(best("pt-BR", &["pt", "pt-BR"]), Some("pt-BR"));
        assert_eq!(best("pt_br", &["pt-BR"]), Some("pt-BR"));
    }

    #[test]
    fn a_regional_variant_uses_the_primary_language() {
        assert_eq!(best("de-AT", &["en", "de"]), Some("de"));
    }

    #[test]
    fn another_variant_of_the_language_is_better_than_nothing() {
        assert_eq!(best("pt-PT", &["en", "pt-BR"]), Some("pt-BR"));
    }

    #[test]
    fn an_unrelated_language_does_not_match() {
        assert_eq!(best("fr", &["en", "de"]), None);
    }
}
