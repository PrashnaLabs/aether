use super::*;

fn skill(name: &str) -> Skill {
    Skill {
        name: name.into(),
        description: String::new(),
        instructions: String::new(),
        source: SkillSource::Aether,
        dir: PathBuf::new(),
    }
}

#[test]
fn finds_the_mention_being_typed() {
    assert_eq!(mention_at("hi @sum", 7), Some((3, "sum")));
    assert_eq!(mention_at("@", 1), Some((0, "")));
    assert_eq!(mention_at("hi @sum more", 7), Some((3, "sum")));
    assert_eq!(mention_at("hi @sum more", 12), None);
    assert_eq!(mention_at("mail@sum", 8), None);
    assert_eq!(mention_at("@Sum", 4), None);
}

#[test]
fn completes_prefix_matches_first() {
    let skills = [skill("recap"), skill("meeting-recap"), skill("tone"), skill("recap")];
    let names: Vec<&str> =
        complete_skill("rec", &skills, 5).iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["recap", "meeting-recap"]);
    assert_eq!(complete_skill("", &skills, 2).len(), 2);
}

#[test]
fn names_follow_the_spec() {
    assert!(valid_skill_name("code-review2"));
    for bad in ["", "Code", "-a", "a-", "a--b", "a b", "a_b"] {
        assert!(!valid_skill_name(bad), "{bad}");
    }
}

#[test]
fn mentions_match_known_skills_once() {
    let skills = [skill("summarize"), skill("tone")];
    let found =
        mentioned_skills("@summarize this, then @tone. Again @summarize, @missing", &skills);
    let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["summarize", "tone"]);
    assert!(mentioned_skills("email@summarize.com", &skills).is_empty());
}
