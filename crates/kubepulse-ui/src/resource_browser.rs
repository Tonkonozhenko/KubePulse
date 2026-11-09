//! The table for one kind. Each browser has its own view of the shared store,
//! so two tabs keep their own filter, sort, and scroll position. The namespace
//! selection belongs to the cluster, so every namespaced table shares it.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        combobox::{Combobox, ComboboxEvent, ComboboxState},
        input::{Input, InputEvent, InputState},
        menu::{PopupMenu, PopupMenuItem},
        searchable_list::SearchableVec,
        table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ObjectTarget, ResourceKind};
use kubepulse_extension_api::{ClusterRef, ExtensionRequest, ExtensionResponse, ObjectRef};
use kubepulse_store::{
    Cell, LabelSelector, Row, Snapshot, SortDirection, SortKey, View, ViewQuery,
};

use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, ClusterModelEvent},
    columns::{
        ComputedColumn, KindColumns, is_cronjob, is_pod, is_restartable, is_service,
        kind_operation, kind_ref,
    },
    identity::ClusterKey,
    object_dialog::Operation,
    services::ClusterServices,
    status::{Tone, status_tag, status_tone},
};

const PENDING_CELL: &str = "…";

const SELECT_COLUMN: usize = 0;
const ACTIONS_COLUMN: usize = 1;
const SELECT_WIDTH: f32 = 40.;
const ACTION_WIDTH: f32 = 28.;
/// The padding around the buttons of the actions column.
const ACTIONS_PADDING: f32 = 24.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
    Logs,
    Shell,
    Forward,
    Restart,
    Run,
    Edit,
    Delete,
}

impl RowAction {
    fn button_id(self) -> &'static str {
        match self {
            Self::Logs => "row-logs",
            Self::Shell => "row-shell",
            Self::Forward => "row-forward",
            Self::Restart => "row-restart",
            Self::Run => "row-run",
            Self::Edit => "row-edit",
            Self::Delete => "row-delete",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Logs => IconName::Logs,
            Self::Shell => IconName::SquareTerminal,
            Self::Forward => IconName::ArrowRightLeft,
            Self::Restart => Operation::Restart.icon(),
            Self::Run => Operation::Run.icon(),
            Self::Edit => IconName::Pencil,
            Self::Delete => IconName::Trash,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Logs => t!("detail.logs").to_string(),
            Self::Shell => t!("detail.shell").to_string(),
            Self::Forward => t!("forward.action").to_string(),
            Self::Restart => Operation::Restart.label(),
            Self::Run => Operation::Run.label(),
            Self::Edit => t!("detail.edit").to_string(),
            Self::Delete => Operation::Delete.label(),
        }
    }
}

fn row_actions(kind: &ResourceKind) -> &'static [RowAction] {
    if is_pod(kind) {
        &[
            RowAction::Logs,
            RowAction::Shell,
            RowAction::Forward,
            RowAction::Edit,
            RowAction::Delete,
        ]
    } else if is_service(kind) {
        &[RowAction::Forward, RowAction::Edit, RowAction::Delete]
    } else if is_restartable(kind) {
        &[RowAction::Restart, RowAction::Edit, RowAction::Delete]
    } else if is_cronjob(kind) {
        &[RowAction::Run, RowAction::Edit, RowAction::Delete]
    } else {
        &[RowAction::Edit, RowAction::Delete]
    }
}

/// A fixed column that takes no part in sorting, moving, or selection.
fn control_column(key: &'static str, width: f32) -> Column {
    Column::new(key, "")
        .width(px(width))
        .fixed_left()
        .movable(false)
        .resizable(false)
        .selectable(false)
}

fn display_only_checkbox(id: impl Into<ElementId>, checked: bool) -> Checkbox {
    Checkbox::new(id).checked(checked).tab_stop(false)
}

type NamespacePicker = ComboboxState<SearchableVec<String>>;

