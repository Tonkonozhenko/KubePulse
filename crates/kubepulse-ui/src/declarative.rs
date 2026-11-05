//! Renders an extension's declarative interface with the host's own components.
//! An element this build does not know becomes a placeholder, so one unknown
//! element never takes down the rest of the view.

use std::collections::{BTreeMap, HashMap};

use gpui_kit::{
    component::{
        ActiveTheme as _,
        button::Button,
        chart::LineChart,
        description_list::DescriptionList,
        input::{Input, InputState},
        table::{Column, DataTable, TableDelegate, TableState},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_extension_api::{FormFieldKind, SurfaceKind, UiNode};

pub struct FormSubmitted {
    pub form: String,
    pub values: BTreeMap<String, String>,
}

impl EventEmitter<FormSubmitted> for DeclarativeView {}

pub struct StringTable {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
}

impl TableDelegate for StringTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
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
        self.rows[row_ix].get(col_ix).cloned().unwrap_or_default()
    }
}

enum FieldState {
    Text(Entity<InputState>),
    Toggle(bool),
    Choice { options: Vec<String>, value: String },
}

pub struct DeclarativeView {
    root: UiNode,
    tables: Vec<Entity<TableState<StringTable>>>,
    fields: HashMap<(String, String), FieldState>,
}

