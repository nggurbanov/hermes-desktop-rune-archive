//! Search over the text actually rendered by one conversation surface.
use crate::composer::{DraftScope, EditorId};
use rune::TextEdit;
use std::{collections::HashMap, hash::Hash, ops::Range};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Owner {
    pub scope: DraftScope,
    pub editor: EditorId,
}

#[derive(Default)]
pub struct SearchView {
    pub open: bool,
    pub input: TextEdit,
    pub search: Search,
    pub pending: Option<PendingReveal>,
    pub revision: u64,
}

#[derive(Clone)]
pub struct PendingReveal {
    pub owner: Owner,
    pub query: String,
    pub current: Match,
    pub revision: u64,
    pub mounted_frame: bool,
}

/// The caller supplies its complete, typed surface identity.
pub struct Searches<Owner> {
    pub views: HashMap<Owner, SearchView>,
}
impl<Owner> Default for Searches<Owner> {
    fn default() -> Self {
        Self {
            views: HashMap::new(),
        }
    }
}

impl<Owner: Eq + Hash> Searches<Owner> {
    pub fn view(&mut self, owner: Owner) -> &mut SearchView {
        self.views.entry(owner).or_default()
    }
    pub fn retire_pending(&mut self) {
        for view in self.views.values_mut() {
            view.pending = None;
        }
    }
    pub fn remove(&mut self, owner: &Owner) {
        self.views.remove(owner);
    }
}

/// Stable message and fragment identities are provided by the conversation renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentKind {
    Markdown,
    MarkdownAudio,
    Literal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment<'a> {
    pub message: &'a str,
    pub part: &'a str,
    pub text: &'a str,
    pub kind: FragmentKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub message: String,
    pub part: String,
    pub bytes: Vec<Range<usize>>,
}

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub matches: Vec<Match>,
    active: Option<Match>,
}

