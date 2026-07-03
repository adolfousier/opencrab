//! Tests for main-content isolation. Prove the selector cascade prefers real
//! content containers over surrounding chrome, that the body fallback strips
//! nav/footer/aside when no container matches, and that content images survive
//! extraction (they carry into the markdown for on-demand vision).

use crate::brain::tools::web_scrape::extract::extract_main_content;

const PADDING: &str =
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor";

#[test]
fn prefers_main_over_surrounding_chrome() {
    let html = format!(
        r#"<html><body>
            <nav>site navigation links here</nav>
            <main><p>The real article body. {PADDING}</p></main>
            <footer>copyright and contact footer</footer>
        </body></html>"#
    );
    let out = extract_main_content(&html);
    assert!(out.contains("The real article body"));
    assert!(!out.contains("site navigation"));
    assert!(!out.contains("copyright and contact"));
}

#[test]
fn falls_back_to_article_when_no_main() {
    let html = format!(
        r#"<html><body>
            <div class="header">header stuff</div>
            <article><p>Article content lives here. {PADDING}</p></article>
        </body></html>"#
    );
    let out = extract_main_content(&html);
    assert!(out.contains("Article content lives here"));
}

#[test]
fn body_fallback_strips_nav_and_footer() {
    // No recognized content container — fall back to <body> minus junk.
    let html = format!(
        r#"<html><body>
            <nav>menu one menu two menu three</nav>
            <div><p>Plain content with no semantic wrapper. {PADDING}</p></div>
            <footer>footer boilerplate text here</footer>
            <aside>related sidebar widgets here</aside>
        </body></html>"#
    );
    let out = extract_main_content(&html);
    assert!(out.contains("Plain content with no semantic wrapper"));
    assert!(!out.contains("menu one menu two"));
    assert!(!out.contains("footer boilerplate"));
    assert!(!out.contains("related sidebar"));
}

#[test]
fn preserves_content_images() {
    let html = format!(
        r#"<html><body><main>
            <p>See the chart. {PADDING}</p>
            <img src="https://cdn.example.com/q3.png" alt="Q3 revenue">
        </main></body></html>"#
    );
    let out = extract_main_content(&html);
    assert!(out.contains("https://cdn.example.com/q3.png"));
    assert!(out.contains("Q3 revenue"));
}

#[test]
fn returns_something_for_bodyless_input() {
    // Pathological input with no body still yields a non-empty fragment so the
    // caller always has something to convert.
    let out = extract_main_content("<p>bare fragment</p>");
    assert!(out.contains("bare fragment"));
}