pub fn cell_text(cell: Option<&Cell>) -> String {
    match cell {
        Some(Cell::Text(text)) => text.to_string(),
        Some(Cell::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// A computed value and the row it was computed for. A change replaces the row,
/// which is how an out-of-date value is noticed.
struct ComputedCell {
    row: Arc<Row>,
    text: String,
}

pub struct RowsDelegate {
    rows: Vec<Arc<Row>>,
    table_columns: Vec<Column>,
    columns: KindColumns,
    namespaced: bool,
    browser: WeakEntity<ResourceBrowser>,
    computed: HashMap<(usize, Arc<str>), ComputedCell>,
    visible: Range<usize>,
    actions: &'static [RowAction],
    /// Only ever holds rows in `rows`, so nothing hidden can be deleted.
    checked: HashSet<Arc<str>>,
    /// The row a Shift+click extends from.
    anchor: Option<Arc<str>>,
}

impl RowsDelegate {
    /// The columns before the declared ones: the checkbox, the actions, the
    /// name, and for namespaced kinds the namespace.
    fn leading_columns(&self) -> usize {
        if self.namespaced { 4 } else { 3 }
    }

    fn name_column(&self) -> usize {
        ACTIONS_COLUMN + 1
    }

    fn sort_key(&self, col_ix: usize) -> Option<SortKey> {
        let lead = self.leading_columns();
        match col_ix {
            SELECT_COLUMN | ACTIONS_COLUMN => None,
            index if index == self.name_column() => Some(SortKey::Name),
            index if index < lead => Some(SortKey::Namespace),
            index if index - lead < self.columns.declared.len() => {
                Some(SortKey::Column(index - lead))
            }
            _ => None,
        }
    }

    fn is_status_column(&self, col_ix: usize) -> bool {
        col_ix
            .checked_sub(self.leading_columns())
            .and_then(|index| self.columns.declared.get(index))
            .is_some_and(|column| column.status)
    }

    /// How a status cell is colored, or `None` for any other cell.
    pub fn cell_tone(&self, row_ix: usize, col_ix: usize, cx: &App) -> Option<Tone> {
        let text = self.cell_text(row_ix, col_ix, cx);
        (self.is_status_column(col_ix) && !text.is_empty()).then(|| status_tone(&text))
    }

    pub fn checked(&self) -> &HashSet<Arc<str>> {
        &self.checked
    }

    fn all_checked(&self) -> bool {
        !self.rows.is_empty() && self.checked.len() == self.rows.len()
    }

    fn click_checkbox(&mut self, row_ix: usize, shift: bool) {
        let Some(key) = self.rows.get(row_ix).map(|row| row.key.clone()) else {
            return;
        };
        if shift {
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|anchor| self.rows.iter().position(|row| row.key == *anchor));
            if let Some(anchor) = anchor {
                let range = anchor.min(row_ix)..=anchor.max(row_ix);
                let keys = self.rows[range].iter().map(|row| row.key.clone());
                self.checked.extend(keys);
                return;
            }
            self.checked.insert(key.clone());
        } else if !self.checked.remove(&key) {
            self.checked.insert(key.clone());
        }
        self.anchor = Some(key);
    }

    pub fn click_header_checkbox(&mut self) {
        if self.all_checked() {
            self.checked.clear();
        } else {
            self.checked = self.rows.iter().map(|row| row.key.clone()).collect();
        }
    }

    fn drop_missing_rows(&mut self) {
        if !self.checked.is_empty() {
            self.checked = self
                .rows
                .iter()
                .filter(|row| self.checked.contains(&row.key))
                .map(|row| row.key.clone())
                .collect();
        }
        if let Some(anchor) = &self.anchor
            && !self.rows.iter().any(|row| row.key == *anchor)
        {
            self.anchor = None;
        }
    }

    fn render_row_checkbox(
        &self,
        row_ix: usize,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement + use<> {
        let checked = self
            .rows
            .get(row_ix)
            .is_some_and(|row| self.checked.contains(&row.key));
        let browser = self.browser.clone();
        div()
            .id(("row-check", row_ix))
            .size_full()
            .flex()
            .items_center()
            .child(display_only_checkbox(("row-check-box", row_ix), checked))
            // The checkbox's own click cannot see Shift.
            .on_click(cx.listener(move |table, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                table
                    .delegate_mut()
                    .click_checkbox(row_ix, event.modifiers().shift);
                cx.notify();
                let _ = browser.update(cx, |_, cx| cx.notify());
            }))
            .test_support()
    }

    fn render_row_actions(&self, row_ix: usize) -> impl IntoElement + use<> {
        let row = self.rows.get(row_ix).cloned();
        let browser = self.browser.clone();
        div()
            .flex()
            .items_center()
            .gap_1()
            .children(row.into_iter().flat_map(|row| {
                let browser = browser.clone();
                self.actions.iter().map(move |&action| {
                    let (browser, row) = (browser.clone(), row.clone());
                    Button::new((action.button_id(), row_ix))
                        .icon(action.icon())
                        .ghost()
                        .xsmall()
                        .tab_stop(false)
                        .tooltip(action.label())
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = browser.update(cx, |browser, cx| {
                                browser.run_action(action, &row, cx);
                            });
                        })
                })
            }))
    }

    fn computed_cell(&self, column: usize, row: &Arc<Row>) -> Option<&str> {
        let cell = self.computed.get(&(column, row.key.clone()))?;
        Arc::ptr_eq(&cell.row, row).then_some(cell.text.as_str())
    }
}

