//! Builds the cluster list from kubeconfig files. Nothing here touches the
//! network or runs a credential helper.

use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kube::config::{Kubeconfig, KubeconfigError};
use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use tokio::sync::watch;

const RELOAD_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterEntry {
    pub context: String,
    pub cluster: Option<String>,
    pub server: Option<String>,
    pub user: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct ClusterList {
    pub version: u64,
    pub clusters: Vec<ClusterEntry>,
    pub errors: Vec<FileError>,
    // Holds credentials, so this type deliberately has no `Debug`.
    pub(crate) kubeconfig: Kubeconfig,
}

/// The files to read: `KUBECONFIG` when it is set, otherwise `~/.kube/config`.
pub fn default_paths() -> Vec<PathBuf> {
    resolve_paths(env::var_os("KUBECONFIG"), env::home_dir())
}

fn resolve_paths(kubeconfig: Option<OsString>, home: Option<PathBuf>) -> Vec<PathBuf> {
    match kubeconfig.filter(|value| !value.is_empty()) {
        Some(value) => env::split_paths(&value)
            .filter(|path| !path.as_os_str().is_empty())
            .collect(),
        None => home
            .map(|home| home.join(".kube").join("config"))
            .into_iter()
            .collect(),
    }
}

/// Reads and merges `paths` in order, so the first file to define a name wins.
/// A missing file is skipped, and an invalid one is reported without affecting
/// the others.
pub fn load(paths: &[PathBuf]) -> ClusterList {
    let mut merged = Kubeconfig::default();
    let mut errors = Vec::new();
    for path in paths {
        match Kubeconfig::read_from(path).and_then(|next| merged.clone().merge(next)) {
            Ok(next) => merged = next,
            Err(error) if is_not_found(&error) => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), "skipping invalid kubeconfig file");
                errors.push(FileError {
                    path: path.clone(),
                    message: describe(&error),
                });
            }
        }
    }
    ClusterList {
        version: 0,
        clusters: entries(&merged),
        errors,
        kubeconfig: merged,
    }
}

fn is_not_found(error: &KubeconfigError) -> bool {
    matches!(error, KubeconfigError::ReadConfig(source, _) if source.kind() == io::ErrorKind::NotFound)
}

/// Parser messages can quote the file's contents, including credentials, so
/// they are reduced to a summary.
fn describe(error: &KubeconfigError) -> String {
    match error {
        KubeconfigError::Parse(_) => "the file is not valid kubeconfig YAML".to_owned(),
        KubeconfigError::ReadConfig(source, _) => format!("the file could not be read: {source}"),
        KubeconfigError::KindMismatch | KubeconfigError::ApiVersionMismatch => {
            "the file does not match the kind or version of the other kubeconfig files".to_owned()
        }
        _ => "the file could not be loaded".to_owned(),
    }
}

fn entries(kubeconfig: &Kubeconfig) -> Vec<ClusterEntry> {
    kubeconfig
        .contexts
        .iter()
        .map(|named| {
            let context = named.context.as_ref();
            let cluster = context.map(|c| c.cluster.clone());
            let server = cluster.as_ref().and_then(|name| {
                let named = kubeconfig.clusters.iter().find(|c| &c.name == name)?;
                named.cluster.as_ref()?.server.clone()
            });
            ClusterEntry {
                context: named.name.clone(),
                cluster,
                server,
                user: context.and_then(|c| c.user.clone()),
                namespace: context.and_then(|c| c.namespace.clone()),
            }
        })
        .collect()
}

