//! Rune draws the terminal's ANSI cell grid directly; no browser terminal host.
use crate::{
    app::Viewer,
    terminal::{Action, Source, Status},
};
use rune::{Color, El, Key, KeyEvent, TextStyle, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};
fn button(label: impl Into<String>, key: impl Into<String>, action: Action, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .on_press_tx(move |s: &mut Viewer, tx| {
            s.terminal_action(action.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
pub fn panel(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body = col()
        .key("terminal-panel")
        .w_full()
        .h_full()
        .min_w(0.)
        .min_h(0.)
        .fill(k.c.bg_base)
        .gap(0.);
    let Some(workspace) = s.terminal.current() else {
        return body.child(
            text("Connect to an agent to open its terminal workspace")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap()
                .px(16.)
                .py(16.),
        );
    };
    let mut tabs = row().w_full().min_w(0.).gap(3.).center_y().px(6.).py(4.);
    for tab in &workspace.tabs {
        let selected = workspace.active.as_deref() == Some(&tab.id);
        tabs = tabs
            .child(
                Button::new(format!(
                    "{}{}",
                    if tab.source == Source::Agent {
                        "◈ "
                    } else {
                        ""
                    },
                    tab.title
                ))
                .variant(if selected {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::GhostMuted
                })
                .on_press({
                    let id = tab.id.clone();
                    move |s: &mut Viewer| s.terminal_action(Action::Select(id.clone()))
                })
                .build(k)
                .key(format!("terminal-tab-{}", tab.id))
                .max_w(180.),
            )
            .child(button(
                "×",
                format!("terminal-close-{}", tab.id),
                Action::Close(tab.id.clone()),
                k,
            ));
    }
    tabs =
        tabs.child(el().grow(1.))
            .child(button("+", "terminal-new", Action::Start, k).help(s.terminal.default_label()))
            .child(button("⋯", "terminal-menu", Action::Menu, k).on_layout(
                |s: &mut Viewer, rect| s.terminal.menu_anchor = [rect[0] - 280., rect[1] + rect[3]],
            ));
    body = body.child(tabs);
    if let Some(error) = &workspace.error {
        body = body.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .px(12.)
                .py(6.)
                .role("alert"),
        );
    }
    if s.terminal.menu {
        let mut menu = col()
            .key("terminal-actions")
            .w(300.)
            .gap(4.)
            .px(8.)
            .py(8.)
            .child(button(
                s.terminal.default_label(),
                "terminal-start-menu",
                Action::Start,
                k,
            ))
            .child(button(
                "New shell on this device",
                "terminal-start-local",
                Action::StartLocal,
                k,
            ))
            .child(button(
                "Select all retained output",
                "terminal-select-all",
                Action::SelectAll,
                k,
            ))
            .child(button(
                "Clear screen and scrollback",
                "terminal-clear",
                Action::Clear,
                k,
            ))
            .child(button(
                "Close all tabs",
                "terminal-close-all",
                Action::CloseAll,
                k,
            ));
        if s.terminal.ssh() {
            menu = menu.child(button(
                if s.terminal.ssh_windows() {
                    "SSH shell: PowerShell (switch to POSIX)"
                } else {
                    "SSH shell: POSIX (switch to PowerShell)"
                },
                "terminal-ssh-platform",
                Action::SshWindows(!s.terminal.ssh_windows()),
                k,
            ));
        }
        let selected = s.terminal.selection();
        if !selected.is_empty() {
            let copy = selected.clone();
            menu = menu.child(button(
                "Add selection to chat",
                "terminal-quote",
                Action::Quote,
                k,
            ));
            menu = menu.child(
                Button::new("Copy selection")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                    .build(k)
                    .key("terminal-copy"),
            );
            let linked = workspace
                .active()
                .and_then(|tab| {
                    tab.grid
                        .parser
                        .callbacks()
                        .hyperlinks
                        .iter()
                        .rev()
                        .find(|(label, _)| label.trim() == selected.trim())
                })
                .map(|(_, url)| url.as_str())
                .unwrap_or(selected.trim());
            if let Ok(url) = url::Url::parse(linked) {
                if matches!(url.scheme(), "http" | "https") {
                    let url = url.to_string();
                    menu = menu.child(
                        Button::new("Open selected link")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press_tx(move |_: &mut Viewer, tx| {
                                let _ = tx.open_url(&url);
                            })
                            .build(k)
                            .key("terminal-open-link"),
                    );
                }
            }
        }
        let search = TextInput::new(&s.terminal.find)
            .placeholder("Find in terminal output…")
            .width(None)
            .on_edit(|s: &mut Viewer| &mut s.terminal.find)
            .on_submit(|s: &mut Viewer, _| s.terminal_action(Action::FindNext))
            .build(k)
            .key("terminal-find");
        menu = menu
            .child(row().gap(4.).child(search.grow(1.)).child(button(
                "Next",
                "terminal-find-next",
                Action::FindNext,
                k,
            )))
            .child(button("Done", "terminal-menu-done", Action::HideMenu, k));
        if let Some(tab) = workspace.active() {
            menu = menu.child(button(
                "Close other tabs",
                "terminal-close-others",
                Action::CloseOthers(tab.id.clone()),
                k,
            ));
            if matches!(
                tab.status,
                Status::Detached | Status::Error | Status::Exited
            ) {
                menu = menu.child(button(
                    "Forget disconnected tab",
                    "terminal-forget",
                    Action::Forget(tab.id.clone()),
                    k,
                ));
            }
        }
        menu = menu.child(
            row()
                .gap(8.)
                .child(button(
                    "Smaller",
                    "terminal-font-smaller",
                    Action::FontSize(s.terminal.font_size - 1.),
                    k,
                ))
                .child(button(
                    "Larger",
                    "terminal-font-larger",
                    Action::FontSize(s.terminal.font_size + 1.),
                    k,
                )),
        );
        let bounds = s.terminal.bounds;
        let width = 300f32.min((bounds[2] - 16.).max(200.));
        let height = bounds[3].clamp(140., 480.);
        let x = s.terminal.menu_anchor[0].clamp(
            bounds[0] + 4.,
            (bounds[0] + bounds[2] - width - 4.).max(bounds[0] + 4.),
        );
        let y = s.terminal.menu_anchor[1].clamp(
            bounds[1].max(4.),
            (bounds[1] + bounds[3] - height).max(bounds[1].max(4.)),
        );
        layer.push(rune_kit::scrim("terminal-menu-scrim", |s: &mut Viewer| {
            s.terminal.menu = false
        }));
        layer.push(
            menu.absolute(x, y)
                .w(width)
                .max_h(height)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .fill(k.c.bg_deep)
                .border(1., k.c.border_muted)
                .radius(8.),
        );
    }
    let Some(tab) = workspace.active() else {
        return body.child(col().grow(1.).justify_center().items_center().gap(10.).child(text("A shell when you need one").font(k.ty.body_medium).color(k.c.text_base)).child(text("Shells start only when you ask. Agent background processes appear as read-only tabs.").font(k.ty.small).color(k.c.text_muted).wrap().max_w(360.)).child(button(s.terminal.default_label(),"terminal-start",Action::Start,k)));
    };
    let status = match tab.status {
        Status::Starting => "Starting…",
        Status::Open => {
            if tab.source == Source::Agent {
                "Agent process · read-only"
            } else {
                "Connected"
            }
        }
        Status::Reconnecting => "Reconnecting to the same shell…",
        Status::Detached => "Detached",
        Status::Exited => "Process exited",
        Status::Closing => "Closing…",
        Status::Error => "Connection failed",
    };
    let mut information = row()
        .w_full()
        .gap(8.)
        .px(10.)
        .py(4.)
        .center_y()
        .child(text(status).font(k.ty.small).color(k.c.text_muted))
        .child(
            text(&tab.cwd)
                .font(k.ty.small)
                .color(k.c.text_faint)
                .truncate()
                .grow(1.),
        );
    if matches!(tab.status, Status::Error | Status::Detached)
        && tab.source == Source::Remote
        && tab.durable.is_some()
    {
        information = information.child(button(
            "Reconnect",
            "terminal-reconnect",
            Action::Reconnect(tab.id.clone()),
            k,
        ));
    }
    body = body.child(information);
    if let Some(error) = &tab.error {
        body = body.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .px(10.)
                .py(6.)
                .role("alert"),
        );
    }
    let size = s.terminal.font_size;
    let cw = size * 0.6;
    let lh = size * 1.3;
    let screen = tab.grid.screen();
    let (rows, cols) = screen.size();
    let cursor = screen.cursor_position();
    let mut grid = el()
        .key("terminal-grid")
        .w_full()
        .grow(1.)
        .min_h(20.)
        .clip_soft(0.)
        .focusable(true)
        .role("textbox")
        .label(if tab.source == Source::Agent {
            "Read-only agent process output"
        } else {
            "Interactive terminal"
        })
        .on_key(on_key)
        .on_layout(|s: &mut Viewer, rect| s.terminal.layout(rect))
        .on_drop_files(|s: &mut Viewer, files, _| s.terminal_action(Action::Drop(files.to_vec())))
        .on_secondary_press(|s: &mut Viewer, point, tx| {
            s.terminal.menu_anchor = [point.x, point.y];
            s.terminal_action(Action::Menu);
            tx.instant();
        })
        .on_wheel(move |s: &mut Viewer, event, tx| {
            let amount = (event.dy / lh).round() as i32;
            let amount = if amount == 0 {
                if event.dy > 0. { 1 } else { -1 }
            } else {
                amount
            };
            if !mouse_wheel(s, amount, event.x, event.y, tx) {
                s.terminal_action(Action::Scroll(amount));
            }
            tx.instant();
            true
        })
        .on_drag(move |s: &mut Viewer, drag, tx| {
            let to_cell = |x: f32, y: f32| {
                (
                    ((y - 4.) / lh).floor().max(0.) as u16,
                    ((x - 6.) / cw).floor().max(0.) as u16,
                )
            };
            let first = to_cell(drag.start_x, drag.start_y);
            let last = to_cell(drag.x, drag.y);
            if mouse_drag(s, drag.phase, last, tx) {
                return;
            }
            if let Some(tab) = s
                .terminal
                .current_mut()
                .and_then(crate::terminal::Workspace::active_mut)
            {
                let (r, c) = tab.grid.screen().size();
                tab.grid.selection_all = false;
                tab.grid.selection = Some((
                    (first.0.min(r - 1), first.1.min(c - 1)),
                    (last.0.min(r - 1), last.1.min(c - 1)),
                ));
            }
            tx.instant();
        });
    for r in 0..rows {
        for c in 0..cols {
            let Some(cell) = screen.cell(r, c) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let is_cursor = (r, c) == cursor
                && !screen.hide_cursor()
                && screen.scrollback() == 0
                && tab.source != Source::Agent;
            let selected = tab.grid.contains(r, c);
            let mut foreground = ansi(cell.fgcolor(), k.c.text_base, k);
            let mut background = ansi(cell.bgcolor(), k.c.bg_base, k);
            if cell.inverse() {
                std::mem::swap(&mut foreground, &mut background);
            }
            if selected {
                background = k.c.hover;
            }
            if is_cursor {
                background = k.c.text_base;
                foreground = k.c.bg_base;
            }
            foreground = contrast(foreground, background);
            if !cell.has_contents()
                && !is_cursor
                && !selected
                && cell.bgcolor() == vt100::Color::Default
            {
                continue;
            }
            let mut style = TextStyle::new(size, if cell.bold() { 700. } else { 400. })
                .mono()
                .lh(lh);
            if cell.italic() {
                style = style.italic();
            }
            let mut node = el()
                .key(format!("terminal-cell-{r}-{c}"))
                .absolute(6. + c as f32 * cw, 4. + r as f32 * lh)
                .w(if cell.is_wide() { cw * 2. } else { cw })
                .h(lh)
                .fill(background)
                .child(
                    text(if cell.has_contents() {
                        cell.contents()
                    } else {
                        " "
                    })
                    .font(style)
                    .color(if cell.dim() {
                        foreground.alpha(0.55)
                    } else {
                        foreground
                    }),
                );
            if cell.underline() {
                node = node.child(
                    el().pin(Some(0.), None, Some(0.), Some(1.))
                        .h(1.)
                        .fill(foreground),
                );
            }
            if is_cursor {
                node = node.ime_cursor();
                if !tab.preedit.is_empty() {
                    node = node.child(
                        text(&tab.preedit)
                            .font(style)
                            .color(k.c.text_base)
                            .fill(k.c.hover),
                    );
                }
            }
            grid = grid.child(node);
        }
    }
    body.child(grid)
}
fn contrast(mut foreground: Color, background: Color) -> Color {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    let bg = luminance(background);
    let target = if bg > 0.18 {
        Color::BLACK
    } else {
        Color::WHITE
    };
    for _ in 0..20 {
        let fg = luminance(foreground);
        if (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05) >= 4.5 {
            break;
        }
        foreground = foreground.mix(target, 0.12);
    }
    foreground
}
fn ansi(color: vt100::Color, default: Color, _k: &Kit) -> Color {
    let rgb = match color {
        vt100::Color::Default => return default,
        vt100::Color::Rgb(r, g, b) => [r, g, b],
        vt100::Color::Idx(index) => {
            const COLORS: [[u8; 3]; 16] = [
                [44, 44, 44],
                [194, 70, 67],
                [76, 139, 85],
                [163, 123, 36],
                [78, 116, 183],
                [146, 90, 166],
                [54, 139, 151],
                [196, 196, 196],
                [117, 117, 117],
                [235, 106, 100],
                [111, 177, 119],
                [210, 172, 75],
                [118, 153, 218],
                [185, 129, 204],
                [94, 181, 192],
                [235, 235, 235],
            ];
            if index < 16 {
                COLORS[index as usize]
            } else if index < 232 {
                let value = index - 16;
                let level = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
                [level(value / 36), level((value % 36) / 6), level(value % 6)]
            } else {
                let value = 8 + 10 * (index - 232);
                [value; 3]
            }
        }
    };
    Color {
        r: rune::color::srgb_decode(rgb[0] as f32 / 255.),
        g: rune::color::srgb_decode(rgb[1] as f32 / 255.),
        b: rune::color::srgb_decode(rgb[2] as f32 / 255.),
        a: 1.,
    }
}
pub fn on_key(s: &mut Viewer, event: &KeyEvent, tx: &mut Tx) -> bool {
    if event.key == Key::Escape && s.terminal.menu {
        s.terminal.menu = false;
        tx.instant();
        return true;
    }
    if tx.focused() != Some("terminal-grid") {
        return false;
    }
    let selected = s.terminal.selection();
    if event.is_command('l') && !selected.is_empty() {
        s.terminal_action(Action::Quote);
        tx.instant();
        return true;
    }
    let command_copy = event.is_command('c') && !selected.is_empty();
    if command_copy {
        tx.copy(selected);
        return true;
    }
    if event.is_command('v') && (cfg!(target_os = "macos") || event.mods.shift) {
        tx.read_clipboard(|s: &mut Viewer, content, _| s.terminal.paste(content.text()));
        return true;
    }
    if event.is_command('a') && (cfg!(target_os = "macos") || event.mods.shift) {
        s.terminal_action(Action::SelectAll);
        return true;
    }
    if event.is_command('f') {
        s.terminal_action(Action::Menu);
        s.terminal.find.focused = true;
        return true;
    }
    if event.mods.shift && matches!(event.key, Key::PageUp | Key::PageDown) {
        s.terminal_action(Action::Scroll(if event.key == Key::PageUp {
            12
        } else {
            -12
        }));
        return true;
    }
    let Some(tab) = s
        .terminal
        .current()
        .and_then(crate::terminal::Workspace::active)
    else {
        return false;
    };
    if event.is_command('w') {
        if tab.source == Source::Agent || tab.status != Status::Open {
            let id = tab.id.clone();
            s.terminal_action(Action::Close(id));
        } else {
            s.terminal_action(Action::Input(vec![23]));
        }
        tx.instant();
        return true;
    }
    if event.is_command('=') || event.is_command('+') {
        s.terminal_action(Action::FontSize(s.terminal.font_size + 1.));
        return true;
    }
    if event.is_command('-') {
        s.terminal_action(Action::FontSize(s.terminal.font_size - 1.));
        return true;
    }
    if tab.source == Source::Agent {
        return true;
    }
    if tab.grid.screen().application_keypad() {
        if let Some(bytes) = tx.physical_key().and_then(crate::terminal::input::keypad) {
            s.terminal_action(Action::Input(bytes));
            return true;
        }
    }
    if let Some(bytes) = crate::terminal::input::key(event, tab.grid.screen().application_cursor())
    {
        s.terminal_action(Action::Input(bytes));
        tx.instant();
        true
    } else {
        false
    }
}
pub fn on_ime(s: &mut Viewer, event: &rune::Ime, tx: &mut Tx) -> bool {
    if tx.focused() != Some("terminal-grid") {
        return false;
    }
    match event {
        rune::Ime::Commit(text) => {
            s.terminal_action(Action::Input(text.as_bytes().to_vec()));
            if let Some(tab) = s
                .terminal
                .current_mut()
                .and_then(crate::terminal::Workspace::active_mut)
            {
                tab.preedit.clear();
            }
        }
        rune::Ime::Preedit(text, _) => {
            if let Some(tab) = s
                .terminal
                .current_mut()
                .and_then(crate::terminal::Workspace::active_mut)
            {
                tab.preedit = text.clone();
            }
        }
        _ => {}
    }
    tx.instant();
    true
}
fn mouse_drag(s: &mut Viewer, phase: rune::Phase, position: (u16, u16), tx: &mut Tx) -> bool {
    if tx.mods().shift || (cfg!(target_os = "macos") && tx.mods().alt) {
        return false;
    }
    let Some(tab) = s
        .terminal
        .current()
        .and_then(crate::terminal::Workspace::active)
    else {
        return false;
    };
    let mode = tab.grid.screen().mouse_protocol_mode();
    if mode == vt100::MouseProtocolMode::None || tab.source == Source::Agent {
        return false;
    }
    let release = phase == rune::Phase::Up;
    let motion = phase == rune::Phase::Move;
    if release && mode == vt100::MouseProtocolMode::Press {
        return true;
    }
    if motion
        && !matches!(
            mode,
            vt100::MouseProtocolMode::ButtonMotion | vt100::MouseProtocolMode::AnyMotion
        )
    {
        return true;
    }
    let code = if motion { 32 } else { 0 };
    let sgr = tab.grid.screen().mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr;
    let bytes = if sgr {
        format!(
            "\x1b[<{code};{};{}{}",
            position.1 + 1,
            position.0 + 1,
            if release { 'm' } else { 'M' }
        )
        .into_bytes()
    } else {
        vec![
            27,
            b'[',
            b'M',
            if release { 35 } else { 32 + code },
            (position.1.min(222) + 33) as u8,
            (position.0.min(222) + 33) as u8,
        ]
    };
    s.terminal_action(Action::Input(bytes));
    tx.instant();
    true
}
fn mouse_wheel(s: &mut Viewer, amount: i32, x: f32, y: f32, tx: &mut Tx) -> bool {
    if tx.mods().shift {
        return false;
    }
    let Some(tab) = s
        .terminal
        .current()
        .and_then(crate::terminal::Workspace::active)
    else {
        return false;
    };
    if tab.grid.screen().mouse_protocol_mode() == vt100::MouseProtocolMode::None
        || tab.source == Source::Agent
    {
        return false;
    }
    let position = (
        ((y - 4.) / (s.terminal.font_size * 1.3)).max(0.) as u16,
        ((x - 6.) / (s.terminal.font_size * 0.6)).max(0.) as u16,
    );
    let code = if amount > 0 { 64 } else { 65 };
    let bytes = if tab.grid.screen().mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr
    {
        format!("\x1b[<{code};{};{}M", position.1 + 1, position.0 + 1).into_bytes()
    } else {
        vec![
            27,
            b'[',
            b'M',
            32 + code,
            (position.1.min(222) + 33) as u8,
            (position.0.min(222) + 33) as u8,
        ]
    };
    s.terminal_action(Action::Input(bytes));
    true
}
