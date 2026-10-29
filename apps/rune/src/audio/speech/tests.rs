use super::*;
#[test]
fn retained_speech_corpus_preserves_prose_and_silences_machine_identifiers() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("identifier_corpus.json")).unwrap();
    for rows in ["identifier_tokens", "pass_through_tokens"] {
        for case in corpus[rows].as_array().unwrap() {
            let actual = sanitize(case[0].as_str().unwrap());
            for absent in case[1].as_array().unwrap() {
                assert!(!actual.contains(absent.as_str().unwrap()), "{actual}");
            }
            for present in case[2].as_array().unwrap() {
                assert!(actual.contains(present.as_str().unwrap()), "{actual}");
            }
        }
    }
}
#[test]
fn fenced_code_links_and_table_bodies_are_silent_without_english_placeholders() {
    assert_eq!(
        sanitize("Here is code:\n```ts\nconst x = 1\n```\nDone."),
        "Here is code. Done."
    );
    assert_eq!(
        sanitize("Before.\n\n| Item | Value |\n| --- | ---: |\n| private body | 42 |\n\nAfter."),
        "Before. Item, Value. After."
    );
    assert_eq!(
        sanitize("See MEDIA:/tmp/report-2026-q3.xlsx. Then reply."),
        "See. Then reply."
    );
    assert_eq!(
        sanitize("Use `git status` and [the guide](https://example.com)."),
        "Use git status and the guide."
    );
    assert_eq!(
        sanitize("**First sentence.**\n\nSecond sentence."),
        "First sentence. Second sentence."
    );
}