impl TableDelegate for RowsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.table_columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.table_columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let key = self.sort_key(col_ix);
        let browser = self.browser.clone();
        // This runs inside the table's update, so the query waits for it to end.
        cx.defer(move |cx| {
            let _ = browser.update(cx, |browser, cx| {
                let order = match (key, sort) {
                    (Some(key), ColumnSort::Ascending) => Some((key, SortDirection::Ascending)),
                    (Some(key), ColumnSort::Descending) => Some((key, SortDirection::Descending)),
                    _ => None,
                };
                browser.set_sort(order, cx);
            });
        });
    }

    fn visible_rows_changed(
        &mut self,
        visible_range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.visible = visible_range;
        let browser = self.browser.clone();
        cx.defer(move |cx| {
            let _ = browser.update(cx, |browser, cx| {
                browser.sync_selection(cx);
                browser.request_computed(cx);
            });
        });
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix != SELECT_COLUMN {
            return div()
                .size_full()
                .child(self.column(col_ix, cx).name.clone())
                .into_any_element();
        }
        let browser = self.browser.clone();
        div()
            .id("select-all")
            .size_full()
            .flex()
            .items_center()
            .child(display_only_checkbox("select-all-box", self.all_checked()))
            .on_click(cx.listener(move |table, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                table.delegate_mut().click_header_checkbox();
                cx.notify();
                let _ = browser.update(cx, |_, cx| cx.notify());
            }))
            .test_support()
            .into_any_element()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        match col_ix {
            SELECT_COLUMN => self.render_row_checkbox(row_ix, cx).into_any_element(),
            ACTIONS_COLUMN => self.render_row_actions(row_ix).into_any_element(),
            _ => {
                let text = self.cell_text(row_ix, col_ix, cx);
                if self.is_status_column(col_ix) && !text.is_empty() {
                    status_tag(text).into_any_element()
                } else {
                    div().child(text).into_any_element()
                }
            }
        }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(row) = self.rows.get(row_ix).cloned() else {
            return menu;
        };
        self.actions.iter().fold(menu, |menu, &action| {
            let (browser, row) = (self.browser.clone(), row.clone());
            menu.item(
                PopupMenuItem::new(action.label())
                    .icon(action.icon())
                    .on_click(move |_, _, cx| {
                        let _ = browser.update(cx, |browser, cx| {
                            browser.run_action(action, &row, cx);
                        });
                    }),
            )
        })
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        let Some(row) = self.rows.get(row_ix) else {
            return String::new();
        };
        let lead = self.leading_columns();
        let declared = self.columns.declared.len();
        match col_ix {
            SELECT_COLUMN | ACTIONS_COLUMN => String::new(),
            index if index == self.name_column() => row.name.to_string(),
            index if index < lead => row.namespace.as_deref().unwrap_or_default().to_owned(),
            index if index - lead < declared => cell_text(row.cells.get(index - lead)),
            index => self
                .computed_cell(index - lead - declared, row)
                .unwrap_or(PENDING_CELL)
                .to_owned(),
        }
    }
}

