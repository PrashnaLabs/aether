use super::*;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-links-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.rs"), "").unwrap();
    std::fs::write(dir.join("my file.rs"), "").unwrap();
    std::fs::write(dir.join("odd:12"), "").unwrap();
    dir
}

fn at(path: PathBuf, line: Option<usize>, col: Option<usize>) -> Option<FileLink> {
    Some(FileLink { path, line, col })
}

#[test]
fn suffixes_give_line_and_column() {
    let dir = project("suffix");
    let a = dir.join("src/a.rs");
    let parse = |href: &str| parse_file_link(href, Some(&dir));
    assert_eq!(parse("src/a.rs"), at(a.clone(), None, None));
    assert_eq!(parse("src/a.rs:12"), at(a.clone(), Some(12), None));
    assert_eq!(parse("src/a.rs:12:3"), at(a.clone(), Some(12), Some(3)));
    assert_eq!(parse("src/a.rs:12-20"), at(a.clone(), Some(12), None));
    assert_eq!(parse("src/a.rs#L12"), at(a.clone(), Some(12), None));
    assert_eq!(parse("src/a.rs#L12-L20"), at(a.clone(), Some(12), None));
    assert_eq!(parse("src/a.rs#L12C3"), at(a, Some(12), Some(3)));
}

#[test]
fn file_urls_are_percent_decoded() {
    let dir = project("url");
    let url = format!("file://{}#L4", dir.join("my%20file.rs").display());
    assert_eq!(parse_file_link(&url, None), at(dir.join("my file.rs"), Some(4), None));
    let url = format!("file://{}:12", dir.join("my%20file.rs").display());
    assert_eq!(parse_file_link(&url, None), at(dir.join("my file.rs"), Some(12), None));
    let url = format!("file://{}#L12C3", dir.join("my%20file.rs").display());
    assert_eq!(parse_file_link(&url, None), at(dir.join("my file.rs"), Some(12), Some(3)));
    let url = format!("file://{}", dir.join("src/a.rs").display());
    assert_eq!(parse_file_link(&url, None), at(dir.join("src/a.rs"), None, None));
}

#[test]
fn file_urls_take_localhost_but_no_other_host() {
    let dir = project("host");
    let a = dir.join("src/a.rs");
    let url = format!("file://localhost{}#L4", a.display());
    assert_eq!(parse_file_link(&url, None), at(a.clone(), Some(4), None));
    let url = format!("file://server{}", a.display());
    assert_eq!(parse_file_link(&url, None), None);
    assert_eq!(parse_file_link("file://src/a.rs", Some(&dir)), None);
}

#[test]
fn absolute_paths_ignore_cwd_and_relative_ones_need_it() {
    let dir = project("rel");
    let abs = format!("{}:7", dir.join("src/a.rs").display());
    assert_eq!(
        parse_file_link(&abs, Some(Path::new("/nowhere"))),
        at(dir.join("src/a.rs"), Some(7), None)
    );
    assert_eq!(parse_file_link("src/a.rs", None), None);
    assert_eq!(
        parse_file_link("./src/../src/a.rs", Some(&dir)).map(|l| l.path.exists()),
        Some(true)
    );
    assert_eq!(parse_file_link("src", Some(&dir)), at(dir.join("src"), None, None));
}

#[test]
fn missing_files_and_other_schemes_are_not_links() {
    let dir = project("none");
    for href in ["src/b.rs", "src/b.rs:3", "", "https://example.com", "mailto:a@b.c", "http:src"] {
        assert_eq!(parse_file_link(href, Some(&dir)), None, "{href}");
    }
}

#[test]
fn a_colon_name_that_exists_keeps_its_suffix() {
    let dir = project("colon");
    assert_eq!(parse_file_link("odd:12", Some(&dir)), at(dir.join("odd:12"), None, None));
}

#[test]
fn inline_code_naming_a_file_becomes_a_link() {
    let dir = project("inline");
    let a = dir.join("src/a.rs").display().to_string();
    let linked = link_inline_code("See `src/a.rs:4` and `src/b.rs`.", &dir);
    assert_eq!(linked, format!("See [`src/a.rs:4`](<{a}#L4>) and `src/b.rs`."));

    let root = markdown::to_mdast(&linked, &markdown::ParseOptions::gfm()).unwrap();
    let markdown::mdast::Node::Paragraph(p) = &root.children().unwrap()[0] else { panic!() };
    let markdown::mdast::Node::Link(link) = &p.children[1] else { panic!("{:?}", p.children) };
    assert!(matches!(link.children[..], [markdown::mdast::Node::InlineCode(_)]));
    assert_eq!(parse_file_link(&link.url, None), at(dir.join("src/a.rs"), Some(4), None));

    let linked = link_inline_code("`src/a.rs:4:2`", &dir);
    assert_eq!(linked, format!("[`src/a.rs:4:2`](<{a}#L4C2>)"));
}

#[test]
fn inline_code_in_fences_links_or_with_spaces_stays() {
    let dir = project("skip");
    for text in [
        "```\n`src/a.rs`\n```",
        "[the `src/a.rs` file](https://x.y)",
        "run `cat src/a.rs`",
        "pass `--src/a.rs`",
        "no code here",
    ] {
        assert_eq!(link_inline_code(text, &dir), text);
    }
}

#[gpui_kit::test]
fn markdown_routes_web_links_to_the_app_and_keeps_file_links_separate(
    cx: &mut gpui_kit::TestAppContext,
) {
    use std::cell::RefCell;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let web_calls = calls.clone();
    let file_calls = calls.clone();
    cx.update(|cx| {
        cx.set_global(OpenWebLink(Rc::new(move |href, _, _| {
            web_calls.borrow_mut().push(href.to_string());
            true
        })));
        cx.set_global(OpenFileLink(Rc::new(move |link, _, _| {
            file_calls.borrow_mut().push(link.path.display().to_string());
            true
        })));
    });
    let dir = project("routing");
    let window = cx.add_window(|_, _| gpui_kit::EmptyView);
    window
        .update(cx, |_, window, cx| {
            open_link("https://example.com/docs#intro", None, window, cx);
            open_link("src/a.rs:7", Some(&dir), window, cx);
        })
        .unwrap();
    assert_eq!(
        *calls.borrow(),
        vec![
            "https://example.com/docs#intro".to_string(),
            dir.join("src/a.rs").display().to_string()
        ]
    );
    std::fs::remove_dir_all(dir).unwrap();
}