/// Keeps a [`ClusterList`] current as kubeconfig files change. Dropping it stops
/// watching.
pub struct KubeconfigWatcher {
    list: watch::Receiver<Arc<ClusterList>>,
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

impl KubeconfigWatcher {
    pub fn start(paths: Vec<PathBuf>) -> notify::Result<Self> {
        let paths: Vec<PathBuf> = paths.iter().map(|p| absolute(p)).collect();
        let (sender, list) = watch::channel(Arc::new(load(&paths)));

        let watched = paths.clone();
        let mut debouncer = new_debouncer(RELOAD_DEBOUNCE, move |result: DebounceEventResult| {
            let relevant = match result {
                Ok(events) => events.iter().any(|event| watched.contains(&event.path)),
                Err(error) => {
                    tracing::warn!(%error, "kubeconfig watcher error");
                    false
                }
            };
            if relevant {
                let version = sender.borrow().version + 1;
                sender.send_replace(Arc::new(ClusterList {
                    version,
                    ..load(&watched)
                }));
            }
        })?;

        // Watching directories catches files that are created later or replaced
        // by rename, which is how most tools save them.
        let directories: HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
        for directory in directories.into_iter().filter(|d| d.is_dir()) {
            debouncer
                .watcher()
                .watch(directory, RecursiveMode::NonRecursive)?;
        }
        Ok(Self {
            list,
            _debouncer: debouncer,
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<ClusterList>> {
        self.list.clone()
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs};

    use super::*;

    fn config(names: &[(&str, &str)]) -> String {
        let mut clusters = String::new();
        let mut contexts = String::new();
        for (name, server) in names {
            writeln!(clusters, "- name: {name}\n  cluster:\n    server: {server}").unwrap();
            writeln!(
                contexts,
                "- name: {name}\n  context:\n    cluster: {name}\n    user: u"
            )
            .unwrap();
        }
        format!(
            "apiVersion: v1\nkind: Config\nclusters:\n{clusters}contexts:\n{contexts}users:\n- name: u\n  user: {{}}\n"
        )
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn names(list: &ClusterList) -> Vec<&str> {
        list.clusters.iter().map(|c| c.context.as_str()).collect()
    }

    #[test]
    fn uses_home_config_when_kubeconfig_is_unset() {
        let home = PathBuf::from("/home/someone");
        let expected = vec![home.join(".kube").join("config")];
        assert_eq!(resolve_paths(None, Some(home.clone())), expected);
        assert_eq!(resolve_paths(Some("".into()), Some(home)), expected);
    }

    #[test]
    fn splits_kubeconfig_into_paths() {
        let value = env::join_paths(["/a/one", "/b/two"]).unwrap();
        let paths = resolve_paths(Some(value), None);
        assert_eq!(paths, [PathBuf::from("/a/one"), PathBuf::from("/b/two")]);
    }

    #[test]
    fn lists_context_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "config",
            &config(&[("dev", "https://dev.example:6443")]),
        );
        let list = load(&[path]);
        assert_eq!(
            list.clusters,
            [ClusterEntry {
                context: "dev".into(),
                cluster: Some("dev".into()),
                server: Some("https://dev.example:6443".into()),
                user: Some("u".into()),
                namespace: None,
            }]
        );
    }

    #[test]
    fn first_file_wins_when_merging() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(
            dir.path(),
            "first",
            &config(&[("shared", "https://first"), ("a", "https://a")]),
        );
        let second = write(
            dir.path(),
            "second",
            &config(&[("shared", "https://second"), ("b", "https://b")]),
        );
        let list = load(&[first, second]);
        assert_eq!(names(&list), ["shared", "a", "b"]);
        assert_eq!(list.clusters[0].server.as_deref(), Some("https://first"));
    }

    #[test]
    fn skips_missing_files_without_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let real = write(dir.path(), "real", &config(&[("a", "https://a")]));
        let list = load(&[dir.path().join("missing"), real]);
        assert_eq!(names(&list), ["a"]);
        assert!(list.errors.is_empty());
    }

    #[test]
    fn invalid_file_is_reported_and_valid_files_still_list() {
        let dir = tempfile::tempdir().unwrap();
        let bad = write(dir.path(), "bad", "clusters: [unclosed");
        let good = write(dir.path(), "good", &config(&[("a", "https://a")]));
        let list = load(&[bad.clone(), good]);
        assert_eq!(names(&list), ["a"]);
        assert_eq!(list.errors.len(), 1);
        assert_eq!(list.errors[0].path, bad);
    }

    #[test]
    fn parse_errors_do_not_quote_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let bad = write(
            dir.path(),
            "bad",
            "users:\n- name: u\n  user:\n    token: s3cr3t-value: [\n",
        );
        let list = load(&[bad]);
        assert_eq!(list.errors.len(), 1);
        assert!(!list.errors[0].message.contains("s3cr3t-value"));
    }

    #[test]
    fn lists_five_hundred_contexts() {
        let dir = tempfile::tempdir().unwrap();
        let contexts: Vec<(String, String)> = (0..500)
            .map(|i| (format!("cluster-{i}"), format!("https://c{i}.example")))
            .collect();
        let borrowed: Vec<(&str, &str)> = contexts
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let path = write(dir.path(), "config", &config(&borrowed));
        let list = load(&[path]);
        assert_eq!(list.clusters.len(), 500);
        assert_eq!(list.clusters[499].context, "cluster-499");
    }

    #[tokio::test]
    async fn watcher_reloads_when_a_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "config", &config(&[("a", "https://a")]));
        let watcher = KubeconfigWatcher::start(vec![path.clone()]).unwrap();
        let mut list = watcher.subscribe();
        assert_eq!(names(&list.borrow()), ["a"]);

        fs::write(&path, config(&[("a", "https://a"), ("b", "https://b")])).unwrap();
        let updated = tokio::time::timeout(
            Duration::from_secs(10),
            list.wait_for(|list| list.clusters.len() == 2),
        )
        .await
        .expect("the list should update")
        .unwrap();
        assert_eq!(names(&updated), ["a", "b"]);
        assert!(updated.version > 0);
    }

    #[tokio::test]
    async fn watcher_sees_a_file_created_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let watcher = KubeconfigWatcher::start(vec![path.clone()]).unwrap();
        let mut list = watcher.subscribe();
        assert!(list.borrow().clusters.is_empty());

        fs::write(&path, config(&[("a", "https://a")])).unwrap();
        tokio::time::timeout(
            Duration::from_secs(10),
            list.wait_for(|list| list.clusters.len() == 1),
        )
        .await
        .expect("the list should update")
        .unwrap();
    }
}