fn namespace_kind(model: &Entity<ClusterModel>, context: &str, cx: &App) -> Option<ResourceKind> {
    let status = model.read(cx).status(context)?;
    status
        .resources
        .iter()
        .find(|kind| kind.group.is_empty() && kind.kind == "Namespace")
        .cloned()
}

pub enum BrowserEvent {
    Open(Arc<Row>),
    /// The user dismissed the open object from the table.
    Close,
    Logs(ObjectTarget),
    Shell(ObjectTarget),
    Forward(ObjectTarget),
    Edit(ObjectTarget),
    /// Asks before deleting, restarting, or running these objects.
    Confirm(Operation, Vec<ObjectTarget>),
}

impl EventEmitter<BrowserEvent> for ResourceBrowser {}

pub struct ResourceBrowser {
    key: ClusterKey,
    kind: ResourceKind,
    model: Entity<ClusterModel>,
    services: Arc<dyn ClusterServices>,
    view: View,
    query: ViewQuery,
    table: Entity<TableState<RowsDelegate>>,
    filter: Entity<InputState>,
    labels: Entity<InputState>,
    /// Only namespaced kinds have one.
    namespaces: Option<Entity<NamespacePicker>>,
    /// The picker's choices, which are refreshed only when they differ.
    namespace_options: BTreeSet<String>,
    /// The cluster's namespaces, as far as the user may list them.
    cluster_namespaces: Vec<String>,
    /// Whether the cluster's namespaces are unavailable.
    namespaces_incomplete: bool,
    _namespace_view: Option<View>,
    snapshot: Arc<Snapshot>,
    selected: Option<Arc<str>>,
    /// Selection events still on their way from changes made here, not by the
    /// user. The table delivers them after the change that caused them returns.
    expected_selection_events: usize,
    label_error: Option<String>,
    computing: HashSet<(usize, Arc<str>)>,
    _subscriptions: Vec<Subscription>,
}

