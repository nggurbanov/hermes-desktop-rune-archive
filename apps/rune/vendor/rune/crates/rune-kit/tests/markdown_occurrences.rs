use rune_kit::markdown::{Block, link_occurrences_cached, parse_cached};

#[test]
fn authored_occurrences_retain_ranges_and_labels() {
    let src = "é **bold** [one](https://example.test/a) and [two](https://example.test/a)";
    let links = link_occurrences_cached(src);
    assert_eq!(links.len(), 2);
    assert_eq!(&src[links[0].source.clone()], "[one](https://example.test/a)");
    assert_eq!(&src[links[1].source.clone()], "[two](https://example.test/a)");
    assert_eq!(links[0].destination, links[1].destination);
    let blocks = parse_cached(src);
    let Block::Para(inline) = &blocks[0] else { panic!("paragraph") };
    assert_eq!(&inline.text[links[0].label.clone()], "one");
    assert_eq!(&inline.text[links[1].label.clone()], "two");
    assert_eq!(inline.links.len(), 2);
    assert_eq!(link_occurrences_cached(src), links);
}

#[test]
fn nested_reference_and_code_follow_parser_events() {
    let src = "- [list](list)\n\n> [quote](quote)\n\n| [head](head) | x |\n| --- | --- |\n| [cell](cell) | y |\n\n[ref][id] and <https://example.test>\n\n[id]: /target\n\n`[inline](no)`\n\n```\n[fence](no)\n```\n";
    let links = link_occurrences_cached(src);
    let slices: Vec<_> = links.iter().map(|link| &src[link.source.clone()]).collect();
    assert_eq!(
        slices,
        ["[list](list)", "[quote](quote)", "[head](head)", "[cell](cell)", "[ref][id]", "<https://example.test>"]
    );
    assert_eq!(links[4].destination, "/target");
    assert_eq!(links[5].destination, "https://example.test");
}

#[test]
fn cache_stream_and_edit_preserve_offsets_and_rendering() {
    let prefix = "[first](same)\n\n";
    let initial = format!("{prefix}[second](same)\n");
    let extended = format!("{initial} and [third](same)");
    let before = link_occurrences_cached(&initial);
    let before_blocks = parse_cached(&initial);
    let after = link_occurrences_cached(&extended);
    assert_eq!(after[0], before[0]);
    assert_eq!(&extended[after[1].source.clone()], "[second](same)");
    assert_eq!(&extended[after[2].source.clone()], "[third](same)");
    assert_eq!(parse_cached(&extended)[0], before_blocks[0]);
    let edited = format!("é {extended}");
    let shifted = link_occurrences_cached(&edited);
    for (old, new) in after.iter().zip(&shifted) {
        assert_eq!(new.source, old.source.start + 3..old.source.end + 3);
        assert_eq!(&edited[new.source.clone()], &extended[old.source.clone()]);
    }
}
