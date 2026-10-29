use std::collections::VecDeque;
#[derive(Default)]
pub struct Callbacks {
    pub replies: VecDeque<Vec<u8>>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub bell: bool,
    pub hyperlinks: Vec<(String, String)>,
    link_start: Option<((u16, u16), String)>,
}
impl vt100::Callbacks for Callbacks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).chars().take(120).collect());
    }
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        last: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let response = match (last, first, i1) {
            ('n', 6, None) => {
                let (r, c) = screen.cursor_position();
                Some(format!("\x1b[{};{}R", r + 1, c + 1))
            }
            ('n', 5, None) => Some("\x1b[0n".into()),
            ('c', _, Some(b'>')) => Some("\x1b[>0;136;0c".into()),
            ('c', _, None) => Some("\x1b[?1;2c".into()),
            ('t', 18, None) => {
                let (r, c) = screen.size();
                Some(format!("\x1b[8;{r};{c}t"))
            }
            _ => None,
        };
        if let Some(response) = response {
            self.replies.push_back(response.into_bytes());
        }
    }
    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        if params.first() == Some(&b"7".as_slice()) {
            if let Some(uri) = params
                .get(1)
                .and_then(|v| std::str::from_utf8(v).ok())
                .and_then(|v| url::Url::parse(v).ok())
            {
                if uri.scheme() == "file" {
                    self.cwd = uri
                        .to_file_path()
                        .ok()
                        .map(|p| p.to_string_lossy().into_owned());
                }
            }
        }
        if params.first() == Some(&b"8".as_slice()) {
            if let Some(raw) = params
                .get(2)
                .and_then(|value| std::str::from_utf8(value).ok())
            {
                if raw.is_empty() {
                    if let Some((start, url)) = self.link_start.take() {
                        let end = screen.cursor_position();
                        let label = screen.contents_between(start.0, start.1, end.0, end.1);
                        if !label.is_empty() {
                            self.hyperlinks.push((label, url));
                            if self.hyperlinks.len() > 64 {
                                self.hyperlinks.remove(0);
                            }
                        }
                    }
                } else if url::Url::parse(raw)
                    .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
                {
                    self.link_start = Some((screen.cursor_position(), raw.into()));
                }
            }
        }
        // OSC52 does not read/write the user's clipboard without an explicit
        // user gesture. Native copy/paste is routed by the focused pane.
    }
}
pub struct Grid {
    pub parser: vt100::Parser<Callbacks>,
    pub selection: Option<((u16, u16), (u16, u16))>,
    pub selection_all: bool,
    search: Option<(String, usize, u16)>,
}
impl Grid {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(rows, cols, 1000, Callbacks::default()),
            selection: None,
            selection_all: false,
            search: None,
        }
    }
    pub fn process(&mut self, bytes: &[u8], replay: bool) -> Vec<Vec<u8>> {
        let mut converted = Vec::with_capacity(bytes.len());
        let mut previous = 0;
        for byte in bytes {
            if *byte == b'\n' && previous != b'\r' {
                converted.push(b'\r');
            }
            converted.push(*byte);
            previous = *byte;
        }
        self.parser.process(&converted);
        let replies = self.parser.callbacks_mut().replies.drain(..).collect();
        if replay { vec![] } else { replies }
    }
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.selection = None;
        self.selection_all = false;
    }
    pub fn scroll(&mut self, lines: i32) {
        let previous = self.screen().scrollback() as i64;
        self.parser
            .screen_mut()
            .set_scrollback((previous + lines as i64).max(0) as usize);
        self.selection = None;
        self.selection_all = false;
    }
    pub fn selected(&self) -> String {
        if self.selection_all {
            return self.all_text();
        }
        self.selection
            .map(|(a, b)| {
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                self.screen()
                    .contents_between(a.0, a.1, b.0, b.1.saturating_add(1))
            })
            .unwrap_or_default()
    }
    pub fn select_all(&mut self) {
        self.selection_all = true;
        self.selection = None;
    }
    pub fn all_text(&self) -> String {
        self.history()
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn history(&self) -> Vec<(usize, String)> {
        let mut screen = self.screen().clone();
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        let (rows, cols) = screen.size();
        (0..history + rows as usize)
            .map(|index| {
                screen.set_scrollback(history.saturating_sub(index));
                let row = index.saturating_sub(history) as u16;
                let line = screen.rows(0, cols).nth(row as usize).unwrap_or_default();
                (index, line)
            })
            .collect()
    }
    pub fn snapshot(&self) -> Vec<u8> {
        if self.screen().alternate_screen() {
            return self.screen().state_formatted();
        }
        let mut screen = self.screen().clone();
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        let (rows, cols) = screen.size();
        let mut retained = std::collections::VecDeque::new();
        let mut size = 0;
        for index in 0..history + rows as usize {
            screen.set_scrollback(history.saturating_sub(index));
            let row = index.saturating_sub(history) as u16;
            let end = (0..cols)
                .rev()
                .find(|col| {
                    screen.cell(row, *col).is_some_and(|cell| {
                        cell.has_contents() || cell.bgcolor() != vt100::Color::Default
                    })
                })
                .map(|col| col + 1)
                .unwrap_or(0);
            let mut line = vec![];
            let mut previous = None;
            for col in 0..end {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let attrs = (
                        cell.fgcolor(),
                        cell.bgcolor(),
                        cell.bold(),
                        cell.dim(),
                        cell.italic(),
                        cell.underline(),
                        cell.inverse(),
                    );
                    if previous != Some(attrs) {
                        line.extend_from_slice(sgr(cell).as_bytes());
                        previous = Some(attrs);
                    }
                    line.extend_from_slice(if cell.contents().is_empty() {
                        b" "
                    } else {
                        cell.contents().as_bytes()
                    });
                }
            }
            line.extend_from_slice(b"\x1b[0m");
            size += line.len();
            retained.push_back(line);
            while size > 48000 && retained.len() > rows as usize {
                size -= retained.pop_front().unwrap().len();
            }
        }
        let mut bytes = b"\x1bc".to_vec();
        let count = retained.len();
        for (index, line) in retained.into_iter().enumerate() {
            bytes.extend(line);
            if index + 1 < count {
                bytes.extend_from_slice(b"\r\n");
            }
        }
        bytes.extend(self.screen().input_mode_formatted());
        let (r, c) = self.screen().cursor_position();
        bytes.extend_from_slice(format!("\x1b[{};{}H", r + 1, c + 1).as_bytes());
        bytes
    }
    pub fn find_next(&mut self, needle: &str) -> Option<(u16, u16)> {
        if needle.is_empty() {
            self.search = None;
            return None;
        }
        let mut matches = vec![];
        for (index, line) in self.history() {
            for (byte, _) in line.match_indices(needle) {
                matches.push((index, byte));
            }
        }
        let after = self
            .search
            .as_ref()
            .filter(|(query, _, _)| query == needle)
            .map(|(_, line, col)| (*line, *col as usize));
        let found = matches
            .iter()
            .find(|position| after.is_none_or(|previous| **position > previous))
            .or(matches.first())
            .copied()?;
        self.parser.screen_mut().set_scrollback(usize::MAX);
        let maximum = self.screen().scrollback();
        self.parser
            .screen_mut()
            .set_scrollback(maximum.saturating_sub(found.0));
        let row = found.0.saturating_sub(maximum) as u16;
        let (_, cols) = self.screen().size();
        let mut bytes = 0;
        let mut column = 0;
        let mut end_column = 0;
        for col in 0..cols {
            if let Some(cell) = self.screen().cell(row, col) {
                if !cell.is_wide_continuation() {
                    if bytes <= found.1 {
                        column = col;
                    }
                    if bytes < found.1 + needle.len() {
                        end_column = col + u16::from(cell.is_wide());
                    }
                    bytes += cell.contents().len().max(1);
                }
            }
        }
        self.search = Some((needle.into(), found.0, found.1 as u16));
        self.selection_all = false;
        self.selection = Some(((row, column), (row, end_column.min(cols - 1))));
        Some((row, column))
    }
    pub fn contains(&self, row: u16, col: u16) -> bool {
        if self.selection_all {
            return true;
        }
        self.selection.is_some_and(|(a, b)| {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            (row, col) >= a && (row, col) <= b
        })
    }
}
fn sgr(cell: &vt100::Cell) -> String {
    let mut codes = vec!["0".to_owned()];
    for (enabled, code) in [
        (cell.bold(), "1"),
        (cell.dim(), "2"),
        (cell.italic(), "3"),
        (cell.underline(), "4"),
        (cell.inverse(), "7"),
    ] {
        if enabled {
            codes.push(code.into());
        }
    }
    for (color, kind) in [(cell.fgcolor(), 38), (cell.bgcolor(), 48)] {
        match color {
            vt100::Color::Default => {}
            vt100::Color::Idx(index) => codes.push(format!("{kind};5;{index}")),
            vt100::Color::Rgb(r, g, b) => codes.push(format!("{kind};2;{r};{g};{b}")),
        }
    }
    format!("\x1b[{}m", codes.join(";"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ansi_wide_cells_scrollback_and_replay_queries_remain_terminal_semantics() {
        let mut grid = Grid::new(3, 12);
        grid.process(b"\x1b[31mred\x1b[0m\r\n", false);
        grid.process("界😀".as_bytes(), false);
        assert_eq!(
            grid.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        assert!(grid.screen().cell(1, 0).unwrap().is_wide());
        let replies = grid.process(b"\x1b[6n", false);
        assert_eq!(replies, vec![b"\x1b[2;5R".to_vec()]);
        assert!(grid.process(b"\x1b[6n", true).is_empty());
        grid.process(b"\r\none\r\ntwo\r\nthree\r\nfour", false);
        grid.scroll(2);
        assert!(grid.screen().scrollback() > 0);
        grid.selection = Some(((0, 0), (1, 2)));
        assert!(!grid.selected().is_empty());
        grid.process(b"\x1b[?1049h\x1b[Halternate", false);
        assert!(grid.screen().alternate_screen());
        grid.process(b"\x1b[?1049l", false);
        assert!(!grid.screen().alternate_screen());
    }
}
