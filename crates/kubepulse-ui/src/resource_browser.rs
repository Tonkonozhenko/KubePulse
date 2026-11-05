//! The table for one kind. Each browser has its own view of the shared store,
//! so two tabs keep their own filter, sort, and scroll position.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use gpui_kit::{
    component::{
        ActiveTheme as _,
        input::{Input, InputEvent, InputState},
        table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
    },
    *,
};
use kubepulse_cluster::ResourceKind;
use kubepulse_extension_api::{ClusterRef, ExtensionRequest, ExtensionResponse, ObjectRef};
use kubepulse_store::{
    Cell, LabelSelector, Row, Snapshot, SortDirection, SortKey, View, ViewQuery,
};

use crate::{
    columns::{ComputedColumn, KindColumns, kind_ref},
    services::ClusterServices,
};

const PENDING_CELL: &str = "…";

fn cell_text(cell: Option<&Cell>) -> String {
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
}

impl RowsDelegate {
    fn leading_columns(&self) -> usize {
        if self.namespaced { 2 } else { 1 }
    }

    fn sort_key(&self, col_ix: usize) -> Option<SortKey> {
        let lead = self.leading_columns();
        match col_ix {
            0 => Some(SortKey::Name),
            1 if self.namespaced => Some(SortKey::Namespace),
            index if index - lead < self.columns.declared.len() => {
                Some(SortKey::Column(index - lead))
            }
            _ => None,
        }
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

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().child(self.cell_text(row_ix, col_ix, cx))
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        let Some(row) = self.rows.get(row_ix) else {
            return String::new();
        };
        let lead = self.leading_columns();
        let declared = self.columns.declared.len();
        match col_ix {
            0 => row.name.to_string(),
            1 if self.namespaced => row.namespace.as_deref().unwrap_or_default().to_owned(),
            index if index - lead < declared => cell_text(row.cells.get(index - lead)),
            index => self
                .computed_cell(index - lead - declared, row)
                .unwrap_or(PENDING_CELL)
                .to_owned(),
        }
    }
}

pub enum BrowserEvent {
    Open(Arc<Row>),
}

impl EventEmitter<BrowserEvent> for ResourceBrowser {}

pub struct ResourceBrowser {
    context: String,
    kind: ResourceKind,
    services: Arc<dyn ClusterServices>,
    view: View,
    query: ViewQuery,
    table: Entity<TableState<RowsDelegate>>,
    filter: Entity<InputState>,
    labels: Entity<InputState>,
    namespaces: Entity<InputState>,
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
        context: &str,
        kind: ResourceKind,
        columns: KindColumns,
        services: Arc<dyn ClusterServices>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Self>> {
        let view = services.open_view(context, &kind, ViewQuery::default())?;
        services.kind_opened(kind_ref(&kind));
        let context = context.to_owned();
        Some(cx.new(|cx| Self::new(context, kind, columns, services, view, window, cx)))
    }

    fn new(
        context: String,
        kind: ResourceKind,
        columns: KindColumns,
        services: Arc<dyn ClusterServices>,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut table_columns = vec![Column::new("name", "Name").sortable()];
        if kind.namespaced {
            table_columns.push(Column::new("namespace", "Namespace").sortable());
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
        };
        let table = cx.new(|cx| TableState::new(delegate, window, cx));

        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let labels = cx.new(|cx| InputState::new(window, cx).placeholder("Label selector"));
        let namespaces =
            cx.new(|cx| InputState::new(window, cx).placeholder("Namespaces, comma separated"));
        let subscriptions = vec![
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
            cx.subscribe(&namespaces, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.set_namespaces(&text, cx);
                }
            }),
        ];

        let mut snapshots = view.subscribe();
        cx.spawn(async move |this, cx| {
            loop {
                let latest = snapshots.borrow_and_update().clone();
                let applied = this.update(cx, |this, cx| this.apply_snapshot(latest, cx));
                if applied.is_err() || snapshots.changed().await.is_err() {
                    return;
                }
            }
        })
        .detach();

        Self {
            context,
            kind,
            services,
            view,
            query: ViewQuery::default(),
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
                Err(error) => self.label_error = Some(error.to_string()),
            }
        }
        self.apply_query(cx);
    }

    pub fn set_namespaces(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.namespaces = text
            .split(',')
            .map(str::trim)
            .filter(|namespace| !namespace.is_empty())
            .map(str::to_owned)
            .collect();
        self.apply_query(cx);
    }

    pub fn set_sort(&mut self, sort: Option<(SortKey, SortDirection)>, cx: &mut Context<Self>) {
        self.query.sort = sort;
        self.apply_query(cx);
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
            TableEvent::ClearSelection => self.selected = None,
            _ => {}
        }
    }

    fn apply_snapshot(&mut self, snapshot: Arc<Snapshot>, cx: &mut Context<Self>) {
        self.snapshot = snapshot.clone();
        self.table.update(cx, |table, cx| {
            table.delegate_mut().rows = snapshot.rows.clone();
            cx.notify();
        });
        self.sync_selection(cx);
        self.request_computed(cx);
        cx.notify();
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
                            vec!["unavailable".to_owned(); wanted.len()]
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
            cluster: ClusterRef(self.context.clone()),
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
            "Loading…".to_owned()
        } else if snapshot.rows.len() == snapshot.total {
            format!("{} objects", snapshot.total)
        } else {
            format!("{} of {} objects", snapshot.rows.len(), snapshot.total)
        };
        let denied = snapshot.denied.clone();
        let muted = cx.theme().muted_foreground;
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
                    .child(div().flex_1().child(Input::new(&self.filter)))
                    .child(div().flex_1().child(Input::new(&self.labels)))
                    .child(div().flex_1().child(Input::new(&self.namespaces))),
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
                    .id("resource-summary")
                    .text_xs()
                    .text_color(muted)
                    .child(summary)
                    .test_support(),
            )
            .children(denied.map(|reason| {
                div()
                    .id("access-denied")
                    .child(format!("Access denied: {reason}"))
                    .test_support()
            }))
            .child(div().flex_1().w_full().child(DataTable::new(&self.table)))
    }
}