impl ResourceBrowser {
    /// Opens a browser, or `None` if the cluster is not connected.
    pub fn open(
        key: &ClusterKey,
        kind: ResourceKind,
        columns: KindColumns,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Self>> {
        let query = ViewQuery {
            namespaces: if kind.namespaced {
                model.read(cx).selected_namespaces(key)
            } else {
                BTreeSet::new()
            },
            ..Default::default()
        };
        let view = services.open_view(&key.context, &kind, query.clone())?;
        services.kind_opened(kind_ref(&kind));
        let key = key.clone();
        Some(cx.new(|cx| Self::new(key, kind, columns, model, services, view, query, window, cx)))
    }

    #[expect(clippy::too_many_arguments)]
    fn new(
        key: ClusterKey,
        kind: ResourceKind,
        columns: KindColumns,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
        view: View,
        query: ViewQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let actions = row_actions(&kind);
        let actions_width = ACTIONS_PADDING + ACTION_WIDTH * actions.len() as f32;
        let mut table_columns = vec![
            control_column("select", SELECT_WIDTH),
            control_column("actions", actions_width),
            Column::new("name", t!("column.name").to_string())
                .width(px(300.))
                .sortable(),
        ];
        if kind.namespaced {
            table_columns
                .push(Column::new("namespace", t!("column.namespace").to_string()).sortable());
        }
        for column in &columns.declared {
            table_columns
                .push(Column::new(column.spec.id.to_string(), column.title.to_string()).sortable());
        }
        for column in &columns.computed {
            table_columns.push(Column::new(column.id.clone(), column.title.clone()));
        }
        let delegate = RowsDelegate {
            rows: Vec::new(),
            table_columns,
            columns,
            namespaced: kind.namespaced,
            browser: cx.weak_entity(),
            computed: HashMap::new(),
            visible: 0..0,
            actions,
            checked: HashSet::new(),
            anchor: None,
        };
        let table = cx.new(|cx| TableState::new(delegate, window, cx));

        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("browser.filter").to_string()));
        let labels = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("browser.label_selector").to_string())
        });
        let mut subscriptions = vec![
            cx.subscribe(&table, Self::on_table_event),
            cx.subscribe(&filter, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.set_filter(&text, cx);
                }
            }),
            cx.subscribe(&labels, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.set_labels(&text, cx);
                }
            }),
        ];

        let mut snapshots = view.subscribe();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let latest = snapshots.borrow_and_update().clone();
                let applied = this.update_in(cx, |this, window, cx| {
                    this.apply_snapshot(latest, window, cx);
                });
                if applied.is_err() || snapshots.changed().await.is_err() {
                    return;
                }
            }
        })
        .detach();

        let namespaces = kind.namespaced.then(|| {
            let selected = query.namespaces.iter().cloned().collect::<Vec<_>>();
            let indices = (0..selected.len()).map(IndexPath::new).collect();
            let picker = cx.new(|cx| {
                ComboboxState::new(SearchableVec::new(selected), indices, window, cx)
                    .multiple(true)
                    .searchable(true)
            });
            subscriptions.push(cx.subscribe(
                &picker,
                |this, _, event: &ComboboxEvent<SearchableVec<String>>, cx| {
                    if let ComboboxEvent::Change(selected) = event {
                        this.set_namespaces(selected.iter().cloned().collect(), cx);
                    }
                },
            ));
            subscriptions.push(cx.subscribe_in(&model, window, Self::on_model_event));
            picker
        });
        let namespace_view = namespaces.as_ref().and_then(|_| {
            let kind = namespace_kind(&model, &key.context, cx)?;
            let view = services.open_view(&key.context, &kind, ViewQuery::default())?;
            let mut snapshots = view.subscribe();
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    let latest = snapshots.borrow_and_update().clone();
                    let applied = this.update_in(cx, |this, window, cx| {
                        this.apply_namespace_list(&latest, window, cx);
                    });
                    if applied.is_err() || snapshots.changed().await.is_err() {
                        return;
                    }
                }
            })
            .detach();
            Some(view)
        });

        Self {
            namespace_options: query.namespaces.clone(),
            namespaces_incomplete: namespace_view.is_none(),
            _namespace_view: namespace_view,
            cluster_namespaces: Vec::new(),
            key,
            kind,
            model,
            services,
            view,
            query,
            table,
            filter,
            labels,
            namespaces,
            snapshot: Arc::default(),
            selected: None,
            expected_selection_events: 0,
            label_error: None,
            computing: HashSet::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn table(&self) -> &Entity<TableState<RowsDelegate>> {
        &self.table
    }

    pub fn snapshot(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    pub fn selected(&self) -> Option<&Arc<str>> {
        self.selected.as_ref()
    }

    pub fn label_error(&self) -> Option<&str> {
        self.label_error.as_deref()
    }

    pub fn namespace_picker(&self) -> Option<&Entity<NamespacePicker>> {
        self.namespaces.as_ref()
    }

    pub fn namespace_options(&self) -> &BTreeSet<String> {
        &self.namespace_options
    }

    pub fn cluster_namespaces(&self) -> &[String] {
        &self.cluster_namespaces
    }

    pub fn namespaces_incomplete(&self) -> bool {
        self.namespaces_incomplete
    }

    fn apply_query(&mut self, cx: &mut Context<Self>) {
        self.view.set_query(self.query.clone());
        cx.notify();
    }

    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.text = text.to_owned();
        self.apply_query(cx);
    }

    /// Applies a label selector. An unfinished one keeps the last valid filter
    /// and shows why, because it is usually still being typed.
    pub fn set_labels(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            self.query.labels = None;
            self.label_error = None;
        } else {
            match LabelSelector::parse(text) {
                Ok(selector) => {
                    self.query.labels = Some(selector);
                    self.label_error = None;
                }
                Err(error) => {
                    self.label_error =
                        Some(t!("browser.invalid_selector", error = error).to_string());
                }
            }
        }
        self.apply_query(cx);
    }

    /// Selects namespaces for the whole cluster. This table follows the change
    /// the same way every other one does.
    pub fn set_namespaces(&mut self, selected: BTreeSet<String>, cx: &mut Context<Self>) {
        let key = self.key.clone();
        self.model
            .update(cx, |model, cx| model.set_namespaces(key, selected, cx));
    }

    pub fn set_sort(&mut self, sort: Option<(SortKey, SortDirection)>, cx: &mut Context<Self>) {
        self.query.sort = sort;
        self.apply_query(cx);
    }

    fn target(&self, row: &Row) -> ObjectTarget {
        ObjectTarget {
            kind: self.kind.clone(),
            namespace: row.namespace.as_deref().map(str::to_owned),
            name: row.name.to_string(),
        }
    }

    pub fn run_action(&mut self, action: RowAction, row: &Row, cx: &mut Context<Self>) {
        let target = self.target(row);
        cx.emit(match action {
            RowAction::Logs => BrowserEvent::Logs(target),
            RowAction::Shell => BrowserEvent::Shell(target),
            RowAction::Forward => BrowserEvent::Forward(target),
            RowAction::Edit => BrowserEvent::Edit(target),
            RowAction::Restart => BrowserEvent::Confirm(Operation::Restart, vec![target]),
            RowAction::Run => BrowserEvent::Confirm(Operation::Run, vec![target]),
            RowAction::Delete => BrowserEvent::Confirm(Operation::Delete, vec![target]),
        });
    }

    fn checked_targets(&self, cx: &App) -> Vec<ObjectTarget> {
        let delegate = self.table.read(cx).delegate();
        delegate
            .rows
            .iter()
            .filter(|row| delegate.checked.contains(&row.key))
            .map(|row| self.target(row))
            .collect()
    }

    pub fn clear_checked(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().checked.clear();
            cx.notify();
        });
        cx.notify();
    }

    /// Unchecks objects that are gone, or going, though their rows may stay
    /// until the cluster finishes removing them.
    pub fn uncheck(&mut self, targets: &[ObjectTarget], cx: &mut Context<Self>) {
        let gone: HashSet<_> = targets
            .iter()
            .map(|target| (target.namespace.as_deref(), target.name.as_str()))
            .collect();
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let keys: Vec<_> = delegate
                .rows
                .iter()
                .filter(|row| gone.contains(&(row.namespace.as_deref(), &*row.name)))
                .map(|row| row.key.clone())
                .collect();
            for key in keys {
                delegate.checked.remove(&key);
            }
            cx.notify();
        });
        cx.notify();
    }

    /// Stops showing a row as open, so clicking it opens it again.
    pub fn clear_open(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.sync_selection(cx);
    }

    fn on_table_event(
        &mut self,
        table: Entity<TableState<RowsDelegate>>,
        event: &TableEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, TableEvent::SelectRow(_) | TableEvent::ClearSelection)
            && self.expected_selection_events > 0
        {
            self.expected_selection_events -= 1;
            return;
        }
        match event {
            TableEvent::SelectRow(index) => {
                if let Some(row) = table.read(cx).delegate().rows.get(*index).cloned() {
                    self.selected = Some(row.key.clone());
                    cx.emit(BrowserEvent::Open(row));
                }
            }
            TableEvent::ClearSelection => {
                self.selected = None;
                cx.emit(BrowserEvent::Close);
            }
            _ => {}
        }
    }

    fn apply_snapshot(
        &mut self,
        snapshot: Arc<Snapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let namespaces_changed = snapshot.namespaces != self.snapshot.namespaces;
        self.snapshot = snapshot.clone();
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.rows = snapshot.rows.clone();
            delegate.drop_missing_rows();
            cx.notify();
        });
        self.sync_selection(cx);
        self.request_computed(cx);
        if namespaces_changed {
            self.sync_namespace_picker(window, cx);
        }
        cx.notify();
    }

    fn apply_namespace_list(
        &mut self,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.namespaces_incomplete = snapshot.denied.is_some();
        self.cluster_namespaces = snapshot
            .rows
            .iter()
            .map(|row| row.name.to_string())
            .collect();
        self.sync_namespace_picker(window, cx);
        cx.notify();
    }

    fn on_model_event(
        &mut self,
        model: &Entity<ClusterModel>,
        event: &ClusterModelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ClusterModelEvent::NamespacesChanged(key) = event
            && *key == self.key
        {
            self.query.namespaces = model.read(cx).selected_namespaces(key);
            self.apply_query(cx);
            self.sync_namespace_picker(window, cx);
        }
    }

    /// Offers the cluster's namespaces, those in this kind, and the selection,
    /// so that a deleted namespace stays selected until the user removes it.
    /// The picker clears its search text when given a selection, so it gets one
    /// only if something differs. New options always need one, because it holds
    /// its selection by list position.
    fn sync_namespace_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.namespaces.clone() else {
            return;
        };
        let options: BTreeSet<String> = self
            .cluster_namespaces
            .iter()
            .chain(self.snapshot.namespaces.iter())
            .chain(&self.query.namespaces)
            .cloned()
            .collect();
        let new_options = options != self.namespace_options;
        let selected = &self.query.namespaces;
        picker.update(cx, |picker, cx| {
            if new_options {
                let items = options.iter().cloned().collect::<Vec<_>>();
                picker.set_items(SearchableVec::new(items), window, cx);
            }
            let shown: BTreeSet<String> = picker.selected_values().into_iter().collect();
            if new_options || shown != *selected {
                let values = selected.iter().cloned().collect::<Vec<_>>();
                picker.set_selected_values(&values, window, cx);
            }
        });
        self.namespace_options = options;
    }

    /// Puts the table's selection on the selected object. The table scrolls to
    /// whatever it selects, so an off-screen row is only remembered, and the
    /// view stays where the user put it.
    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected.clone();
        let expected = self.table.update(cx, |table, cx| {
            let delegate = table.delegate();
            let position = selected
                .as_ref()
                .and_then(|key| delegate.rows.iter().position(|row| row.key == *key));
            let on_screen = delegate.visible.is_empty()
                || position.is_some_and(|index| delegate.visible.contains(&index));
            match position.filter(|_| on_screen) {
                Some(index) if table.selected_row() != Some(index) => {
                    table.set_selected_row(index, cx);
                    1
                }
                None if table.selected_row().is_some() => {
                    table.clear_selection(cx);
                    1
                }
                _ => 0,
            }
        });
        self.expected_selection_events += expected;
    }

    /// Asks extensions for visible cells they have not answered. Each column is
    /// one batched background request, so a slow extension delays only its cells.
    fn request_computed(&mut self, cx: &mut Context<Self>) {
        let delegate = self.table.read(cx).delegate();
        if delegate.columns.computed.is_empty() {
            return;
        }
        let end = delegate.visible.end.min(delegate.rows.len());
        let start = delegate.visible.start.min(end);
        let visible: Vec<Arc<Row>> = delegate.rows[start..end].to_vec();
        let columns: Vec<ComputedColumn> = delegate.columns.computed.clone();

        for (index, column) in columns.into_iter().enumerate() {
            let delegate = self.table.read(cx).delegate();
            let wanted: Vec<Arc<Row>> = visible
                .iter()
                .filter(|row| delegate.computed_cell(index, row).is_none())
                .filter(|row| !self.computing.contains(&(index, row.key.clone())))
                .cloned()
                .collect();
            if wanted.is_empty() {
                continue;
            }
            self.computing
                .extend(wanted.iter().map(|row| (index, row.key.clone())));
            let targets = wanted.iter().map(|row| self.object_ref(row)).collect();
            let request = ExtensionRequest::ComputeColumn {
                column: column.id.clone(),
                targets,
            };
            let reply = self.services.extension(&column.extension, request);
            cx.spawn(async move |this, cx| {
                let result = reply.await;
                let _ = this.update(cx, |this, cx| {
                    for row in &wanted {
                        this.computing.remove(&(index, row.key.clone()));
                    }
                    let cells = match result {
                        Ok(ExtensionResponse::Cells { cells }) => cells,
                        // Failures show in the cell rather than retrying forever.
                        other => {
                            tracing::warn!(column = %column.id, ?other, "computed column failed");
                            vec![t!("browser.unavailable").to_string(); wanted.len()]
                        }
                    };
                    this.table.update(cx, |table, cx| {
                        let computed = &mut table.delegate_mut().computed;
                        for (row, text) in wanted.iter().zip(cells) {
                            let cell = ComputedCell {
                                row: row.clone(),
                                text,
                            };
                            computed.insert((index, row.key.clone()), cell);
                        }
                        cx.notify();
                    });
                });
            })
            .detach();
        }
    }

    fn object_ref(&self, row: &Row) -> ObjectRef {
        ObjectRef {
            cluster: ClusterRef(self.key.context.clone()),
            kind: kind_ref(&self.kind),
            namespace: row.namespace.as_deref().map(str::to_owned),
            name: row.name.to_string(),
        }
    }
}

