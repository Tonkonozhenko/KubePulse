//! The locale is process-wide, so tests that change it have this binary to
//! themselves. Every test here runs in the pseudo-locale.

mod support;

use std::{collections::BTreeMap, sync::Arc};

use gpui_kit::{
    AppContext as _, Entity, TestAppContext, VisualTestContext, WindowOptions,
    component::dock::DockPlacement,
    test::{TestQueryExt as _, TestWindowExt as _},
};
use kubepulse_cluster::{ConnectionState, ResourceKind};
use kubepulse_extension_api::{
    ColumnContribution, ColumnSource, Contributions, KindRef, ResourceViewContribution, ViewIcon,
};
use kubepulse_extension_host::ContributionSet;
use kubepulse_store::synthetic::{SyntheticCluster, pod_kind};
use kubepulse_ui::{
    cluster_model::{ClusterModel, state_label},
    cluster_rail::tooltip_text,
    columns::navigation,
    identity::{ClusterIdentity, ClusterKey, IdentityStore},
    language,
    preferences::Preferences,
    runtime::{ClusterState, ClusterStatus},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    workspace::Workspace,
};
use support::{FakeServices, connected, kind};
use tokio::sync::{mpsc, watch};

const ENGLISH: &str = include_str!("../locales/en.json");
const CONTEXT: &str = "test";

fn pseudo() {
    language::set_current(language::PSEUDO);
}

fn is_pseudo(text: &str) -> bool {
    text.starts_with('[') && text.ends_with(']')
}

fn contributions() -> ContributionSet {
    let pod = KindRef::new("", "Pod");
    ContributionSet {
        version: 1,
        extensions: vec![(
            "test".to_owned(),
            Contributions {
                resource_views: vec![ResourceViewContribution {
                    id: "pods".to_owned(),
                    title: "Pods".to_owned(),
                    kind: pod.clone(),
                    category: "Workloads".to_owned(),
                    icon: ViewIcon::default(),
                }],
                columns: vec![ColumnContribution {
                    id: "status".to_owned(),
                    title: "Phase".to_owned(),
                    kind: pod,
                    source: ColumnSource::Path {
                        path: ".status.phase".to_owned(),
                    },
                }],
                ..Contributions::default()
            },
        )],
        ..ContributionSet::default()
    }
}

fn status(resources: Vec<ResourceKind>) -> ClusterStatus {
    connected(CONTEXT, resources)
}

fn key() -> ClusterKey {
    ClusterKey::new(CONTEXT, Some("https://test.example"))
}

/// Live for the test, so the model keeps its channels.
struct Channels(
    #[expect(dead_code)] watch::Sender<Arc<ClusterState>>,
    #[expect(dead_code)] mpsc::UnboundedReceiver<kubepulse_ui::runtime::ClusterCommand>,
);

fn workspace(
    cx: &mut TestAppContext,
) -> (
    Channels,
    Entity<Workspace>,
    Entity<ShellState>,
    &mut VisualTestContext,
) {
    cx.executor().allow_parking();
    let (state, receiver) = watch::channel(Arc::new(ClusterState {
        version: 1,
        clusters: vec![status(vec![pod_kind()])],
        errors: vec![],
    }));
    let (sender, commands) = mpsc::unbounded_channel();
    let services = Arc::new(FakeServices::new(
        Arc::new(SyntheticCluster::new(10, None)),
        contributions(),
    ));
    let (model, shell) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(SharedServices(services));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        let shell = cx.new(|cx| {
            let mut shell = ShellState::default();
            shell.select(key(), cx);
            shell
        });
        cx.set_global(SharedShellState(shell.clone()));
        settings_tab::init(Preferences::default(), None, cx);
        (model, shell)
    });
    let (handle, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(model, None, window, cx))
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1000.)));
    cx.run_until_parked();
    (Channels(state, commands), workspace, shell, cx)
}

fn labels(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window
            .elements()
            .iter()
            .filter_map(|element| element.label().map(str::to_owned))
            .collect()
    })
}

fn assert_no_english(cx: &mut VisualTestContext) {
    let english: BTreeMap<String, String> = serde_json::from_str(ENGLISH).unwrap();
    let found = labels(cx);
    assert!(found.iter().any(|label| is_pseudo(label)), "{found:?}");
    for label in found {
        assert!(
            !english.values().any(|text| *text == label),
            "untranslated: {label}"
        );
    }
}

