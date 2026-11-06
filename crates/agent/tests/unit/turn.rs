use super::*;

fn skill(name: &str) -> Skill {
    Skill {
        name: name.into(),
        description: format!("{name} things"),
        instructions: format!("Do {name}."),
        source: aether_core::SkillSource::Aether,
        dir: Default::default(),
    }
}

fn settings(web_search: bool, plan_mode: bool, instructions: &str) -> Settings {
    Settings { web_search, plan_mode, instructions: instructions.into(), ..Settings::default() }
}

#[test]
fn system_prompt_adds_search_plan_and_user_instructions() {
    let plain = system_prompt(&settings(false, false, "  "), &[], &[]);
    assert_eq!(plain, format!("{BASE_PROMPT} {ASK_PROMPT}"));
    let prompt = system_prompt(&settings(true, true, " Be brief. "), &[], &[]);
    assert!(prompt.contains(SEARCH_PROMPT) && prompt.contains(PLAN_PROMPT));
    assert!(prompt.ends_with("User instructions:\nBe brief."));
}

#[test]
fn system_prompt_lists_skills_and_inlines_invoked_ones() {
    let skills = [skill("tone"), skill("recap")];
    let prompt = system_prompt(&settings(false, false, ""), &skills, &skills[1..]);
    assert!(prompt.contains("- tone: tone things\n- recap: recap things"));
    assert!(prompt.ends_with("## recap\nDo recap."));
    assert!(!prompt.contains("Do tone."));
}

#[test]
fn user_content_inlines_short_text_files_and_sends_the_rest_as_data() {
    let dir = std::env::temp_dir().join(format!("aether-turn-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, mime: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        Attachment { name: name.into(), path, mime: mime.into(), size: bytes.len() as u64 }
    };
    let files = [
        file("a.rs", "text/plain", b"fn main() {}"),
        file("a.png", "image/png", &[1, 2, 3]),
        file("big.txt", "text/plain", &vec![b'x'; INLINE_TEXT_BYTES as usize + 1]),
    ];
    let content = user_content("look".into(), &files).unwrap();
    let text = |p: &Part| match p {
        Part::Text { text } => Some(text.clone()),
        _ => None,
    };
    let mime = |p: &Part| match p {
        Part::InlineData { mime_type, .. } => Some(mime_type.clone()),
        _ => None,
    };
    assert_eq!(content.parts.len(), 4);
    assert_eq!(text(&content.parts[0]).as_deref(), Some("look"));
    assert_eq!(text(&content.parts[1]).as_deref(), Some("File: a.rs\n```\nfn main() {}\n```"));
    assert_eq!(mime(&content.parts[2]).as_deref(), Some("image/png"));
    assert_eq!(mime(&content.parts[3]).as_deref(), Some("text/plain"));
    let only = user_content(String::new(), &files[1..2]).unwrap();
    assert_eq!(only.parts.len(), 1);
    let gone = Attachment { path: dir.join("gone.png"), ..files[1].clone() };
    assert!(user_content("x".into(), &[gone]).is_err());
    let huge = file("huge.pdf", "application/pdf", &vec![0; DIRECT_INLINE_BYTES as usize + 1]);
    let Err(e) = user_content("x".into(), &[huge]) else { panic!("huge file was sent") };
    assert!(e.to_string().contains("huge.pdf is over 10 MB"), "{e}");
}
