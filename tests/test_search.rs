use pimonitor::find_feed_index_by_query;

#[test]
fn empty_query_does_nothing() {
    let feeds = vec![(1u64, "One"), (2, "Two")];
    assert_eq!(find_feed_index_by_query(&feeds, ""), None);
    assert_eq!(find_feed_index_by_query(&feeds, "   "), None);
}

#[test]
fn numeric_id_search() {
    let feeds = vec![(10u64, "Ten"), (20, "Twenty"), (30, "Thirty")];
    assert_eq!(find_feed_index_by_query(&feeds, "20"), Some(1));
    assert_eq!(find_feed_index_by_query(&feeds, "999"), None);
}

#[test]
fn word_conjunction_case_insensitive() {
    let feeds = vec![
        (1u64, "The Daily Tech News Show"),
        (2u64, "Daily Science Podcast"),
        (3u64, "Random Talk"),
    ];
    // all words must appear
    assert_eq!(find_feed_index_by_query(&feeds, "daily news"), Some(0));
    // case-insensitive and substring matching
    assert_eq!(find_feed_index_by_query(&feeds, "SCIENCE"), Some(1));
    // conjunction fails -> no match
    assert_eq!(find_feed_index_by_query(&feeds, "daily random"), None);
}

#[test]
fn numeric_zero_or_negative_not_treated_as_id() {
    let feeds = vec![(1u64, "Zero Show")];
    // leading plus/minus or zero shouldn't match id mode; falls back to words.
    assert_eq!(find_feed_index_by_query(&feeds, "0"), None);
    // "-1" parse fails to u64 so becomes word search and won't match title
    assert_eq!(find_feed_index_by_query(&feeds, "-1"), None);
}
