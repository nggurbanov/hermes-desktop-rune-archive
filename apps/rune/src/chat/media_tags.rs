//! Display-only port of the pinned Desktop MEDIA tag normalizer.
use regex::{Captures, Regex};
use std::sync::OnceLock;

const EXTS: &str = "geojson|jpeg|webm|tiff|docx|xlsx|pptx|html|flac|opus|webp|json|yaml|epub|png|jpg|gif|bmp|svg|mp4|mov|avi|mkv|3gp|mp3|m2a|wav|ogg|m4a|pdf|doc|odt|rtf|txt|xls|ods|csv|tsv|xml|yml|kmz|kml|gpx|ppt|odp|key|zip|tar|tgz|bz2|rar|apk|ipa|htm|md|gz|xz|7z";
fn line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"(^|\n)[\t ]*[`"']?MEDIA:[\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]*(?P<path>`[^`\n]+`|"[^"\n]+"|'[^'\n]+'|(?:~/|/|[A-Za-z]:[/\\])[^\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+?(?:[\x09\x0B-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+[^\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+?)*?\.(?:{EXTS})|[^\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}`"]+)[`"']?[\t ]*(\n|$)"#)).unwrap())
}
fn anchored_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"^(?:~/|/|[A-Za-z]:[/\\])[^\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+?(?:[\x09\x0B-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+[^\x09-\x0D\x20\x{{00A0}}\x{{1680}}\x{{2000}}-\x{{200A}}\x{{2028}}\x{{2029}}\x{{202F}}\x{{205F}}\x{{3000}}\x{{FEFF}}]+?)*?\.(?:{EXTS})$")).unwrap())
}
fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn plausible(s: &str) -> bool {
    s.contains(['/', '\\'])
        || s.as_bytes()
            .windows(2)
            .any(|w| w[0] == b'.' && w[1] != b'.')
}
fn link(value: &str) -> Option<String> {
    let raw = value.trim_matches(js_whitespace);
    let quoted = raw.len() >= 2
        && matches!(raw.as_bytes()[0], b'\'' | b'"' | b'`')
        && raw.as_bytes()[0] == *raw.as_bytes().last()?;
    let mut path = if quoted {
        &raw[1..raw.len() - 1]
    } else {
        raw.strip_suffix(['`', '"']).unwrap_or(raw)
    };
    let mut punctuation = "";
    if !quoted {
        let end = path.len();
        while path.ends_with(['.', ',', ';', ':', '!', '?']) && plausible(&path[..path.len() - 1]) {
            path = &path[..path.len() - 1];
        }
        punctuation = &raw[path.len()..end];
    }
    if !plausible(path) {
        return None;
    }
    let ext = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = match ext.as_str() {
        "flac" | "m4a" | "mp3" | "ogg" | "opus" | "wav" => "Audio",
        "bmp" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "webp" => "Image",
        "avi" | "mkv" | "mov" | "mp4" | "webm" => "Video",
        _ => "File",
    };
    let windows = path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && matches!(path.as_bytes().get(2), Some(b'/' | b'\\'));
    let name = if !windows {
        url::Url::parse(path).ok().map(|u| {
            u.path()
                .split('/')
                .filter(|s| !s.is_empty())
                .last()
                .unwrap_or(path)
                .to_owned()
        })
    } else {
        None
    }
    .unwrap_or_else(|| {
        path.split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or(path)
            .to_owned()
    });
    let mut escaped = String::new();
    for c in name.chars() {
        if matches!(c, '[' | ']' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let mut encoded = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    Some(format!(
        "[{kind}: {escaped}](#media:{encoded}){punctuation}"
    ))
}
fn boundary(s: &str) -> bool {
    s.is_empty()
        || s.starts_with("MEDIA:")
        || s.chars()
            .next()
            .is_some_and(|c| js_whitespace(c) || "`\"'*_,;:)]}".contains(c))
}
fn inline(input: &str) -> String {
    let mut output = String::new();
    let mut pos = 0;
    while let Some(offset) = input[pos..].find("MEDIA:") {
        let tag = pos + offset;
        let start = if tag > pos && matches!(input.as_bytes()[tag - 1], b'`' | b'"' | b'\'') {
            tag - 1
        } else {
            tag
        };
        output.push_str(&input[pos..start]);
        let after = tag + 6;
        let ws = input[after..].trim_start_matches(js_whitespace);
        let path_start = input.len() - ws.len();
        let mut end = None;
        if let Some(q @ ('`' | '"' | '\'')) = ws.chars().next() {
            if let Some(close) = ws[1..].find(q) {
                if close > 0 && !ws[1..1 + close].contains('\n') {
                    end = Some(path_start + close + 2);
                }
            }
        }
        if end.is_none() {
            for (i, c) in ws.char_indices().skip(1) {
                if anchored_re().is_match(&ws[..i]) && boundary(&ws[i..]) {
                    end = Some(path_start + i);
                    break;
                }
                if c == '\n' {
                    break;
                }
            }
            if end.is_none() && anchored_re().is_match(ws) {
                end = Some(input.len());
            }
        }
        if end.is_none() {
            let len = ws
                .find(|c: char| js_whitespace(c) || c == '`' || c == '"')
                .unwrap_or(ws.len());
            if len > 0 {
                end = Some(path_start + len);
            }
        }
        if let Some(path_end) = end {
            let mut match_end = path_end;
            if matches!(input.as_bytes().get(match_end), Some(b'`' | b'"' | b'\'')) {
                match_end += 1;
            }
            output.push_str(
                &link(&input[path_start..path_end])
                    .unwrap_or_else(|| input[start..match_end].to_owned()),
            );
            pos = match_end;
        } else {
            output.push_str(&input[start..after]);
            pos = after;
        }
    }
    output.push_str(&input[pos..]);
    output
}
/// Returns a display-only Markdown string. The caller retains raw source separately.
pub(crate) fn render_media_tags(raw: &str) -> String {
    let lines = line_re().replace_all(raw, |c: &Captures<'_>| {
        link(&c["path"])
            .map(|v| format!("{}{}{}", &c[1], v, &c[3]))
            .unwrap_or_else(|| c[0].to_owned())
    });
    inline(&lines)
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_goldens() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("media_tags_goldens.json")).unwrap();
        let mut failures = Vec::new();
        for (group, count) in [("normalization", 34), ("stream", 11)] {
            let cases = fixtures[group].as_array().unwrap();
            assert_eq!(cases.len(), count);
            for case in cases {
                let actual = super::render_media_tags(case["input"].as_str().unwrap());
                if actual != case["expected"].as_str().unwrap() {
                    failures.push(format!(
                        "{group} {}: {actual:?} != {:?}",
                        case["id"], case["expected"]
                    ));
                }
            }
        }
        let review: serde_json::Value =
            serde_json::from_str(include_str!("media_tags_review_goldens.json")).unwrap();
        assert_eq!(review.as_array().unwrap().len(), 7);
        for case in review.as_array().unwrap() {
            let actual = super::render_media_tags(case["input"].as_str().unwrap());
            if actual != case["output"].as_str().unwrap() {
                failures.push(format!(
                    "review {:?}: {actual:?} != {:?}",
                    case["input"], case["output"]
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// Match the role-selected Desktop presentation without altering history.
pub(crate) fn display_text(role: &str, raw: &str) -> String {
    if role == "assistant" {
        render_media_tags(raw)
    } else {
        raw.to_owned()
    }
}
