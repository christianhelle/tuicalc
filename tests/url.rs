use tuicalc::url::find_url;

#[test]
fn finds_a_whole_cell_of_url() {
    assert_eq!(
        find_url("https://example.com/a?b=1&c=2").as_deref(),
        Some("https://example.com/a?b=1&c=2")
    );
}

#[test]
fn finds_plain_http() {
    assert_eq!(
        find_url("http://example.com").as_deref(),
        Some("http://example.com")
    );
}

#[test]
fn finds_a_url_among_other_words() {
    assert_eq!(
        find_url("see https://example.com/docs for more").as_deref(),
        Some("https://example.com/docs")
    );
}

#[test]
fn ignores_the_scheme_case() {
    assert_eq!(
        find_url("HTTPS://Example.com").as_deref(),
        Some("HTTPS://Example.com")
    );
}

#[test]
fn adds_https_to_a_bare_www_address() {
    assert_eq!(
        find_url("www.example.com").as_deref(),
        Some("https://www.example.com")
    );
}

#[test]
fn leaves_off_trailing_punctuation() {
    assert_eq!(
        find_url("Go to https://example.com.").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        find_url("(https://example.com)").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        find_url("\"https://example.com\",").as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn keeps_a_closing_parenthesis_that_belongs_to_the_url() {
    assert_eq!(
        find_url("https://en.wikipedia.org/wiki/Rust_(programming_language)").as_deref(),
        Some("https://en.wikipedia.org/wiki/Rust_(programming_language)")
    );
}

#[test]
fn takes_the_first_of_several() {
    assert_eq!(
        find_url("https://a.example https://b.example").as_deref(),
        Some("https://a.example")
    );
}

#[test]
fn finds_nothing_without_a_url() {
    assert_eq!(find_url(""), None);
    assert_eq!(find_url("just some text"), None);
    assert_eq!(find_url("https://"), None);
    assert_eq!(find_url("ftp://example.com"), None);
    assert_eq!(find_url("mailto:someone@example.com"), None);
}