impl Render for ResourceBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let summary = if !snapshot.synced {
            t!("common.loading").to_string()
        } else if snapshot.rows.len() == snapshot.total {
            t!("browser.objects", count = snapshot.total).to_string()
        } else {
            t!(
                "browser.objects_filtered",
                shown = snapshot.rows.len(),
                total = snapshot.total
            )
            .to_string()
        };
        let denied = snapshot.denied.clone();
        let muted = cx.theme().muted_foreground;
        let checked = self.table.read(cx).delegate().checked.len();
        div()
            .id("resource-browser")
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(
                        Input::new(&self.filter).prefix(Icon::new(IconName::Funnel).xsmall()),
                    ))
                    .child(
                        div().flex_1().child(
                            Input::new(&self.labels).prefix(Icon::new(IconName::Tag).xsmall()),
                        ),
                    )
                    .children(self.namespaces.as_ref().map(|picker| {
                        let picker = Combobox::new(picker)
                            .placeholder(t!("browser.all_namespaces").to_string())
                            .search_placeholder(t!("browser.search_namespaces").to_string())
                            .cleanable(true)
                            .when(self.namespaces_incomplete, |this| {
                                this.footer(|_, _| {
                                    div()
                                        .id("namespaces-incomplete")
                                        .p_2()
                                        .text_xs()
                                        .child(t!("browser.namespaces_incomplete").to_string())
                                        .test_support()
                                })
                            });
                        div()
                            .id("namespace-picker")
                            .flex_1()
                            .child(picker)
                            .test_support()
                    })),
            )
            .children(self.label_error.clone().map(|error| {
                div()
                    .id("label-error")
                    .text_xs()
                    .child(error)
                    .test_support()
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("resource-summary")
                            .text_xs()
                            .text_color(muted)
                            .child(summary)
                            .test_support(),
                    )
                    .when(checked > 0, |this| {
                        this.child(
                            div()
                                .id("checked-count")
                                .text_xs()
                                .child(t!("browser.checked", count = checked).to_string())
                                .test_support(),
                        )
                        .children(kind_operation(&self.kind).map(|operation| {
                            let (id, label) = match operation {
                                Operation::Run => ("run-checked", t!("browser.run_checked")),
                                _ => ("restart-checked", t!("browser.restart_checked")),
                            };
                            Button::new(id)
                                .icon(operation.icon())
                                .xsmall()
                                .label(label.to_string())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.emit(BrowserEvent::Confirm(
                                        operation,
                                        this.checked_targets(cx),
                                    ));
                                }))
                        }))
                        .child(
                            Button::new("delete-checked")
                                .icon(IconName::Trash)
                                .xsmall()
                                .label(t!("browser.delete_checked").to_string())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.emit(BrowserEvent::Confirm(
                                        Operation::Delete,
                                        this.checked_targets(cx),
                                    ));
                                })),
                        )
                        .child(
                            Button::new("clear-checked")
                                .ghost()
                                .xsmall()
                                .label(t!("browser.clear_checked").to_string())
                                .on_click(cx.listener(|this, _, _, cx| this.clear_checked(cx))),
                        )
                    }),
            )
            .children(denied.map(|reason| {
                div()
                    .id("access-denied")
                    .child(t!("browser.access_denied", reason = reason).to_string())
                    .test_support()
            }))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    // The table passes Escape on when the open row is off screen.
                    .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            cx.emit(BrowserEvent::Close);
                        }
                    }))
                    .child(DataTable::new(&self.table)),
            )
    }
}
