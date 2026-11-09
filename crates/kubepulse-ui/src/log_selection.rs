//! Bridges the virtualized log list to the window text selection. Content keys
//! name a line and a byte offset into its shown text, so selecting and copying
//! do not depend on which rows are painted.

use std::{cell::Cell, cell::RefCell, ops::Range, rc::Rc};

use gpui_kit::{
    base::{ScrollbarHandle, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun},
    *,
};

const OFFSET_BITS: u32 = 16;

pub fn encode_key(seq: u64, offset: usize) -> u64 {
    debug_assert!(offset < 1 << OFFSET_BITS);
    seq << OFFSET_BITS | offset as u64
}

pub fn decode_key(key: u64) -> (u64, usize) {
    (
        key >> OFFSET_BITS,
        (key & ((1 << OFFSET_BITS) - 1)) as usize,
    )
}

/// The selected keys of `handle`, in order, when both ends are in it.
pub fn selected_keys(handle: &TextSelectionHandle, cx: &App) -> Option<Range<u64>> {
    let snapshot = handle.snapshot(cx)?;
    let ours = Some(handle.entity_id());
    if snapshot.anchor().entity_id() != ours || snapshot.cursor().entity_id() != ours {
        return None;
    }
    let anchor = snapshot.anchor().content_key()?.value();
    let cursor = snapshot.cursor().content_key()?.value();
    (anchor != cursor).then(|| anchor.min(cursor)..anchor.max(cursor))
}

/// The part of line `seq`, whose shown text is `len` bytes, that `keys` cover.
pub fn line_range(keys: &Range<u64>, seq: u64, len: usize) -> Option<Range<usize>> {
    let (start_seq, start) = decode_key(keys.start);
    let (end_seq, end) = decode_key(keys.end);
    if seq < start_seq || seq > end_seq {
        return None;
    }
    let start = if seq == start_seq { start.min(len) } else { 0 };
    let end = if seq == end_seq { end.min(len) } else { len };
    (start <= end).then_some(start..end)
}

pub enum Scroll {
    Rows(UniformListScrollHandle),
    Wrapped(ListState),
}

impl Scroll {
    fn offset(&self) -> Point<Pixels> {
        match self {
            Self::Rows(handle) => ScrollbarHandle::offset(handle),
            Self::Wrapped(state) => ScrollbarHandle::offset(state),
        }
    }
}

struct Run {
    seq: u64,
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

#[derive(Default)]
pub struct Geometry {
    bounds: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
    /// A double or triple click, its position, and whether its start was
    /// resolved already.
    multi_click: Cell<Option<(usize, Point<Pixels>, bool)>>,
    runs: Vec<Run>,
}

impl Geometry {
    pub fn key_at(&self, point: Point<Pixels>) -> Option<u64> {
        if let Some((count, position, start_done)) = self.multi_click.get() {
            let run = self.run_at(position)?;
            let offset = run
                .layout
                .index_for_position(position)
                .unwrap_or_else(|offset| offset);
            let range = if count >= 3 {
                0..run.text.len()
            } else {
                word_range(&run.text, offset)
            };
            self.multi_click
                .set((!start_done).then_some((count, position, true)));
            let offset = if start_done { range.end } else { range.start };
            return Some(encode_key(run.seq, offset));
        }
        let point = point + self.bounds.origin + self.scroll_offset;
        let run = self.run_at(point)?;
        let offset = run
            .layout
            .index_for_position(point)
            .unwrap_or_else(|offset| offset);
        Some(encode_key(run.seq, offset))
    }

    /// The row under `point`, or the nearest one above or below it.
    fn run_at(&self, point: Point<Pixels>) -> Option<&Run> {
        self.runs.iter().min_by_key(|run| {
            let bounds = run.bounds;
            let distance = if point.y < bounds.top() {
                bounds.top() - point.y
            } else if point.y >= bounds.bottom() {
                point.y - bounds.bottom() + px(0.5)
            } else {
                px(0.)
            };
            f32::from(distance).to_bits()
        })
    }
}

/// The word at `offset`: a run of letters, digits, and underscores, a run of
/// whitespace, or one other character.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let Some(character) = text[offset..].chars().next() else {
        return offset..offset;
    };
    let class = |character: char| {
        if character == '_' || character.is_alphanumeric() {
            1
        } else if character.is_whitespace() {
            2
        } else {
            0
        }
    };
    let kind = class(character);
    let end = offset + character.len_utf8();
    if kind == 0 {
        return offset..end;
    }
    let same = |character: &char| class(*character) == kind;
    let start = text[..offset]
        .chars()
        .rev()
        .take_while(same)
        .fold(offset, |offset, character| offset - character.len_utf8());
    let end = text[end..]
        .chars()
        .take_while(same)
        .fold(end, |offset, character| offset + character.len_utf8());
    start..end
}

