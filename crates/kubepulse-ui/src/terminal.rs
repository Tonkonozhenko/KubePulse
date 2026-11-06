//! The terminal emulator behind a shell tab. Parsing runs off the UI thread, and
//! the view reads a [`Frame`] of what to draw, so nothing here touches a window.

use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{
        Config, Osc52, TermMode,
        cell::{Cell, Flags},
        color::Colors,
    },
    vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb},
};
use gpui_kit::{Hsla, Keystroke, Rgba, component::Theme};

const SCROLLBACK_LINES: usize = 10_000;
const DIM_OPACITY: f32 = 0.7;
/// Puts the terminal back in its normal modes without touching the scrollback,
/// because a program that lost its connection never restored them itself.
const RESTORE_MODES: &[u8] = b"\x1b[?1049l\x1b[0m\x1b[r\x1b[?25h\x1b[?1l\x1b>\
\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\r\n";

/// The colors the terminal draws with. Programs choose among the named ones, so
/// they follow the active theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    ansi: [Hsla; 16],
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_theme(&Theme::default())
    }
}

impl Palette {
    /// The theme has no black or white, so they are its darker and lighter text
    /// colors, which keeps both readable in light and dark themes.
    pub fn from_theme(theme: &Theme) -> Self {
        let (mut black, mut white) = (theme.background, theme.foreground);
        if black.l > white.l {
            (black, white) = (white, black);
        }
        Self {
            ansi: [
                black,
                theme.red,
                theme.green,
                theme.yellow,
                theme.blue,
                theme.magenta,
                theme.cyan,
                white,
                theme.muted_foreground,
                theme.red_light,
                theme.green_light,
                theme.yellow_light,
                theme.blue_light,
                theme.magenta_light,
                theme.cyan_light,
                white,
            ],
            foreground: theme.foreground,
            background: theme.background,
            cursor: theme.caret,
            selection: theme.selection,
        }
    }

    fn named(&self, name: NamedColor) -> Hsla {
        match name {
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            _ => self
                .ansi
                .get(name as usize)
                .copied()
                .unwrap_or(self.foreground),
        }
    }

    fn indexed(&self, index: u8) -> Hsla {
        match self.ansi.get(usize::from(index)) {
            Some(color) => *color,
            None => hsla_of(extended_color(index)),
        }
    }

    /// Resolves a color, honoring any a program has redefined.
    fn resolve(&self, color: Color, redefined: &Colors) -> Hsla {
        match color {
            Color::Spec(color) => hsla_of(color),
            Color::Named(name) => redefined[name].map_or_else(|| self.named(name), hsla_of),
            Color::Indexed(index) => {
                redefined[usize::from(index)].map_or_else(|| self.indexed(index), hsla_of)
            }
        }
    }

    fn rgb_at(&self, index: usize, redefined: &Colors) -> Rgb {
        if let Some(color) = redefined[index] {
            return color;
        }
        let color = match index {
            256 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            _ => self.indexed(u8::try_from(index).unwrap_or(0)),
        };
        let color = Rgba::from(color);
        let channel = |value: f32| (value * 255.).round() as u8;
        Rgb {
            r: channel(color.r),
            g: channel(color.g),
            b: channel(color.b),
        }
    }
}

fn hsla_of(color: Rgb) -> Hsla {
    let channel = |value: u8| f32::from(value) / 255.;
    Rgba {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
        a: 1.,
    }
    .into()
}

/// The standard colors above 15: a 6×6×6 cube, then a grey ramp.
fn extended_color(index: u8) -> Rgb {
    if index >= 232 {
        let level = 8 + (index - 232) * 10;
        return Rgb {
            r: level,
            g: level,
            b: level,
        };
    }
    let cube = index - 16;
    let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
    Rgb {
        r: level(cube / 36),
        g: level(cube / 6 % 6),
        b: level(cube % 6),
    }
}

#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// What the emulator wants to say back, which it does while parsing and so
/// cannot read the rest of the terminal.
struct Shared {
    replies: Vec<u8>,
    palette: Palette,
    colors: Colors,
    window: WindowSize,
}