#[test]
fn the_locale_reaches_our_catalogs_and_the_kit() {
    pseudo();
    assert_eq!(language::current(), language::PSEUDO);
    assert_eq!(&*gpui_kit::component::locale(), language::PSEUDO);
    assert_eq!(language::name(language::ENGLISH), "English");
    assert!(is_pseudo(&language::name(language::PSEUDO)));
    assert_eq!(language::shipped(), [language::ENGLISH]);
}

#[test]
fn a_failed_connection_has_a_translated_summary_and_a_verbatim_detail() {
    pseudo();
    let identity = ClusterIdentity {
        name: "Production".to_owned(),
        color: 0,
        protected: false,
    };
    let failed = ConnectionState::Failed {
        reason: "dial tcp 192.0.2.1:6443: i/o timeout".to_owned(),
    };
    let summary = state_label(&failed);
    assert!(is_pseudo(&summary), "{summary}");
    let tooltip = tooltip_text(&identity, "prod", &failed);
    assert!(tooltip.ends_with(&format!("{summary}: dial tcp 192.0.2.1:6443: i/o timeout")));
}

#[test]
fn navigation_translates_our_group_and_keeps_kinds_from_the_cluster() {
    pseudo();
    let mut widget = kind("example.com", "Widget", "widgets", true);
    widget.verbs = vec!["list".to_owned()];
    let groups = navigation(std::slice::from_ref(&widget), &contributions());
    assert!(is_pseudo(&groups[0].title), "{}", groups[0].title);
    assert_eq!(groups[0].sections[0].name, "example.com", "cluster data");
    assert_eq!(
        groups[0].sections[0].kinds[0].title, "widgets",
        "cluster data"
    );
}

#[gpui_kit::test]
fn a_live_workspace_shows_no_english_catalog_text(cx: &mut TestAppContext) {
    pseudo();
    let (_channels, workspace, shell, cx) = workspace(cx);
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.show_kind(key(), pod_kind(), true, window, cx);
        });
    });
    support::wait_until(cx, "the rows", |cx| {
        cx.update(|_, cx| {
            let shell = shell.read(cx);
            let tab = shell.tab(shell.active().unwrap()).unwrap();
            let browser = tab.read(cx).browser().unwrap().clone();
            !browser.read(cx).snapshot().rows.is_empty()
        })
    });
    assert_no_english(cx);

    let (headers, rows) = cx.update(|_, cx| {
        let shell = shell.read(cx);
        let tab = shell.tab(shell.active().unwrap()).unwrap();
        let browser = tab.read(cx).browser().unwrap().clone();
        browser.read(cx).table().read(cx).dump_range(0..1, cx)
    });
    // After the checkbox and action columns, which have no title.
    assert!(is_pseudo(&headers[2]), "{headers:?}");
    let status = headers.iter().position(|title| title == "Phase").unwrap();
    assert!(!is_pseudo(&rows[0][status]), "cluster data stays verbatim");

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(settings_tab::OpenSettings), cx);
    });
    cx.run_until_parked();
    let empty = cx.update(|_, cx| {
        workspace
            .read(cx)
            .dock()
            .read(cx)
            .is_empty(DockPlacement::Center, cx)
    });
    assert!(!empty);
    assert_no_english(cx);
}

#[test]
fn a_translated_category_groups_by_its_declared_name() {
    pseudo();
    let mut set = contributions();
    let view = set.extensions[0].1.resource_views[0].clone();
    set.extensions.push((
        "other".to_owned(),
        Contributions {
            resource_views: vec![ResourceViewContribution {
                kind: KindRef::new("batch", "Job"),
                ..view
            }],
            ..Contributions::default()
        },
    ));
    set.category_titles
        .insert("Workloads".to_owned(), "[Wörkloads]".to_owned());
    let job = kind("batch", "Job", "jobs", true);
    let groups = navigation(&[pod_kind(), job], &set);
    assert_eq!(groups.len(), 1, "one group for both extensions");
    assert_eq!(groups[0].name, "Workloads");
    assert_eq!(groups[0].title, "[Wörkloads]");
    assert_eq!(groups[0].kinds.len(), 2);
}