/// The text of one log line, reporting its layout to the selection.
pub struct LineText {
    seq: u64,
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    geometry: Rc<RefCell<Geometry>>,
    styled: Option<StyledText>,
}

impl LineText {
    pub fn new(
        seq: u64,
        text: SharedString,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        geometry: Rc<RefCell<Geometry>>,
    ) -> Self {
        Self {
            seq,
            text,
            highlights,
            geometry,
            styled: None,
        }
    }
}

impl IntoElement for LineText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for LineText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let highlights = std::mem::take(&mut self.highlights);
        let styled = self
            .styled
            .insert(StyledText::new(self.text.clone()).with_highlights(highlights));
        styled.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(styled) = self.styled.as_mut() else {
            return;
        };
        styled.prepaint(id, inspector, bounds, &mut (), window, cx);
        self.geometry.borrow_mut().runs.push(Run {
            seq: self.seq,
            text: self.text.clone(),
            layout: styled.layout().clone(),
            bounds,
        });
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(styled) = self.styled.as_mut() {
            styled.paint(id, inspector, bounds, &mut (), &mut (), window, cx);
        }
    }
}

/// The area holding the log rows, which takes part in the selection.
pub struct SelectionArea {
    child: AnyElement,
    scroll: Scroll,
    selection: TextSelectionHandle,
    geometry: Rc<RefCell<Geometry>>,
}

impl SelectionArea {
    pub fn new(
        child: AnyElement,
        scroll: Scroll,
        selection: TextSelectionHandle,
        geometry: Rc<RefCell<Geometry>>,
    ) -> Self {
        Self {
            child,
            scroll,
            selection,
            geometry,
        }
    }
}

impl IntoElement for SelectionArea {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for SelectionArea {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::NamedInteger(
            "log-selection".into(),
            self.selection.entity_id().as_u64(),
        ))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        {
            let mut geometry = self.geometry.borrow_mut();
            geometry.runs.clear();
            geometry.bounds = bounds;
        }
        // Drag auto-scroll measures against the hitbox's mask, so the mask is
        // the rows' own area rather than the whole tab.
        let hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        });
        self.child.prepaint(window, cx);
        let offset = self.scroll.offset();
        self.geometry.borrow_mut().scroll_offset = offset;
        let text_bounds = self
            .geometry
            .borrow()
            .runs
            .iter()
            .map(|run| run.bounds)
            .collect();
        self.selection.register(
            TextSelectionRegistration::new(hitbox, bounds)
                .with_scroll_offset(offset)
                .with_text_bounds(text_bounds)
                .with_rendered_element(&self.selection, window, cx),
            window,
            cx,
        );
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let runs: Vec<TextSelectionRun> = self
            .geometry
            .borrow()
            .runs
            .iter()
            .map(|run| {
                TextSelectionRun::new(run.text.clone(), run.layout.clone(), run.bounds)
                    .with_document_order(encode_key(run.seq, 0))
            })
            .collect();
        self.selection.update_runs(&runs, cx);
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
            if phase.capture() && event.button == MouseButton::Left {
                geometry
                    .borrow()
                    .multi_click
                    .set((event.click_count >= 2).then_some((
                        event.click_count,
                        event.position,
                        false,
                    )));
            }
        });
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, _, _| {
            if phase.capture() {
                geometry.borrow().multi_click.set(None);
            }
        });
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
            if phase.capture() {
                geometry.borrow().multi_click.set(None);
            }
        });
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_key, encode_key, line_range, word_range};

    #[test]
    fn a_key_round_trips() {
        let key = encode_key(123_456_789, 4_321);
        assert_eq!(decode_key(key), (123_456_789, 4_321));
    }

    #[test]
    fn keys_order_by_line_then_offset() {
        assert!(encode_key(1, 60_000) < encode_key(2, 0));
        assert!(encode_key(2, 3) < encode_key(2, 4));
    }

    #[test]
    fn a_selection_covers_the_middle_lines_whole() {
        let keys = encode_key(3, 4)..encode_key(5, 2);
        assert_eq!(line_range(&keys, 2, 10), None);
        assert_eq!(line_range(&keys, 3, 10), Some(4..10));
        assert_eq!(line_range(&keys, 4, 10), Some(0..10));
        assert_eq!(line_range(&keys, 5, 10), Some(0..2));
        assert_eq!(line_range(&keys, 6, 10), None);
        assert_eq!(
            line_range(&keys, 4, 0),
            Some(0..0),
            "an empty line still counts"
        );
    }

    #[test]
    fn a_word_is_letters_digits_and_underscores() {
        let text = "id=req_42 done";
        assert_eq!(&text[word_range(text, 5)], "req_42");
        assert_eq!(&text[word_range(text, 2)], "=");
        assert_eq!(&text[word_range(text, 9)], " ");
    }
}
