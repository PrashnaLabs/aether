use super::*;

#[test]
fn breadcrumb_aria_joins_project_and_title() {
    assert_eq!(
        breadcrumb_aria(Some("auto-clicker"), "Project Purpose"),
        "auto-clicker / Project Purpose"
    );
    assert_eq!(breadcrumb_aria(None, "Scratch"), "Scratch");
}

#[test]
fn project_crumb_aria_names_project() {
    assert_eq!(project_page_aria("auto-clicker"), "Open project page for auto-clicker");
}

#[test]
fn delegated_link_aria_names_lead() {
    assert_eq!(delegated_aria("Plan the refactor"), "Open lead Plan the refactor");
}