#[derive(Clone)]
struct Listener(Arc<Mutex<Shared>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut shared = self.0.lock().expect("terminal replies lock");
        match event {
            Event::PtyWrite(text) => shared.replies.extend_from_slice(text.as_bytes()),
            Event::ColorRequest(index, format) => {
                let color = shared.palette.rgb_at(index, &shared.colors);
                shared.replies.extend_from_slice(format(color).as_bytes());
            }
            Event::TextAreaSizeRequest(format) => {
                let text = format(shared.window);
                shared.replies.extend_from_slice(text.as_bytes());
            }
            _ => {}
        }
    }
}

struct Screen {
    term: Term<Listener>,
    parser: Processor,
}

/// A stretch of cells with one background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub line: usize,
    pub column: usize,
    pub cells: usize,
    pub color: Hsla,
}

/// A stretch of text with one style. Wide characters get a span of their own,
/// so every other character takes exactly one cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub cells: usize,
    pub text: String,
    pub color: Hsla,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorMark {
    /// Drawn behind the text of its cell, as part of the frame's colors.
    Filled,
    Outline,
    Underline,
    Beam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
    pub mark: CursorMark,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub blocks: Vec<Block>,
    pub spans: Vec<Span>,
    pub cursor: Option<Cursor>,
}

impl Frame {
    fn fill(&mut self, line: usize, column: usize, cells: usize, color: Hsla) {
        if let Some(last) = self.blocks.last_mut()
            && last.line == line
            && last.column + last.cells == column
            && last.color == color
        {
            last.cells += cells;
            return;
        }
        self.blocks.push(Block {
            line,
            column,
            cells,
            color,
        });
    }

    fn write(&mut self, span: Span) {
        if let Some(last) = self.spans.last_mut()
            && span.cells == 1
            && last.line == span.line
            && last.column + last.cells == span.column
            && last.color == span.color
            && (last.bold, last.italic, last.underline, last.strikeout)
                == (span.bold, span.italic, span.underline, span.strikeout)
            && last.text.chars().count() == last.cells
        {
            last.cells += 1;
            last.text.push_str(&span.text);
            return;
        }
        self.spans.push(span);
    }
}

fn cell_text(cell: &Cell) -> String {
    let mut text = String::from(cell.c);
    if let Some(combining) = cell.zerowidth() {
        text.extend(combining);
    }
    text
}

fn cursor_mark(shape: CursorShape, focused: bool) -> Option<CursorMark> {
    match shape {
        CursorShape::Hidden => None,
        _ if !focused => Some(CursorMark::Outline),
        CursorShape::Block => Some(CursorMark::Filled),
        CursorShape::HollowBlock => Some(CursorMark::Outline),
        CursorShape::Underline => Some(CursorMark::Underline),
        CursorShape::Beam => Some(CursorMark::Beam),
    }
}

pub struct Emulator {
    screen: Mutex<Screen>,
    shared: Arc<Mutex<Shared>>,
}

