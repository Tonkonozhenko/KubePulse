//! Keyboard access to navigation and actions, including extension commands.

use std::rc::Rc;

use gpui_kit::{
    assets::IconName,
    component::{
        Icon,
        command::{CommandGroup, CommandItem},
    },
    *,
};
use kubepulse_cluster::ResourceKind;
use kubepulse_extension_host::ContributionSet;
use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, can_connect, is_live},
    columns::{NavigationEntry, navigation},
    icons::view_glyph,
    identity::ClusterKey,
};

pub const PALETTE_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "cmd-shift-p"
} else {
    "ctrl-shift-p"
};

gpui_kit::actions!(kubepulse, [OpenCommandPalette]);

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(PALETTE_SHORTCUT, OpenCommandPalette, None)]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    OpenKind { key: ClusterKey, kind: ResourceKind },
    RunCommand { extension: String, command: String },
    Connect(String),
    Disconnect(String),
    OpenSettings,
    OpenForwards,
    OpenAbout,
}

#[derive(Clone)]
pub struct PaletteEntry {
    pub label: String,
    pub keywords: Vec<String>,
    pub icon: Icon,
    pub action: PaletteAction,
}

pub struct PaletteGroup {
    pub heading: String,
    pub entries: Vec<PaletteEntry>,
}

impl PaletteGroup {
    pub fn command_group(&self) -> CommandGroup {
        CommandGroup::new()
            .label(self.heading.clone())
            .items(self.entries.iter().map(|entry| {
                CommandItem::new()
                    .label(entry.label.clone())
                    .icon(entry.icon.clone())
                    .keywords(entry.keywords.clone())
            }))
    }
}

fn kind_entries(
    key: &ClusterKey,
    name: &str,
    resources: &[ResourceKind],
    contributions: &ContributionSet,
) -> Vec<PaletteEntry> {
    let entry = |NavigationEntry { title, icon, kind }: NavigationEntry| PaletteEntry {
        label: title,
        keywords: vec![kind.kind.clone(), kind.plural.clone(), name.to_owned()],
        icon: Icon::new(view_glyph(icon)),
        action: PaletteAction::OpenKind {
            key: key.clone(),
            kind,
        },
    };
    navigation(resources, contributions)
        .into_iter()
        .flat_map(|group| {
            let sections = group.sections.into_iter().flat_map(|section| section.kinds);
            group.kinds.into_iter().chain(sections)
        })
        .map(entry)
        .collect()
}

/// Kinds come first, by cluster, starting with the selected cluster, so that
/// typing a kind's name lands on the cluster in use.
pub fn palette_groups(
    model: &ClusterModel,
    selected: Option<&ClusterKey>,
    contributions: &ContributionSet,
) -> Rc<Vec<PaletteGroup>> {
    let mut clusters: Vec<_> = model.state().clusters.iter().collect();
    clusters.sort_by_key(|status| selected != Some(&model.key(&status.entry.context)));

    let mut groups = Vec::new();
    for status in clusters.iter().filter(|status| is_live(&status.state)) {
        let key = model.key(&status.entry.context);
        let name = model.identity(&key).name;
        let entries = kind_entries(&key, &name, &status.resources, contributions);
        groups.push(PaletteGroup {
            heading: name,
            entries,
        });
    }

    let commands = contributions
        .extensions
        .iter()
        .flat_map(|(extension, contributed)| {
            contributed.commands.iter().map(|command| PaletteEntry {
                label: command.title.clone(),
                keywords: Vec::new(),
                icon: Icon::new(IconName::Play),
                action: PaletteAction::RunCommand {
                    extension: extension.clone(),
                    command: command.id.clone(),
                },
            })
        });
    let settings = PaletteEntry {
        label: t!("settings.title").to_string(),
        keywords: Vec::new(),
        icon: Icon::new(IconName::Settings),
        action: PaletteAction::OpenSettings,
    };
    let forwards = PaletteEntry {
        label: t!("forward.title").to_string(),
        keywords: Vec::new(),
        icon: Icon::new(IconName::ArrowRightLeft),
        action: PaletteAction::OpenForwards,
    };
    let about = PaletteEntry {
        label: t!("about.title").to_string(),
        keywords: vec![
            t!("about.license").to_string(),
            t!("about.notices").to_string(),
        ],
        icon: Icon::new(IconName::Info),
        action: PaletteAction::OpenAbout,
    };
    groups.push(PaletteGroup {
        heading: t!("palette.commands").to_string(),
        entries: commands.chain([settings, forwards, about]).collect(),
    });

    let connections = clusters.iter().filter_map(|status| {
        let context = status.entry.context.clone();
        let name = model.identity(&model.key(&context)).name;
        let (label, icon, action) = if can_connect(&status.state) {
            let label = t!("palette.connect", name = name);
            (
                label,
                IconName::Plug,
                PaletteAction::Connect(context.clone()),
            )
        } else if is_live(&status.state) {
            let label = t!("palette.disconnect", name = name);
            let action = PaletteAction::Disconnect(context.clone());
            (label, IconName::Unplug, action)
        } else {
            return None;
        };
        Some(PaletteEntry {
            label: label.to_string(),
            keywords: vec![context],
            icon: Icon::new(icon),
            action,
        })
    });
    groups.push(PaletteGroup {
        heading: t!("palette.clusters").to_string(),
        entries: connections.collect(),
    });

    groups.retain(|group| !group.entries.is_empty());
    Rc::new(groups)
}
