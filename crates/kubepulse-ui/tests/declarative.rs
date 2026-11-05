use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use gpui_kit::{Entity, SharedString, TestAppContext, VisualTestContext, test::TestWindowExt as _};
use kubepulse_extension_api::{
    Chart, ChartSeries, Description, Direction, FormField, FormFieldKind, Surface, SurfaceKind,
    SurfaceRef, Table, UiNode,
};
use kubepulse_ui::declarative::{DeclarativeView, FormSubmitted};

type Submissions = Rc<RefCell<Vec<(String, BTreeMap<String, String>)>>>;

fn render(
    root: UiNode,
    cx: &mut TestAppContext,
) -> (Entity<DeclarativeView>, Submissions, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let submissions = Submissions::default();
    let recorded = submissions.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let view = DeclarativeView::new(root, window, cx);
        cx.subscribe_self(move |_, submitted: &FormSubmitted, _| {
            recorded
                .borrow_mut()
                .push((submitted.form.clone(), submitted.values.clone()));
        })
        .detach();
        view
    });
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(900.), gpui_kit::px(1600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    (view, submissions, cx)
}

fn count(id: &'static str, cx: &mut VisualTestContext) -> usize {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.find_all(id).len()
    })
}

fn click(id: String, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.click(SharedString::from(id), cx));
    cx.run_until_parked();
}

fn text(text: &str) -> UiNode {
    UiNode::Text { text: text.into() }
}

fn stack(children: Vec<UiNode>) -> UiNode {
    UiNode::Stack {
        direction: Direction::Vertical,
        children,
    }
}

fn surface(kind: SurfaceKind) -> UiNode {
    UiNode::Surface {
        surface: Surface {
            kind,
            reference: SurfaceRef("s".into()),
        },
    }
}

fn form() -> UiNode {
    UiNode::Form {
        id: "scale".into(),
        submit_label: "Apply".into(),
        fields: vec![
            FormField {
                id: "name".into(),
                label: "Name".into(),
                kind: FormFieldKind::Text {
                    value: "web".into(),
                },
            },
            FormField {
                id: "force".into(),
                label: "Force".into(),
                kind: FormFieldKind::Toggle { value: false },
            },
            FormField {
                id: "mode".into(),
                label: "Mode".into(),
                kind: FormFieldKind::Choice {
                    options: vec!["fast".into(), "safe".into()],
                    value: "fast".into(),
                },
            },
        ],
    }
}

fn table(rows: Vec<Vec<String>>) -> UiNode {
    UiNode::Table {
        table: Table {
            columns: vec!["A".into(), "B".into()],
            rows,
        },
    }
}

fn chart(series: Vec<ChartSeries>) -> UiNode {
    UiNode::Chart {
        chart: Chart {
            title: "CPU".into(),
            series,
        },
    }
}

#[gpui_kit::test]
fn each_element_renders(cx: &mut TestAppContext) {
    let root = stack(vec![
        text("hello"),
        stack(vec![text("nested")]),
        table(vec![vec!["1".into(), "2".into()]]),
        UiNode::DescriptionList {
            items: vec![Description {
                label: "Node".into(),
                value: "a".into(),
            }],
        },
        form(),
        chart(vec![ChartSeries {
            name: "used".into(),
            points: vec![(0., 1.), (1., 2.)],
        }]),
        surface(SurfaceKind::Log),
        surface(SurfaceKind::Terminal),
        surface(SurfaceKind::Editor),
        UiNode::Unknown,
    ]);
    let (_, _, cx) = render(root, cx);

    for (id, expected) in [
        ("element-text", 2),
        ("element-stack", 2),
        ("element-table", 1),
        ("element-description-list", 1),
        ("element-form", 1),
        ("element-chart", 1),
        ("element-surface-log", 1),
        ("element-surface-terminal", 1),
        ("element-surface-editor", 1),
        ("element-unknown", 1),
    ] {
        assert_eq!(count(id, cx), expected, "{id}");
    }
}

#[gpui_kit::test]
fn an_element_from_a_newer_contract_becomes_a_placeholder_beside_its_siblings(
    cx: &mut TestAppContext,
) {
    let root: UiNode = serde_json::from_value(serde_json::json!({
        "element": "stack",
        "direction": "vertical",
        "children": [
            {"element": "hologram", "depth": 3},
            {"element": "text", "text": "still here"},
        ],
    }))
    .unwrap();
    let (_, _, cx) = render(root, cx);

    assert_eq!(count("element-unknown", cx), 1);
    assert_eq!(count("element-text", cx), 1);
}

#[gpui_kit::test]
fn tables_show_their_rows_and_tolerate_short_ones(cx: &mut TestAppContext) {
    let rows = vec![vec!["1".into(), "2".into()], vec!["3".into()]];
    let (view, _, cx) = render(table(rows), cx);

    let (headers, shown) = cx.update(|_, cx| view.read(cx).tables()[0].read(cx).dump(cx));
    assert_eq!(headers, ["A", "B"]);
    assert_eq!(shown, [["1", "2"], ["3", ""]]);
}

#[gpui_kit::test]
fn charts_without_data_still_render(cx: &mut TestAppContext) {
    let empty = ChartSeries {
        name: "none".into(),
        points: vec![],
    };
    let (_, _, cx) = render(
        stack(vec![stack(vec![chart(vec![])]), chart(vec![empty])]),
        cx,
    );

    assert_eq!(count("element-chart", cx), 2);
}

#[gpui_kit::test]
fn a_form_submits_its_starting_values(cx: &mut TestAppContext) {
    let (_, submissions, cx) = render(form(), cx);

    click("submit-scale".into(), cx);

    let expected = BTreeMap::from(
        [("force", "false"), ("mode", "fast"), ("name", "web")]
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
    assert_eq!(*submissions.borrow(), [("scale".to_owned(), expected)]);
}

#[gpui_kit::test]
fn a_form_submits_what_the_user_changed(cx: &mut TestAppContext) {
    let (_, submissions, cx) = render(form(), cx);

    click("field-scale-force".into(), cx);
    click("field-scale-mode-safe".into(), cx);
    click("submit-scale".into(), cx);

    let submitted = submissions.borrow();
    assert_eq!(submitted[0].1["force"], "true");
    assert_eq!(submitted[0].1["mode"], "safe");
}