impl Emulator {
    pub fn new(cols: u16, rows: u16, palette: Palette) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            replies: Vec::new(),
            palette,
            colors: Colors::default(),
            window: WindowSize {
                num_lines: rows,
                num_cols: cols,
                cell_width: 0,
                cell_height: 0,
            },
        }));
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            // A container has no business reading or writing the clipboard.
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let size = GridSize {
            cols: usize::from(cols),
            rows: usize::from(rows),
        };
        let screen = Screen {
            term: Term::new(config, &size, Listener(shared.clone())),
            parser: Processor::new(),
        };
        Self {
            screen: Mutex::new(screen),
            shared,
        }
    }

    fn screen(&self) -> MutexGuard<'_, Screen> {
        self.screen.lock().expect("terminal lock")
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().expect("terminal replies lock")
    }

    /// Takes in output from the container, and returns what it asked to have
    /// sent back, such as the answer to a cursor position query.
    pub fn advance(&self, bytes: &[u8]) -> Vec<u8> {
        let mut screen = self.screen();
        let Screen { term, parser } = &mut *screen;
        self.shared().colors = *term.colors();
        parser.advance(term, bytes);
        drop(screen);
        std::mem::take(&mut self.shared().replies)
    }

    /// Ends what a lost session left behind, ready for the next one.
    pub fn restore_modes(&self) {
        self.advance(RESTORE_MODES);
    }

    pub fn set_palette(&self, palette: Palette) {
        self.shared().palette = palette;
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.screen().term.resize(GridSize {
            cols: usize::from(cols),
            rows: usize::from(rows),
        });
        let mut shared = self.shared();
        shared.window.num_cols = cols;
        shared.window.num_lines = rows;
    }

    pub fn mode(&self) -> TermMode {
        *self.screen().term.mode()
    }

    /// Moves through the scrollback. Positive goes back in time.
    pub fn scroll(&self, lines: i32) {
        self.screen().term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_end(&self) {
        self.screen().term.scroll_display(Scroll::Bottom);
    }

    pub fn scrollback_lines(&self) -> usize {
        self.screen().term.grid().history_size()
    }

    pub fn display_offset(&self) -> usize {
        self.screen().term.grid().display_offset()
    }

    fn point(term: &Term<Listener>, line: usize, column: usize) -> Point {
        let row = i32::try_from(line).unwrap_or(i32::MAX);
        let offset = i32::try_from(term.grid().display_offset()).unwrap_or(i32::MAX);
        let last_column = term.grid().columns() - 1;
        Point::new(Line(row - offset), Column(column.min(last_column)))
    }

    pub fn start_selection(&self, kind: SelectionType, line: usize, column: usize, side: Side) {
        let mut screen = self.screen();
        let point = Self::point(&screen.term, line, column);
        screen.term.selection = Some(Selection::new(kind, point, side));
    }

    pub fn extend_selection(&self, line: usize, column: usize, side: Side) {
        let mut screen = self.screen();
        let point = Self::point(&screen.term, line, column);
        if let Some(selection) = &mut screen.term.selection {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.screen().term.selection = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        let text = self.screen().term.selection_to_string()?;
        (!text.is_empty()).then_some(text)
    }

    /// The lines on screen, with trailing spaces removed.
    pub fn visible_lines(&self) -> Vec<String> {
        let screen = self.screen();
        let content = screen.term.renderable_content();
        let offset = content.display_offset as i32;
        let mut lines = vec![String::new(); screen.term.screen_lines()];
        for cell in content.display_iter {
            let line = usize::try_from(cell.point.line.0 + offset).unwrap_or(0);
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                lines[line].push(cell.c);
            }
        }
        lines
            .iter()
            .map(|line| line.trim_end().to_owned())
            .collect()
    }

    /// What to draw, leaving out cells that are default and empty.
    pub fn frame(&self, palette: &Palette, focused: bool) -> Frame {
        let screen = self.screen();
        let content = screen.term.renderable_content();
        let offset = content.display_offset as i32;
        let cursor = cursor_mark(content.cursor.shape, focused).and_then(|mark| {
            let line = usize::try_from(content.cursor.point.line.0 + offset).ok()?;
            (line < screen.term.screen_lines()).then_some(Cursor {
                line,
                column: content.cursor.point.column.0,
                mark,
            })
        });
        let (selection, colors) = (content.selection, content.colors);
        let mut frame = Frame {
            cursor,
            ..Frame::default()
        };

        for indexed in content.display_iter {
            let cell = indexed.cell;
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let Ok(line) = usize::try_from(indexed.point.line.0 + offset) else {
                continue;
            };
            let column = indexed.point.column.0;
            let cells = if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };

            let mut foreground = palette.resolve(cell.fg, colors);
            let mut background = palette.resolve(cell.bg, colors);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if cell.flags.contains(Flags::DIM) {
                foreground = foreground.opacity(DIM_OPACITY);
            }
            if selection.is_some_and(|range| range.contains(indexed.point)) {
                background = palette.selection;
            }
            let on_cursor =
                cursor.is_some_and(|cursor| (cursor.line, cursor.column) == (line, column));
            if on_cursor && cursor.is_some_and(|cursor| cursor.mark == CursorMark::Filled) {
                background = palette.cursor;
                foreground = palette.background;
            }

            if background != palette.background {
                frame.fill(line, column, cells, background);
            }
            let underline = cell.flags.intersects(Flags::ALL_UNDERLINES);
            let strikeout = cell.flags.contains(Flags::STRIKEOUT);
            let blank = cell.c == ' ' && cell.zerowidth().is_none();
            if cell.flags.contains(Flags::HIDDEN) || (blank && !underline && !strikeout) {
                continue;
            }
            frame.write(Span {
                line,
                column,
                cells,
                text: cell_text(cell),
                color: foreground,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline,
                strikeout,
            });
        }
        frame
    }
}