impl Search {
    pub fn set_query(&mut self, query: String, fragments: &[Fragment<'_>]) {
        if self.query != query {
            self.query = query;
            self.active = None;
        }
        self.refresh(fragments);
    }

    /// Reconcile after history prepend or streaming without moving a retained hit.
    pub fn refresh(&mut self, fragments: &[Fragment<'_>]) {
        self.matches.clear();
        if !self.query.is_empty() {
            for fragment in fragments {
                let matches = match fragment.kind {
                    FragmentKind::Markdown | FragmentKind::MarkdownAudio => {
                        rune_kit::markdown::find(
                            fragment.text,
                            &self.query,
                            rune_kit::markdown::Case::Insensitive,
                        )
                    }
                    FragmentKind::Literal => find_literal(fragment.text, &self.query),
                };
                let replaced = if fragment.kind == FragmentKind::MarkdownAudio {
                    crate::view::transcript::markdown_audio_ranges(fragment.text)
                } else {
                    Vec::new()
                };
                for bytes in matches {
                    if bytes
                        .iter()
                        .any(|b| replaced.iter().any(|r| b.start < r.end && r.start < b.end))
                    {
                        continue;
                    }
                    self.matches.push(Match {
                        message: fragment.message.into(),
                        part: fragment.part.into(),
                        bytes,
                    });
                }
            }
        }
        self.active = self
            .active
            .take()
            .filter(|hit| self.matches.contains(hit))
            .or_else(|| self.matches.first().cloned());
    }

    pub fn rekey_current(
        &mut self,
        from_message: &str,
        from_part: &str,
        to_message: &str,
        to_part: &str,
    ) {
        if let Some(hit) = self
            .active
            .as_mut()
            .filter(|hit| hit.message == from_message && hit.part == from_part)
        {
            hit.message = to_message.into();
            hit.part = to_part.into();
        }
    }

    pub fn current(&self) -> Option<&Match> {
        self.active.as_ref()
    }
    pub fn ordinal(&self) -> usize {
        self.active
            .as_ref()
            .and_then(|hit| self.matches.iter().position(|m| m == hit))
            .map_or(0, |i| i + 1)
    }
    pub fn step(&mut self, forward: bool) -> Option<&Match> {
        let count = self.matches.len();
        if count == 0 {
            self.active = None;
            return None;
        }
        let index = self.ordinal().saturating_sub(1);
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        self.active = Some(self.matches[next].clone());
        self.current()
    }
}

/// Unicode lowercase matching with original UTF-8 boundaries, matching Markdown's folding.
fn find_literal(text: &str, query: &str) -> Vec<Vec<Range<usize>>> {
    fn fold(c: char) -> String {
        if c == 'ς' {
            "σ".into()
        } else {
            c.to_lowercase().collect()
        }
    }
    let needle: String = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut folded = String::new();
    let mut boundaries = Vec::new();
    for (start, c) in text.char_indices() {
        let from = folded.len();
        folded.push_str(&fold(c));
        boundaries.push((from, folded.len(), start, start + c.len_utf8()));
    }
    let mut result = Vec::new();
    let mut at = 0;
    while let Some(offset) = folded[at..].find(&needle) {
        let start = at + offset;
        let end = start + needle.len();
        if let (Ok(a), Ok(b)) = (
            boundaries.binary_search_by_key(&start, |b| b.0),
            boundaries.binary_search_by_key(&end, |b| b.1),
        ) {
            result.push(vec![boundaries[a].2..boundaries[b].3]);
            at = end;
        } else {
            at = folded[start..]
                .chars()
                .next()
                .map_or(folded.len(), |c| start + c.len_utf8());
        }
        if at >= folded.len() {
            break;
        }
    }
    result
}

pub fn tool_caption(name: &str, completed: bool) -> String {
    format!("{} {name}", if completed { "Completed" } else { "Called" })
}
pub fn disclosure_label(title: &str, open: bool) -> String {
    format!("{} {title}", if open { "⌄" } else { "›" })
}
pub fn image_label(label: &str, loaded: bool) -> String {
    if loaded {
        label.into()
    } else {
        format!("Load {label}")
    }
}
pub fn file_label(label: &str) -> String {
    format!("↗ {label}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f<'a>(message: &'a str, part: &'a str, text: &'a str) -> Fragment<'a> {
        Fragment {
            message,
            part,
            text,
            kind: FragmentKind::Markdown,
        }
    }
    #[test]
    fn literal_punctuation_and_unicode_ranges_are_not_markdown() {
        let value = "_A*<İ>_ and _a*<i\u{307}>_";
        let fragment = Fragment {
            message: "row",
            part: "args",
            text: value,
            kind: FragmentKind::Literal,
        };
        let mut search = Search::default();
        search.set_query("_a*<i\u{307}>_".into(), &[fragment]);
        assert_eq!(search.matches.len(), 2);
        assert_eq!(&value[search.matches[0].bytes[0].clone()], "_A*<İ>_");
        assert_eq!(&value[search.matches[1].bytes[0].clone()], "_a*<i\u{307}>_");
    }
    #[test]
    fn navigation_survives_backfill_and_stream_updates() {
        let mut s = Search::default();
        s.set_query(
            "needle".into(),
            &[
                f("row-2", "text", "Needle and needle"),
                f("live-id", "text", "partial"),
            ],
        );
        assert_eq!(s.matches.len(), 2);
        assert_eq!(s.step(true).unwrap().bytes, vec![11..17]);
        s.refresh(&[
            f("row-1", "text", "needle"),
            f("row-2", "text", "Needle and needle"),
            f("live-id", "text", "partial needle"),
        ]);
        assert_eq!(s.ordinal(), 3);
        assert_eq!(s.current().unwrap().message, "row-2");
        assert_eq!(s.step(true).unwrap().message, "live-id");
        assert_eq!(s.step(true).unwrap().message, "row-1");
        assert_eq!(s.step(false).unwrap().message, "live-id");
    }
    #[test]
    fn distinct_full_owners_with_same_session_do_not_share_state() {
        #[derive(Hash, PartialEq, Eq)]
        struct Owner {
            source: u64,
            profile_intent: u8,
            session: &'static str,
            workspace: u64,
        }
        let owner = |source, profile_intent, workspace| Owner {
            source,
            profile_intent,
            session: "same",
            workspace,
        };
        let mut searches = Searches::<Owner> {
            views: HashMap::new(),
        };
        searches
            .view(owner(1, 1, 1))
            .search
            .set_query("first".into(), &[f("row-1", "text", "first first")]);
        searches.view(owner(1, 1, 1)).search.step(true);
        searches
            .view(owner(2, 1, 1))
            .search
            .set_query("second".into(), &[f("row-1", "text", "second")]);
        searches
            .view(owner(1, 2, 1))
            .search
            .set_query("third".into(), &[f("row-1", "text", "third")]);
        assert_eq!(searches.view(owner(1, 1, 1)).search.ordinal(), 2);
        assert_eq!(searches.view(owner(2, 1, 1)).search.query, "second");
        assert_eq!(searches.view(owner(1, 2, 1)).search.query, "third");
        assert_eq!(searches.view(owner(1, 1, 2)).search.ordinal(), 0);
    }
    #[test]
    fn long_history_preserves_highlight_identity() {
        let rows: Vec<_> = (0..3000)
            .map(|i| (format!("row-{i}"), format!("message {i} needle")))
            .collect();
        let fragments: Vec<_> = rows.iter().map(|(id, text)| f(id, "text", text)).collect();
        let mut search = Search::default();
        search.set_query("NEEDLE".into(), &fragments);
        assert_eq!(search.matches.len(), 3000);
        let hit = search.step(false).unwrap();
        assert_eq!(hit.message, "row-2999");
        assert_eq!(&rows[2999].1[hit.bytes[0].clone()], "needle");
    }
    #[test]
    fn unicode_ranges_select_original_text_and_empty_query_clears() {
        let mut s = Search::default();
        let text = "Привет ПРИВЕТ İ";
        s.set_query("привет".into(), &[f("row", "text", text)]);
        assert_eq!(
            s.matches
                .iter()
                .map(|m| &text[m.bytes[0].clone()])
                .collect::<Vec<_>>(),
            ["Привет", "ПРИВЕТ"]
        );
        s.set_query("i\u{307}".into(), &[f("row", "text", text)]);
        assert_eq!(&text[s.current().unwrap().bytes[0].clone()], "İ");
        s.set_query(String::new(), &[]);
        assert_eq!(s.ordinal(), 0);
    }
    #[test]
    fn markdown_search_uses_visible_text_and_source_ranges() {
        let source = "A **bold** [link](https://hidden.example) and `code`";
        let mut s = Search::default();
        s.set_query("bold link".into(), &[f("row", "text", source)]);
        let hit = s.current().unwrap();
        assert_eq!(
            hit.bytes
                .iter()
                .map(|r| &source[r.clone()])
                .collect::<String>(),
            "bold link"
        );
        s.set_query("hidden.example".into(), &[f("row", "text", source)]);
        assert!(s.matches.is_empty());
    }
}

#[cfg(test)]
pub(crate) fn synthetic_media() -> super::Media {
    super::Media {
        request_id: 1,
        decoded: false,
        image: None,
        error: None,
        loading: true,
    }
}
