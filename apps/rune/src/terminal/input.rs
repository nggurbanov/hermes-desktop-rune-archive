use rune::{Key, KeyEvent};
pub fn key(event: &KeyEvent, application: bool) -> Option<Vec<u8>> {
    let m = event.mods;
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    let sequence = match &event.key {
        Key::Text(text) => {
            if m.ctrl && !m.alt && text.len() == 1 {
                let c = text.as_bytes()[0];
                if c.is_ascii_alphabetic() {
                    return Some(vec![c.to_ascii_uppercase() - b'@']);
                }
            }
            format!(
                "{}{}",
                if m.alt && !(m.ctrl && !cfg!(target_os = "macos")) {
                    "\x1b"
                } else {
                    ""
                },
                text
            )
        }
        Key::Command(c) if m.ctrl || !cfg!(target_os = "macos") => {
            if c.is_ascii_alphabetic() {
                return Some(vec![c.to_ascii_uppercase() as u8 - b'@']);
            }
            match c {
                ' ' | '@' => "\0".into(),
                '[' => "\x1b".into(),
                '\\' => "\x1c".into(),
                ']' => "\x1d".into(),
                '^' => "\x1e".into(),
                '_' | '/' => "\x1f".into(),
                _ => return None,
            }
        }
        Key::Enter => "\r".into(),
        Key::Backspace => if m.alt { "\x1b\x7f" } else { "\x7f" }.into(),
        Key::Escape => "\x1b".into(),
        Key::Tab => if m.shift { "\x1b[Z" } else { "\t" }.into(),
        Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End => {
            let final_ = match event.key {
                Key::Left => 'D',
                Key::Right => 'C',
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Home => 'H',
                _ => 'F',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{final_}")
            } else {
                format!("\x1b{}{final_}", if application { "O" } else { "[" })
            }
        }
        Key::Delete => format!(
            "\x1b[3{}~",
            if modifier > 1 {
                format!(";{modifier}")
            } else {
                String::new()
            }
        ),
        Key::PageUp => "\x1b[5~".into(),
        Key::PageDown => "\x1b[6~".into(),
        Key::Function(n) => match n {
            1..=4 => {
                if modifier > 1 {
                    format!("\x1b[1;{modifier}{}", (b'P' + n - 1) as char)
                } else {
                    format!("\x1bO{}", (b'P' + n - 1) as char)
                }
            }
            5..=24 => {
                let codes = [
                    15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34, 42, 43, 44, 45,
                ];
                if modifier > 1 {
                    format!("\x1b[{};{modifier}~", codes[(*n - 5) as usize])
                } else {
                    format!("\x1b[{}~", codes[(*n - 5) as usize])
                }
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(sequence.into_bytes())
}
pub fn keypad(key: rune::PhysicalKeyCode) -> Option<Vec<u8>> {
    use rune::PhysicalKeyCode as Code;
    let end = match key {
        Code::Numpad0 => 'p',
        Code::Numpad1 => 'q',
        Code::Numpad2 => 'r',
        Code::Numpad3 => 's',
        Code::Numpad4 => 't',
        Code::Numpad5 => 'u',
        Code::Numpad6 => 'v',
        Code::Numpad7 => 'w',
        Code::Numpad8 => 'x',
        Code::Numpad9 => 'y',
        Code::NumpadDecimal => 'n',
        Code::NumpadEnter => 'M',
        Code::NumpadAdd => 'k',
        Code::NumpadSubtract => 'm',
        Code::NumpadMultiply => 'j',
        Code::NumpadDivide => 'o',
        Code::NumpadEqual => 'X',
        _ => return None,
    };
    Some(format!("\x1bO{end}").into_bytes())
}
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace('\0', "").replace("\r\n", "\n");
    if bracketed {
        format!("\x1b[200~{}\x1b[201~", text.replace("\x1b[201~", "")).into_bytes()
    } else {
        text.replace('\n', "\r").into_bytes()
    }
}
pub fn quote(path: &str, powershell: bool) -> String {
    if powershell {
        format!("'{}'", path.replace('\'', "''"))
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_input_preserves_interrupt_modes_and_paste_boundaries() {
        let e = KeyEvent {
            key: Key::Command('c'),
            mods: rune::Mods {
                ctrl: true,
                ..Default::default()
            },
        };
        assert_eq!(key(&e, false).unwrap(), vec![3]);
        let e = KeyEvent {
            key: Key::Up,
            mods: Default::default(),
        };
        assert_eq!(key(&e, true).unwrap(), b"\x1bOA");
        assert_eq!(paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(quote("/a'b", false), "'/a'\\''b'");
    }
}