/// The xterm modifier parameter, which is 1 with no modifiers.
fn modifier_code(keystroke: &Keystroke) -> u8 {
    let modifiers = keystroke.modifiers;
    1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.control)
}

fn cursor_key(letter: char, code: u8, mode: TermMode) -> String {
    if code > 1 {
        format!("\x1b[1;{code}{letter}")
    } else if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{letter}")
    } else {
        format!("\x1b[{letter}")
    }
}

fn tilde_key(number: u8, code: u8) -> String {
    if code > 1 {
        format!("\x1b[{number};{code}~")
    } else {
        format!("\x1b[{number}~")
    }
}

fn function_key(number: u8, code: u8) -> Option<String> {
    let letter_key = |letter: char| {
        Some(if code > 1 {
            format!("\x1b[1;{code}{letter}")
        } else {
            format!("\x1bO{letter}")
        })
    };
    match number {
        1 => letter_key('P'),
        2 => letter_key('Q'),
        3 => letter_key('R'),
        4 => letter_key('S'),
        5 => Some(tilde_key(15, code)),
        6..=10 => Some(tilde_key(number + 11, code)),
        11 | 12 => Some(tilde_key(number + 12, code)),
        _ => None,
    }
}

fn control_byte(key: &str) -> Option<u8> {
    let [byte] = key.as_bytes() else {
        return None;
    };
    match byte.to_ascii_uppercase() {
        byte @ b'@'..=b'_' => Some(byte - b'@'),
        b'?' => Some(0x7f),
        _ => None,
    }
}

/// What a key sends to the program, or `None` if the key is not for the program.
pub fn key_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let code = modifier_code(keystroke);
    let alt_prefixed = |bytes: &[u8]| {
        let mut sent = if modifiers.alt {
            vec![0x1b]
        } else {
            Vec::new()
        };
        sent.extend_from_slice(bytes);
        sent
    };
    let key = keystroke.key.as_str();
    let sequence = match key {
        "up" => cursor_key('A', code, mode),
        "down" => cursor_key('B', code, mode),
        "right" => cursor_key('C', code, mode),
        "left" => cursor_key('D', code, mode),
        "home" => cursor_key('H', code, mode),
        "end" => cursor_key('F', code, mode),
        "insert" => tilde_key(2, code),
        "delete" => tilde_key(3, code),
        "pageup" => tilde_key(5, code),
        "pagedown" => tilde_key(6, code),
        "enter" => return Some(alt_prefixed(b"\r")),
        "escape" => return Some(alt_prefixed(b"\x1b")),
        "backspace" if modifiers.control => return Some(alt_prefixed(b"\x08")),
        "backspace" => return Some(alt_prefixed(b"\x7f")),
        "tab" if modifiers.shift => return Some(b"\x1b[Z".to_vec()),
        "tab" => return Some(alt_prefixed(b"\t")),
        "space" if modifiers.control => return Some(alt_prefixed(b"\0")),
        _ => match key.strip_prefix('f').and_then(|number| number.parse().ok()) {
            Some(number) => function_key(number, code)?,
            None => return typed_bytes(keystroke),
        },
    };
    Some(sequence.into_bytes())
}

fn typed_bytes(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    // Some layouts type characters with control and alt together.
    let composed = modifiers.control && modifiers.alt;
    if modifiers.control && !composed {
        return control_byte(&keystroke.key).map(|byte| vec![byte]);
    }
    let text = keystroke.key_char.as_ref()?;
    // On macOS, option types a different character instead of being a modifier.
    if modifiers.alt && !composed && !cfg!(target_os = "macos") {
        let mut sent = vec![0x1b];
        sent.extend_from_slice(text.as_bytes());
        return Some(sent);
    }
    Some(text.clone().into_bytes())
}

/// What pasted text sends. Within brackets, escape characters are removed so the
/// text cannot end the paste early and run as commands.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let text = text.replace('\x1b', "");
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press(u8),
    Release(u8),
    Motion(u8),
    WheelUp,
    WheelDown,
}

