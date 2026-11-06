//! The extension registry and its lifecycle.
//!
//! Each call into an extension runs in its own task. A panic stays in that task,
//! and a slow call is abandoned at its time limit, so neither reaches the host
//! or another extension. Real memory isolation waits for a sandboxed loader.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use kubepulse_extension_api::{
    ActivationTrigger, CONTRACT_VERSION, ContractVersion, Contributions, Extension, ExtensionError,
    ExtensionRequest, ExtensionResponse, Host, Manifest, best_language,
};
use tokio::{sync::watch, task::JoinSet, time};

use crate::capability::{Capability, HostBackend, ReviewClock};

#[derive(Clone, Debug)]
pub struct HostOptions {
    pub contract: ContractVersion,
    /// The interface language, as a language tag.
    pub language: String,
    pub call_timeout: Duration,
    pub activation_timeout: Duration,
    /// Consecutive failures before an extension is disabled.
    pub failure_limit: u32,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            contract: CONTRACT_VERSION,
            language: "en".to_owned(),
            call_timeout: Duration::from_secs(5),
            activation_timeout: Duration::from_secs(10),
            failure_limit: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionStatus {
    Inactive,
    Active,
    Disabled,
    /// Turned off after repeated failures. Enabling it again clears this.
    Failed {
        reason: String,
    },
    /// It did not load, and cannot be enabled.
    Refused {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub status: ExtensionStatus,
    pub refused_operations: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    UnknownExtension,
    /// It is disabled, failed, or refused, so it cannot take requests.
    Unavailable,
    /// The call failed. Its contribution shows this message.
    Failed {
        message: String,
    },
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownExtension => write!(f, "no such extension"),
            Self::Unavailable => write!(f, "the extension is not available"),
            Self::Failed { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CallError {}

/// The contributions of every loaded, enabled extension.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributionSet {
    pub version: u64,
    /// Titles and categories are already in the interface language, except
    /// `category`, which stays as declared so that views group by it.
    pub extensions: Vec<(String, Contributions)>,
    /// Display names of declared categories, for those with a translation.
    pub category_titles: BTreeMap<String, String>,
}

fn translations_for<'a>(
    manifest: &'a Manifest,
    language: &str,
) -> Option<&'a BTreeMap<String, String>> {
    let tag = best_language(language, manifest.translations.keys().map(String::as_str))?;
    manifest.translations.get(tag)
}

/// Replaces every title that has a translation. A category stays as declared,
/// so that views from different extensions still group together.
fn localized(
    contributions: &Contributions,
    translations: Option<&BTreeMap<String, String>>,
) -> Contributions {
    let mut localized = contributions.clone();
    let Some(translations) = translations else {
        return localized;
    };
    let titles = localized
        .resource_views
        .iter_mut()
        .map(|view| &mut view.title)
        .chain(localized.columns.iter_mut().map(|column| &mut column.title))
        .chain(localized.detail_tabs.iter_mut().map(|tab| &mut tab.title))
        .chain(localized.actions.iter_mut().map(|action| &mut action.title))
        .chain(
            localized
                .commands
                .iter_mut()
                .map(|command| &mut command.title),
        )
        .chain(
            localized
                .navigation
                .iter_mut()
                .map(|entry| &mut entry.title),
        )
        .chain(
            localized
                .settings
                .iter_mut()
                .map(|setting| &mut setting.title),
        );
    for title in titles {
        if let Some(translation) = translations.get(title.as_str()) {
            title.clone_from(translation);
        }
    }
    localized
}

struct Entry {
    manifest: Manifest,
    extension: Arc<dyn Extension>,
    status: ExtensionStatus,
    failures: u32,
    refused: Arc<AtomicU64>,
    capability: Arc<Capability>,
    reviews: Arc<ReviewClock>,
    /// Held while activating, so concurrent triggers activate once.
    activation: Arc<tokio::sync::Mutex<()>>,
}

struct Inner {
    backend: Arc<dyn HostBackend>,
    options: HostOptions,
    entries: Mutex<Vec<Entry>>,
    contributions: watch::Sender<Arc<ContributionSet>>,
}

#[derive(Clone)]
pub struct ExtensionHost {
    inner: Arc<Inner>,
}

impl ExtensionHost {
    pub fn new(backend: Arc<dyn HostBackend>, options: HostOptions) -> Self {
        let (contributions, _) = watch::channel(Arc::new(ContributionSet::default()));
        let inner = Inner {
            backend,
            options,
            entries: Mutex::default(),
            contributions,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Reads each extension's manifest. No extension code runs beyond that.
    pub fn load(&self, extensions: Vec<Arc<dyn Extension>>) {
        {
            let mut entries = self.entries();
            for extension in extensions {
                let manifest = extension.manifest();
                let status = self.check(&manifest, &entries);
                if let ExtensionStatus::Refused { reason } = &status {
                    tracing::warn!(extension = %manifest.id, %reason, "extension refused");
                }
                let refused = Arc::new(AtomicU64::new(0));
                let reviews = Arc::<ReviewClock>::default();
                let capability = Arc::new(Capability::new(
                    manifest.id.clone(),
                    manifest.permissions.clone(),
                    self.inner.backend.clone(),
                    refused.clone(),
                    self.inner.options.language.clone(),
                    reviews.clone(),
                ));
                entries.push(Entry {
                    manifest,
                    extension,
                    status,
                    failures: 0,
                    refused,
                    capability,
                    reviews,
                    activation: Arc::default(),
                });
            }
        }
        self.publish();
    }

    fn check(&self, manifest: &Manifest, entries: &[Entry]) -> ExtensionStatus {
        let host = self.inner.options.contract;
        if !host.supports(manifest.contract) {
            let reason = format!(
                "built for contract {}, but this application provides {host}",
                manifest.contract
            );
            return ExtensionStatus::Refused { reason };
        }
        let taken = entries.iter().any(|entry| {
            entry.manifest.id == manifest.id
                && !matches!(entry.status, ExtensionStatus::Refused { .. })
        });
        if manifest.id.is_empty() || taken {
            let reason = "its id is empty or already in use".to_owned();
            return ExtensionStatus::Refused { reason };
        }
        ExtensionStatus::Inactive
    }

    pub fn extensions(&self) -> Vec<ExtensionInfo> {
        self.entries()
            .iter()
            .map(|entry| ExtensionInfo {
                id: entry.manifest.id.clone(),
                name: entry.manifest.name.clone(),
                version: entry.manifest.version.clone(),
                status: entry.status.clone(),
                refused_operations: entry.refused.load(Ordering::Relaxed),
            })
            .collect()
    }

    pub fn contributions(&self) -> watch::Receiver<Arc<ContributionSet>> {
        self.inner.contributions.subscribe()
    }

    /// Turns an extension on or off. Its contributions appear or disappear
    /// immediately. Refused extensions are not found here, so they stay refused.
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), CallError> {
        let extension = {
            let mut entries = self.entries();
            let entry = find(&mut entries, id)?;
            let was_active = entry.status == ExtensionStatus::Active;
            entry.failures = 0;
            entry.status = if enabled {
                ExtensionStatus::Inactive
            } else {
                ExtensionStatus::Disabled
            };
            (!enabled && was_active).then(|| entry.extension.clone())
        };
        self.publish();
        if let Some(extension) = extension {
            let limit = self.inner.options.call_timeout;
            let deactivated = run_guarded(limit, &ReviewClock::default(), async move {
                extension.deactivate().await;
                Ok(())
            })
            .await;
            if let Err(message) = deactivated {
                tracing::warn!(extension = %id, %message, "extension failed to deactivate");
            }
        }
        Ok(())
    }

    /// Activates every inactive extension declaring `trigger`. They activate
    /// concurrently, so a slow one delays only itself.
    pub async fn trigger(&self, trigger: &ActivationTrigger) {
        let ids: Vec<String> = self
            .entries()
            .iter()
            .filter(|entry| {
                entry.status == ExtensionStatus::Inactive
                    && entry.manifest.activation.contains(trigger)
            })
            .map(|entry| entry.manifest.id.clone())
            .collect();
        let mut activations = JoinSet::new();
        for id in ids {
            let host = self.clone();
            activations.spawn(async move { host.activate(&id).await });
        }
        activations.join_all().await;
    }

    async fn activate(&self, id: &str) -> Result<(), CallError> {
        let lock = {
            let mut entries = self.entries();
            find(&mut entries, id)?.activation.clone()
        };
        let _guard = lock.lock().await;
        let (extension, capability, reviews) = {
            let mut entries = self.entries();
            let entry = find(&mut entries, id)?;
            match entry.status {
                ExtensionStatus::Active => return Ok(()),
                ExtensionStatus::Inactive => {}
                _ => return Err(CallError::Unavailable),
            }
            (
                entry.extension.clone(),
                entry.capability.clone(),
                entry.reviews.clone(),
            )
        };

        let limit = self.inner.options.activation_timeout;
        let result = run_guarded(limit, &reviews, async move {
            extension.activate(capability as Arc<dyn Host>).await
        })
        .await;
        match result {
            Ok(()) => {
                let mut entries = self.entries();
                let entry = find(&mut entries, id)?;
                // A disable during activation wins.
                if entry.status == ExtensionStatus::Inactive {
                    entry.status = ExtensionStatus::Active;
                }
                Ok(())
            }
            Err(message) => Err(self.record_failure(id, message)),
        }
    }

    /// Sends a request, activating the extension first if needed.
    pub async fn request(
        &self,
        id: &str,
        request: ExtensionRequest,
    ) -> Result<ExtensionResponse, CallError> {
        self.activate(id).await?;
        let (extension, capability, reviews) = {
            let mut entries = self.entries();
            let entry = find(&mut entries, id)?;
            if entry.status != ExtensionStatus::Active {
                return Err(CallError::Unavailable);
            }
            (
                entry.extension.clone(),
                entry.capability.clone(),
                entry.reviews.clone(),
            )
        };

        let limit = self.inner.options.call_timeout;
        let result = run_guarded(limit, &reviews, async move {
            extension.handle(request, capability as Arc<dyn Host>).await
        })
        .await;
        match result {
            Ok(response) => {
                if let Ok(entry) = find(&mut self.entries(), id) {
                    entry.failures = 0;
                }
                Ok(response)
            }
            Err(message) => Err(self.record_failure(id, message)),
        }
    }

    fn record_failure(&self, id: &str, message: String) -> CallError {
        tracing::warn!(extension = %id, %message, "extension call failed");
        let disabled = {
            let limit = self.inner.options.failure_limit;
            let mut entries = self.entries();
            find(&mut entries, id).is_ok_and(|entry| {
                entry.failures += 1;
                let disable = entry.failures >= limit
                    && matches!(
                        entry.status,
                        ExtensionStatus::Active | ExtensionStatus::Inactive
                    );
                if disable {
                    entry.status = ExtensionStatus::Failed {
                        reason: message.clone(),
                    };
                }
                disable
            })
        };
        if disabled {
            self.publish();
        }
        CallError::Failed { message }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.inner.entries.lock().expect("extension registry lock")
    }

    fn publish(&self) {
        let language = &self.inner.options.language;
        let mut extensions = Vec::new();
        let mut category_titles = BTreeMap::new();
        for entry in self.entries().iter().filter(|entry| {
            matches!(
                entry.status,
                ExtensionStatus::Inactive | ExtensionStatus::Active
            )
        }) {
            let manifest = &entry.manifest;
            let translations = translations_for(manifest, language);
            for view in &manifest.contributions.resource_views {
                if let Some(title) = translations.and_then(|t| t.get(&view.category)) {
                    category_titles
                        .entry(view.category.clone())
                        .or_insert_with(|| title.clone());
                }
            }
            extensions.push((
                manifest.id.clone(),
                localized(&manifest.contributions, translations),
            ));
        }
        self.inner.contributions.send_if_modified(|current| {
            let changed = current.extensions != extensions;
            if changed {
                let version = current.version + 1;
                *current = Arc::new(ContributionSet {
                    version,
                    extensions,
                    category_titles,
                });
            }
            changed
        });
    }
}

fn find<'a>(entries: &'a mut [Entry], id: &str) -> Result<&'a mut Entry, CallError> {
    entries
        .iter_mut()
        .find(|entry| {
            entry.manifest.id == id && !matches!(entry.status, ExtensionStatus::Refused { .. })
        })
        .ok_or(CallError::UnknownExtension)
}

/// Runs extension code in its own task, turning panics and overruns into errors.
/// A task stuck in synchronous code cannot be stopped, but the host stops waiting.
/// Time spent in a user's review does not count, and the limit starts over after.
async fn run_guarded<T: Send + 'static>(
    limit: Duration,
    reviews: &ReviewClock,
    call: impl Future<Output = Result<T, ExtensionError>> + Send + 'static,
) -> Result<T, String> {
    let mut task = tokio::spawn(call);
    let mut last_seen = reviews.progress();
    let finished = loop {
        match time::timeout(limit, &mut task).await {
            Ok(finished) => break finished,
            Err(_) => {
                let progress = reviews.progress();
                if progress.is_none() || progress != last_seen {
                    last_seen = progress;
                    continue;
                }
                task.abort();
                return Err(format!("the extension did not respond within {limit:?}"));
            }
        }
    };
    match finished {
        Err(error) if error.is_panic() => Err("the extension panicked".to_owned()),
        Err(_) => Err("the extension call was cancelled".to_owned()),
        Ok(Err(error)) => Err(error.message),
        Ok(Ok(value)) => Ok(value),
    }
}
