use super::*;

#[test]
fn tolerates_claude_code_tool_strings() {
    let content =
        "---\nname: merge\ndescription: Merge branches.\nallowed-tools: Read, Bash\n---\nSteps.";
    let path = Path::new("merge/SKILL.md");
    assert!(parse_skill_markdown(path, content).is_err());
    let parsed = parse_skill_markdown(path, &without_tool_list(content)).unwrap();
    assert_eq!((parsed.name.as_str(), parsed.body.trim()), ("merge", "Steps."));
}

#[test]
fn drafts_round_trip_through_the_parser() {
    let markdown = SkillDraft::new("tone", "Rewrite text: friendly, \"plain\" words.")
        .with_body("Keep it short.")
        .to_markdown()
        .unwrap();
    let parsed = parse_skill_markdown(Path::new("tone/SKILL.md"), &markdown).unwrap();
    assert_eq!(parsed.name, "tone");
    assert_eq!(parsed.description, "Rewrite text: friendly, \"plain\" words.");
    assert_eq!(parsed.body.trim(), "Keep it short.");
}

#[test]
#[ignore = "needs agent skills installed on this machine"]
fn loads_installed_agent_skills() {
    let skills = load_all();
    assert!(!skills.is_empty(), "expected skills to be found across sources");
}
