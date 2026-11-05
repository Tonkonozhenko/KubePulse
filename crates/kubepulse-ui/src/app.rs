//! Starts the application.

use std::{path::PathBuf, sync::Arc, time::Instant};

use gpui_kit::*;
use kubepulse_cluster::{SessionOptions, default_paths};
use kubepulse_extension_api::{ActivationTrigger, Extension};
use kubepulse_extension_host::{BuiltinLoader, ExtensionHost, HostOptions};

use crate::{
    cluster_model::{ClusterModel, SharedClusterModel},
    cluster_tab::ClusterTab,
    identity::IdentityStore,
    persistence::{ConfigPaths, read_or_default},
    runtime::{ClusterBackend, ClusterRuntime, ConnectedClusters, RuntimeServices, StoreConfig},
    services::SharedServices,
    workspace::Workspace,
};

/// Overrides for tests and the startup check.
#[derive(Default)]
pub struct RunOptions {
    pub config: Option<ConfigPaths>,
    pub kubeconfig: Option<Vec<PathBuf>>,
    /// Extensions compiled into the application.
    pub extensions: Vec<Arc<dyn Extension>>,
    /// Called once with the time to the first painted frame, then the
    /// application quits.
    pub exit_after_first_frame: Option<Box<dyn FnOnce(std::time::Duration)>>,
}

/// Keeps the background threads running for the life of the application.
struct BackgroundRuntime(#[allow(dead_code)] ClusterRuntime);

impl Global for BackgroundRuntime {}

/// Opens the main window and runs the application until it exits.
pub fn run() {
    run_with(RunOptions::default());
}

pub fn run_with(options: RunOptions) {
    let started = Instant::now();
    let config = options.config.or_else(ConfigPaths::platform_default);
    let paths = options.kubeconfig.unwrap_or_else(default_paths);
    let identities = config
        .as_ref()
        .map_or_else(IdentityStore::default, |config| {
            read_or_default(&config.clusters, IdentityStore::from_json)
        });
    let clusters = Arc::new(ConnectedClusters::default());
    let backend = Arc::new(ClusterBackend(clusters.clone()));
    let host = ExtensionHost::new(backend, HostOptions::default());
    host.load(&BuiltinLoader::new(options.extensions));
    let stores = StoreConfig::for_host(clusters.clone(), &host);
    let runtime = ClusterRuntime::start(paths, SessionOptions::default(), stores)
        .expect("failed to start background threads");
    let startup_host = host.clone();
    runtime
        .handle()
        .spawn(async move { startup_host.trigger(&ActivationTrigger::Startup).await });
    let services = Arc::new(RuntimeServices::new(clusters, host, runtime.handle()));
    let mut on_first_frame = options.exit_after_first_frame;

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        ClusterTab::register(cx);
        cx.set_global(SharedServices(services.clone()));

        let (state, commands) = (runtime.state(), runtime.commands());
        cx.set_global(BackgroundRuntime(runtime));
        let model = cx.new(|cx| ClusterModel::new(state, commands, identities, cx));
        cx.set_global(SharedClusterModel(model.clone()));

        let window_options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("KubePulse".into()),
                ..Default::default()
            }),
            app_id: Some("kubepulse".to_owned()),
            ..Default::default()
        };
        gpui_kit::open_window(window_options, cx, |window, cx| {
            if let Some(report) = on_first_frame.take() {
                // Frame callbacks run before the draw, and a task queued here
                // runs right after it.
                window.on_next_frame(move |_, cx| {
                    cx.spawn(async move |cx| {
                        report(started.elapsed());
                        cx.update(|cx| cx.quit());
                    })
                    .detach();
                });
            }
            cx.new(|cx| Workspace::new(model, config, window, cx))
        })
        .expect("failed to open window");
    });
}
