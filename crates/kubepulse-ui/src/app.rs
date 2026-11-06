//! Starts the application.

use std::{path::PathBuf, sync::Arc, time::Instant};

use gpui_kit::{assets::AllAssets, *};
use kubepulse_cluster::{SessionOptions, default_paths};
use kubepulse_extension_api::{ActivationTrigger, Extension};
use kubepulse_extension_host::{ExtensionHost, HostOptions};

use crate::{
    cluster_model::ClusterModel,
    command_palette,
    identity::IdentityStore,
    language,
    persistence::{ConfigPaths, read_or_default},
    preferences::Preferences,
    runtime::{ClusterBackend, ClusterRuntime, ConnectedClusters, RuntimeServices, StoreConfig},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    terminal_tab,
    theme::ThemeWatcher,
    workspace::Workspace,
};

/// Overrides for tests and the startup check.
#[derive(Default)]
pub struct RunOptions {
    pub config: Option<ConfigPaths>,
    pub kubeconfig: Option<Vec<PathBuf>>,
    pub extensions: Vec<Arc<dyn Extension>>,
    /// Called once with the time to the first painted frame, then the
    /// application quits.
    pub exit_after_first_frame: Option<Box<dyn FnOnce(std::time::Duration)>>,
}

/// Keeps the background threads running for the life of the application.
struct BackgroundRuntime(#[expect(dead_code)] ClusterRuntime);

impl Global for BackgroundRuntime {}

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
    let preferences = config.as_ref().map_or_else(Preferences::default, |config| {
        read_or_default(&config.preferences, Preferences::from_json)
    });
    language::set_current(&language::select(&preferences));
    let themes = config
        .as_ref()
        .map(|config| ThemeWatcher::start(config.themes.clone()));
    let clusters = Arc::new(ConnectedClusters::default());
    let (backend, host_requests) = ClusterBackend::new(clusters.clone());
    let host = ExtensionHost::new(
        Arc::new(backend),
        HostOptions {
            language: language::current(),
            ..HostOptions::default()
        },
    );
    host.load(options.extensions);
    let stores = StoreConfig::for_host(clusters.clone(), &host);
    let runtime = ClusterRuntime::start(paths, SessionOptions::default(), stores)
        .expect("failed to start background threads");
    let startup_host = host.clone();
    runtime
        .handle()
        .spawn(async move { startup_host.trigger(&ActivationTrigger::Startup).await });
    let services = Arc::new(RuntimeServices::new(clusters, host, runtime.handle()));
    let mut on_first_frame = options.exit_after_first_frame;

    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            command_palette::init(cx);
            terminal_tab::init(cx);
            cx.set_global(SharedServices(services.clone()));

            let (state, commands) = (runtime.state(), runtime.commands());
            cx.set_global(BackgroundRuntime(runtime));
            let model = cx.new(|cx| ClusterModel::new(state, commands, identities, cx));
            let shell = cx.new(|_| ShellState::default());
            cx.set_global(SharedShellState(shell));
            settings_tab::init(preferences, themes, cx);

            let window_options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("KubePulse".into()),
                    ..Default::default()
                }),
                app_id: Some("kubepulse".to_owned()),
                icon: Some(window_icon()),
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
                cx.new(|cx| {
                    let workspace = Workspace::new(model, config, window, cx);
                    workspace.listen(host_requests, window, cx);
                    workspace
                })
            })
            .expect("failed to open window");
        });
}

fn window_icon() -> Arc<image::RgbaImage> {
    let logo = image::load_from_memory(include_bytes!("../../../assets/logo-256.png"))
        .expect("the bundled logo is a valid PNG");
    Arc::new(logo.into_rgba8())
}
