use super::*;

#[test]
fn font_choices_start_with_default_and_hide_private_families() {
    let installed = ["Zapfino", ".SFNS", "Arial", "Arial"].map(String::from);
    let names: Vec<_> = font_choices(&installed).into_iter().map(|f| f.name.to_string()).collect();
    assert_eq!(names, ["Default", "Arial", "Zapfino"]);
}

#[test]
fn font_choices_sort_ignoring_case_with_default_first() {
    let installed = ["Zapfino", "iA Writer", "Arial", "arial", "Arial"].map(String::from);
    let names: Vec<_> = font_choices(&installed).into_iter().map(|f| f.name.to_string()).collect();
    assert_eq!(names, ["Default", "Arial", "arial", "iA Writer", "Zapfino"]);
}

#[test]
fn font_choices_skip_an_empty_family() {
    assert_eq!(font_choices(&[String::new()]).len(), 1);
}