/// Reports a mouse event to a program that asked for them, or returns `None` if
/// it did not ask for this one.
pub fn mouse_report(
    action: MouseAction,
    line: usize,
    column: usize,
    mode: TermMode,
) -> Option<Vec<u8>> {
    const MOTION: u8 = 32;
    const WHEEL_UP: u8 = 64;
    const WHEEL_DOWN: u8 = 65;
    let (button, release) = match action {
        MouseAction::Press(button) => (button, false),
        MouseAction::Release(button) => (button, true),
        MouseAction::Motion(button) => (button + MOTION, false),
        MouseAction::WheelUp => (WHEEL_UP, false),
        MouseAction::WheelDown => (WHEEL_DOWN, false),
    };
    let reported = match action {
        MouseAction::Motion(_) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        _ => mode.intersects(TermMode::MOUSE_MODE),
    };
    if !reported {
        return None;
    }
    let (column, line) = (column + 1, line + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{button};{column};{line}{end}").into_bytes());
    }
    const LIMIT: usize = 223;
    if column > LIMIT || line > LIMIT {
        return None;
    }
    let button = if release { 3 } else { button };
    let encode = |value: usize| 32 + u8::try_from(value).unwrap_or(u8::MAX);
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + button,
        encode(column),
        encode(line),
    ])
}

/// What the wheel sends to a full-screen program that does not take the mouse.
pub fn wheel_keys(up: bool, mode: TermMode) -> Vec<u8> {
    let letter = if up { 'A' } else { 'B' };
    cursor_key(letter, 1, mode).into_bytes()
}

#[cfg(test)]
mod tests {
    use gpui_kit::Modifiers;

    use super::*;

    fn palette() -> Palette {
        let color = |h: f32| Hsla {
            h,
            s: 0.5,
            l: 0.5,
            a: 1.,
        };
        let mut ansi = [color(0.); 16];
        for (index, entry) in ansi.iter_mut().enumerate() {
            *entry = color(index as f32 / 16.);
        }
        Palette {
            ansi,
            foreground: color(0.91),
            background: color(0.92),
            cursor: color(0.93),
            selection: color(0.94),
        }
    }

    fn emulator() -> Emulator {
        Emulator::new(20, 5, palette())
    }

