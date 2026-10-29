//! Speech text normalization ported from the retained Desktop speech-text.ts.
//! Machine identifiers, code and table bodies are silence, not English labels.
use regex::{Captures, Regex};
use std::sync::LazyLock;
macro_rules! replace {
    ($text:expr, $expression:literal, $value:expr $(,)?) => {{
        static EXPRESSION: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($expression).expect("static speech expression"));
        EXPRESSION.replace_all($text, $value).into_owned()
    }};
}
fn row(line: &str) -> Option<(usize, Vec<String>)> {
    let mut line = line;
    let mut depth = 0;
    loop {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if indent > 3 || line[..indent].contains('\t') {
            return None;
        }
        line = &line[indent..];
        if let Some(rest) = line.strip_prefix('>') {
            depth += 1;
            line = rest.strip_prefix(' ').unwrap_or(rest);
        } else {
            break;
        }
    }
    let mut cells = vec![];
    let mut start = 0;
    let mut slash = 0;
    let mut pipes = 0;
    for (offset, character) in line.char_indices() {
        if character == '|' && slash % 2 == 0 {
            cells.push(line[start..offset].trim().to_owned());
            start = offset + 1;
            pipes += 1;
        }
        slash = if character == '\\' { slash + 1 } else { 0 };
    }
    if pipes == 0 {
        return None;
    }
    cells.push(line[start..].trim().to_owned());
    let leading = line.trim_start().starts_with('|');
    let trailing = line.trim_end().ends_with('|');
    if leading {
        cells.remove(0);
    }
    if trailing {
        cells.pop();
    }
    if cells.len() < 2 && !(leading && trailing && cells.len() == 1) {
        return None;
    }
    Some((depth, cells))
}
fn tables(text: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    let mut output = vec![];
    let mut index = 0;
    while index < lines.len() {
        let header = row(lines[index]);
        let delimiter = lines.get(index + 1).and_then(|line| row(line));
        let table = header.as_ref().zip(delimiter.as_ref()).is_some_and(
            |((depth, cells), (d, markers))| {
                depth == d
                    && cells.len() == markers.len()
                    && markers.iter().all(|cell| {
                        let marker = cell.trim_matches(':');
                        marker.len() >= 3 && marker.chars().all(|c| c == '-')
                    })
            },
        );
        if table {
            let (depth, cells) = header.unwrap();
            let mut header = cells
                .iter()
                .map(|c| c.replace("\\|", " "))
                .filter(|c| !c.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            if !header.is_empty() {
                if !header.ends_with(['.', '!', '?', ':']) {
                    header.push('.');
                }
                output.push(header);
            }
            index += 2;
            while index < lines.len() && row(lines[index]).is_some_and(|(d, _)| d == depth) {
                index += 1;
            }
        } else {
            output.push(lines[index].to_owned());
            index += 1;
        }
    }
    output.join("\n")
}
fn dense(token: &str) -> bool {
    static EXT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[\w.-]{0,60}\.(?:wav|ogg|mp3|flac|m4a|aac|py|pyc|ts|tsx|js|jsx|mjs|cjs|json|yaml|yml|toml|md|mdx|txt|csv|xlsx|xls|pdf|png|jpg|jpeg|gif|webp|svg|log|sql|sh|bash|zsh|rs|go|java|rb|php|html|css|lock|tar|gz|zip|db|sqlite|sqlite3|onnx|pt|bin|env|ini|conf|cfg|xml)\b").unwrap()
    });
    static DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\d{4}[-/]\d{1,2}(?:[-/]\d{1,2})?$").unwrap());
    static UUID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{12}")
            .unwrap()
    });
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-fA-F0-9]{7,64}").unwrap());
    if token.contains('@') || DATE.is_match(token) {
        return false;
    }
    if token.starts_with(['/', '~']) || token.starts_with("./") || token.starts_with("../") {
        return true;
    }
    let digits = token.chars().any(|c| c.is_ascii_digit());
    if token.contains('/') && (EXT.is_match(token) || digits) {
        return true;
    }
    if EXT.is_match(token) || UUID.is_match(token) || (digits && HEX.is_match(token)) {
        return true;
    }
    if !digits {
        return false;
    }
    let separators = ['_', '.', '/']
        .iter()
        .filter(|c| token.contains(**c))
        .count();
    let hyphens = token.chars().filter(|c| *c == '-').count();
    separators >= 2 || hyphens >= 2 || token.contains('/') || (hyphens >= 1 && separators >= 1)
}
pub(super) fn sanitize(text: &str) -> String {
    static TOKENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_./~@-]+").unwrap());
    let text = tables(&text.replace("\r\n", "\n").replace('\r', "\n"));
    let text = replace!(&text, r"(?m):\s*$", ".");
    let text = replace!(&text, r"(\p{L})-\n(\p{L})", "$1$2");
    let text = replace!(
        &text,
        r#"([.!?])([*_~`>\"'’”)}\]]*)[ \t]*\n{2,}[ \t]*"#,
        "$1$2 ",
    );
    let text = replace!(&text, r"[ \t]*\n{2,}[ \t]*", ". ");
    let text = replace!(&text, r"[ \t]*\n[ \t]*", " ");
    let text = replace!(&text, r"(?s)```.*?(?:```|$)", "");
    let text = replace!(
        &text,
        r"(?i)^\s*(?:\([^\)\n]{1,48}\)\s*)?(?:processing|thinking|reasoning|analyzing|pondering|contemplating|musing|cogitating|ruminating|deliberating|mulling|reflecting|computing|synthesizing|formulating|brainstorming)\.\.\.\s*",
        " ",
    );
    let text = replace!(&text, r"\[([^\]]+)\]\(([^)]+)\)", "$1");
    let text = replace!(&text, r"`([^`]+)`", "$1");
    let text = replace!(&text, r"(?i)\bhttps?://\S+", "");
    static MEDIA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]*MEDIA:\S+").unwrap());
    let text = MEDIA
        .replace_all(&text, |capture: &Captures| {
            capture[0]
                .chars()
                .rev()
                .take_while(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']'))
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        })
        .into_owned();
    let text = replace!(
        &text,
        r"[\x{1F000}-\x{1FAFF}\x{2600}-\x{27BF}\x{FE0F}\x{200D}\x{E0020}-\x{E007F}]+",
        " ",
    );
    let text = replace!(
        &text,
        r"(?i)\b(?:sha(?:-?256|-?512|-?1|3)?|blake2[ab]?|md5|crc32?)[:\s]+[0-9a-f]{7,64}",
        " ",
    );
    let text = TOKENS
        .replace_all(&text, |capture: &Captures| {
            if dense(&capture[0]) {
                " ".to_owned()
            } else {
                capture[0].to_owned()
            }
        })
        .into_owned();
    let text = replace!(&text, r"(?m)^#{1,6}\s+", "");
    let text = replace!(&text, r"[*_~>#]", "");
    let text = replace!(&text, r"(?m)^\s*[-+*]\s+", "");
    let text = replace!(&text, r":\s*$", ".");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
#[cfg(test)]
mod tests;
