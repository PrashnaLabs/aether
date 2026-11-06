use super::*;

#[test]
fn parses_results_and_skips_ads() {
    let html = r#"
            <div class="result web-result result--ad"><a class="result__a" href="https://ad.example">Ad</a></div>
            <div class="result web-result">
              <h2><a class="result__a" href="https://gpui.rs/">GPUI</a></h2>
              <a class="result__snippet">A fast <b>UI</b> framework &amp; more</a>
            </div>"#;
    assert_eq!(
        parse(html),
        [SearchResult {
            title: "GPUI".into(),
            url: "https://gpui.rs/".into(),
            snippet: "A fast UI framework & more".into(),
        }]
    );
}