    fn key(key: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: key.into(),
            key_char: Some(key.into()),
        }
    }

    fn with(modifiers: Modifiers, name: &str) -> Keystroke {
        Keystroke {
            modifiers,
            ..key(name)
        }
    }

    #[test]
    fn text_appears_on_the_grid() {
        let terminal = emulator();
        terminal.advance(b"hello\r\nworld");
        assert_eq!(terminal.visible_lines()[..2], ["hello", "world"]);
    }

    #[test]
    fn lines_that_scroll_off_stay_in_the_scrollback() {
        let terminal = emulator();
        for number in 0..8 {
            terminal.advance(format!("line {number}\r\n").as_bytes());
        }
        assert_eq!(terminal.scrollback_lines(), 4);

        terminal.scroll(4);

        assert_eq!(terminal.visible_lines()[0], "line 0");
    }

    #[test]
    fn scrollback_is_kept_when_modes_are_restored() {
        let terminal = emulator();
        for number in 0..8 {
            terminal.advance(format!("line {number}\r\n").as_bytes());
        }
        terminal.advance(b"\x1b[?1049h\x1b[2Jfull screen");

        terminal.restore_modes();

        assert!(!terminal.mode().contains(TermMode::ALT_SCREEN));
        assert!(terminal.scrollback_lines() >= 4);
        terminal.scroll(terminal.scrollback_lines() as i32);
        assert_eq!(terminal.visible_lines()[0], "line 0");
    }

    #[test]
    fn a_full_screen_program_gets_its_own_screen_and_gives_it_back() {
        let terminal = emulator();
        terminal.advance(b"prompt$ ");

        terminal.advance(b"\x1b[?1049h\x1b[2J\x1b[3;5Heditor");
        assert!(terminal.mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(terminal.visible_lines()[2], "    editor");

        terminal.advance(b"\x1b[?1049l");
        assert_eq!(terminal.visible_lines()[0], "prompt$");
    }

    #[test]
    fn named_colors_come_from_the_palette() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"\x1b[31mred\x1b[92mgreen");

        let frame = terminal.frame(&palette, false);

        assert_eq!(frame.spans[0].text, "red");
        assert_eq!(frame.spans[0].color, palette.ansi[1]);
        assert_eq!(frame.spans[1].text, "green");
        assert_eq!(frame.spans[1].color, palette.ansi[10]);
    }

    #[test]
    fn indexed_low_colors_are_the_named_ones() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"\x1b[38;5;4mblue");
        assert_eq!(
            terminal.frame(&palette, false).spans[0].color,
            palette.ansi[4]
        );
    }

    #[test]
    fn the_extended_colors_follow_the_standard_formula() {
        assert_eq!(extended_color(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(extended_color(196), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(
            extended_color(231),
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(extended_color(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            extended_color(255),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn a_program_can_use_exact_colors() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"\x1b[38;2;10;20;30mexact");
        let expected = hsla_of(Rgb {
            r: 10,
            g: 20,
            b: 30,
        });
        assert_eq!(terminal.frame(&palette, false).spans[0].color, expected);
    }

    #[test]
    fn inverse_text_swaps_its_colors() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"\x1b[7mx");

        let frame = terminal.frame(&palette, false);

        assert_eq!(frame.spans[0].color, palette.background);
        assert_eq!(frame.blocks[0].color, palette.foreground);
    }

    #[test]
    fn a_wide_character_takes_two_cells_and_stands_alone() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance("a日b".as_bytes());

        let frame = terminal.frame(&palette, false);

        let placed: Vec<_> = frame
            .spans
            .iter()
            .map(|span| (span.text.as_str(), span.column, span.cells))
            .collect();
        assert_eq!(placed, [("a", 0, 1), ("日", 1, 2), ("b", 3, 1)]);
    }

    #[test]
    fn the_cursor_is_filled_only_when_focused() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"ab");

        let focused = terminal.frame(&palette, true).cursor.unwrap();
        assert_eq!((focused.column, focused.mark), (2, CursorMark::Filled));

        let unfocused = terminal.frame(&palette, false).cursor.unwrap();
        assert_eq!(unfocused.mark, CursorMark::Outline);
    }

    #[test]
    fn a_program_can_hide_the_cursor() {
        let (terminal, palette) = (emulator(), palette());
        terminal.advance(b"\x1b[?25l");
        assert_eq!(terminal.frame(&palette, true).cursor, None);
    }

    #[test]
    fn a_cursor_position_query_is_answered() {
        let terminal = emulator();
        assert_eq!(terminal.advance(b"abc\x1b[6n"), b"\x1b[1;4R");
    }

    #[test]
    fn a_background_color_query_is_answered_from_the_palette() {
        let reply = emulator().advance(b"\x1b]11;?\x1b\\");
        assert!(
            String::from_utf8(reply)
                .unwrap()
                .starts_with("\x1b]11;rgb:")
        );
    }

    #[test]
    fn a_container_cannot_read_the_clipboard() {
        assert!(emulator().advance(b"\x1b]52;c;?\x1b\\").is_empty());
    }

    #[test]
    fn resizing_changes_the_grid() {
        let terminal = emulator();
        terminal.resize(10, 2);
        assert_eq!(terminal.visible_lines().len(), 2);
    }

    #[test]
    fn a_selection_copies_its_text() {
        let terminal = emulator();
        terminal.advance(b"hello world");
        terminal.start_selection(SelectionType::Simple, 0, 6, Side::Left);
        terminal.extend_selection(0, 10, Side::Right);
        assert_eq!(terminal.selected_text().as_deref(), Some("world"));

        terminal.clear_selection();
        assert_eq!(terminal.selected_text(), None);
    }

    #[test]
    fn a_selection_in_the_scrollback_copies_what_is_shown_there() {
        let terminal = emulator();
        for number in 0..8 {
            terminal.advance(format!("line {number}\r\n").as_bytes());
        }
        terminal.scroll(4);
        terminal.start_selection(SelectionType::Simple, 0, 0, Side::Left);
        terminal.extend_selection(0, 5, Side::Right);
        assert_eq!(terminal.selected_text().as_deref(), Some("line 0"));
    }

    #[test]
    fn plain_keys_send_what_they_type() {
        assert_eq!(key_bytes(&key("a"), TermMode::NONE), Some(b"a".to_vec()));
        assert_eq!(
            key_bytes(&key("enter"), TermMode::NONE),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_bytes(&key("backspace"), TermMode::NONE),
            Some(b"\x7f".to_vec())
        );
    }

    #[test]
    fn control_keys_send_control_bytes() {
        let control = Modifiers::control();
        assert_eq!(
            key_bytes(&with(control, "c"), TermMode::NONE),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(&with(control, "["), TermMode::NONE),
            Some(vec![0x1b])
        );
        assert_eq!(
            key_bytes(&with(control, "space"), TermMode::NONE),
            Some(vec![0])
        );
    }

    #[test]
    fn arrow_keys_follow_the_programs_mode() {
        assert_eq!(
            key_bytes(&key("up"), TermMode::NONE),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(&key("up"), TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn modified_special_keys_carry_the_modifier() {
        let control = Modifiers::control();
        assert_eq!(
            key_bytes(&with(control, "right"), TermMode::NONE),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            key_bytes(&with(control, "delete"), TermMode::NONE),
            Some(b"\x1b[3;5~".to_vec())
        );
    }

    #[test]
    fn function_keys_send_their_standard_sequences() {
        assert_eq!(
            key_bytes(&key("f1"), TermMode::NONE),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_bytes(&key("f5"), TermMode::NONE),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            key_bytes(&key("f6"), TermMode::NONE),
            Some(b"\x1b[17~".to_vec())
        );
        assert_eq!(
            key_bytes(&key("f10"), TermMode::NONE),
            Some(b"\x1b[21~".to_vec())
        );
        assert_eq!(
            key_bytes(&key("f11"), TermMode::NONE),
            Some(b"\x1b[23~".to_vec())
        );
        assert_eq!(
            key_bytes(&key("f12"), TermMode::NONE),
            Some(b"\x1b[24~".to_vec())
        );
    }

    #[test]
    fn a_word_starting_with_f_is_not_a_function_key() {
        assert_eq!(key_bytes(&key("f"), TermMode::NONE), Some(b"f".to_vec()));
    }

    #[test]
    fn application_shortcuts_are_left_alone() {
        assert_eq!(
            key_bytes(&with(Modifiers::command(), "c"), TermMode::NONE),
            None
        );
    }

    #[test]
    fn pasted_lines_end_with_a_carriage_return() {
        assert_eq!(paste_bytes("a\nb\r\nc", TermMode::NONE), b"a\rb\rc");
    }

    #[test]
    fn a_bracketed_paste_cannot_end_itself_early() {
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf /\n", TermMode::BRACKETED_PASTE),
            b"\x1b[200~x[201~rm -rf /\r\x1b[201~"
        );
    }

    #[test]
    fn mouse_events_stay_local_until_a_program_asks() {
        assert_eq!(
            mouse_report(MouseAction::Press(0), 0, 0, TermMode::NONE),
            None
        );
    }

    #[test]
    fn mouse_events_use_the_format_the_program_chose() {
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(MouseAction::Press(0), 1, 2, sgr),
            Some(b"\x1b[<0;3;2M".to_vec())
        );
        assert_eq!(
            mouse_report(MouseAction::Release(0), 1, 2, sgr),
            Some(b"\x1b[<0;3;2m".to_vec())
        );
        assert_eq!(
            mouse_report(MouseAction::Press(0), 1, 2, TermMode::MOUSE_REPORT_CLICK),
            Some(vec![0x1b, b'[', b'M', 32, 35, 34])
        );
    }

    #[test]
    fn motion_is_reported_only_when_asked_for() {
        let clicks = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(mouse_report(MouseAction::Motion(0), 0, 0, clicks), None);
        let drags = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(MouseAction::Motion(0), 0, 0, drags),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
    }

    #[test]
    fn the_wheel_scrolls_a_full_screen_program_with_arrow_keys() {
        assert_eq!(wheel_keys(true, TermMode::NONE), b"\x1b[A");
        assert_eq!(wheel_keys(false, TermMode::APP_CURSOR), b"\x1bOB");
    }
}