impl DeclarativeView {
    pub fn new(root: UiNode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            root: root.clone(),
            tables: Vec::new(),
            fields: HashMap::new(),
        };
        this.prepare(&root, window, cx);
        this
    }

    /// Builds the state that outlives a frame, in tree order.
    fn prepare(&mut self, node: &UiNode, window: &mut Window, cx: &mut Context<Self>) {
        match node {
            UiNode::Stack { children, .. } => {
                children
                    .iter()
                    .for_each(|child| self.prepare(child, window, cx));
            }
            UiNode::Table { table } => {
                let delegate = StringTable {
                    columns: table
                        .columns
                        .iter()
                        .map(|name| Column::new(name.clone(), name.clone()))
                        .collect(),
                    rows: table.rows.clone(),
                };
                self.tables
                    .push(cx.new(|cx| TableState::new(delegate, window, cx)));
            }
            UiNode::Form { id, fields, .. } => {
                for field in fields {
                    let state = match &field.kind {
                        FormFieldKind::Text { value } => {
                            let value = value.clone();
                            FieldState::Text(cx.new(|cx| {
                                let mut input = InputState::new(window, cx);
                                input.set_value(value, window, cx);
                                input
                            }))
                        }
                        FormFieldKind::Toggle { value } => FieldState::Toggle(*value),
                        FormFieldKind::Choice { options, value } => FieldState::Choice {
                            options: options.clone(),
                            value: value.clone(),
                        },
                    };
                    self.fields.insert((id.clone(), field.id.clone()), state);
                }
            }
            _ => {}
        }
    }

    pub fn tables(&self) -> &[Entity<TableState<StringTable>>] {
        &self.tables
    }

    fn submit(&self, form: &str, cx: &mut Context<Self>) {
        let values = self
            .fields
            .iter()
            .filter(|((owner, _), _)| owner == form)
            .map(|((_, field), state)| {
                let value = match state {
                    FieldState::Text(input) => input.read(cx).value().to_string(),
                    FieldState::Toggle(value) => value.to_string(),
                    FieldState::Choice { value, .. } => value.clone(),
                };
                (field.clone(), value)
            })
            .collect();
        cx.emit(FormSubmitted {
            form: form.to_owned(),
            values,
        });
    }

    fn render_field(
        &self,
        form: &str,
        field: &str,
        label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = (form.to_owned(), field.to_owned());
        let element_id = format!("field-{form}-{field}");
        let body = match self.fields.get(&key) {
            Some(FieldState::Text(input)) => Input::new(input).into_any_element(),
            Some(FieldState::Toggle(value)) => {
                let value = *value;
                Button::new(element_id.clone())
                    .label(if value { "On" } else { "Off" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(FieldState::Toggle(state)) = this.fields.get_mut(&key) {
                            *state = !value;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Some(FieldState::Choice { options, value }) => {
                let buttons = options.iter().map(|option| {
                    let (key, chosen) = (key.clone(), option.clone());
                    let marker = if option == value { "● " } else { "" };
                    Button::new(format!("{element_id}-{option}"))
                        .label(format!("{marker}{option}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(FieldState::Choice { value, .. }) =
                                this.fields.get_mut(&key)
                            {
                                *value = chosen.clone();
                            }
                            cx.notify();
                        }))
                });
                div().flex().gap_1().children(buttons).into_any_element()
            }
            None => div().into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(label.to_owned())
            .child(body)
            .into_any_element()
    }

    fn render_node(&self, node: &UiNode, tables: &mut usize, cx: &mut Context<Self>) -> AnyElement {
        match node {
            UiNode::Text { text } => div()
                .id("element-text")
                .child(text.clone())
                .test_support()
                .into_any_element(),
            UiNode::Stack {
                direction,
                children,
            } => {
                let vertical = matches!(direction, kubepulse_extension_api::Direction::Vertical);
                let children: Vec<AnyElement> = children
                    .iter()
                    .map(|child| self.render_node(child, tables, cx))
                    .collect();
                div()
                    .id("element-stack")
                    .flex()
                    .when(vertical, |this| this.flex_col())
                    .gap_2()
                    .children(children)
                    .test_support()
                    .into_any_element()
            }
            UiNode::Table { .. } => {
                let table = self.tables[*tables].clone();
                *tables += 1;
                div()
                    .id("element-table")
                    .h(px(240.))
                    .w_full()
                    .child(DataTable::new(&table))
                    .test_support()
                    .into_any_element()
            }
            UiNode::DescriptionList { items } => {
                let list = items
                    .iter()
                    .fold(DescriptionList::new().columns(1), |list, item| {
                        list.item(item.label.clone(), item.value.clone(), 1)
                    });
                div()
                    .id("element-description-list")
                    .child(list)
                    .test_support()
                    .into_any_element()
            }
            UiNode::Form {
                id,
                fields,
                submit_label,
            } => {
                let form = id.clone();
                let rows: Vec<AnyElement> = fields
                    .iter()
                    .map(|field| self.render_field(id, &field.id, &field.label, cx))
                    .collect();
                div()
                    .id("element-form")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(rows)
                    .child(
                        Button::new(format!("submit-{id}"))
                            .label(submit_label.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.submit(&form, cx))),
                    )
                    .test_support()
                    .into_any_element()
            }
            UiNode::Chart { chart } => {
                let series: Vec<AnyElement> = chart
                    .series
                    .iter()
                    .map(|series| {
                        div()
                            .h(px(160.))
                            .w_full()
                            .child(
                                LineChart::new(series.points.clone())
                                    .name(series.name.clone())
                                    .x(|point: &(f64, f64)| SharedString::from(point.0.to_string()))
                                    .y(|point: &(f64, f64)| point.1),
                            )
                            .into_any_element()
                    })
                    .collect();
                div()
                    .id("element-chart")
                    .flex()
                    .flex_col()
                    .w_full()
                    .gap_1()
                    .child(chart.title.clone())
                    .children(series)
                    .test_support()
                    .into_any_element()
            }
            UiNode::Surface { surface } => {
                let (id, label) = match surface.kind {
                    SurfaceKind::Log => ("element-surface-log", "Log surface"),
                    SurfaceKind::Terminal => ("element-surface-terminal", "Terminal surface"),
                    SurfaceKind::Editor => ("element-surface-editor", "Editor surface"),
                };
                div().id(id).child(label).test_support().into_any_element()
            }
            UiNode::Unknown => div()
                .id("element-unknown")
                .p_2()
                .border_1()
                .border_color(cx.theme().danger)
                .child("This element is not supported by this version.")
                .test_support()
                .into_any_element(),
        }
    }
}

impl Render for DeclarativeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.root.clone();
        let mut tables = 0;
        self.render_node(&root, &mut tables, cx)
    }
}
