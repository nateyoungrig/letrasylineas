//! Explicit opt-in smoke test: uses the user's installed Codex and subscription quota.
use letras_core::{Ai, models::LineContext, provider::Provider, transcribe::DraftLine};

#[tokio::test]
#[ignore = "requires Codex CLI signed in with ChatGPT; uses subscription quota"]
async fn codex_all_five_operations() {
    assert!(letras_core::codex::logged_in().await.unwrap());
    let ai = Ai::new(Provider::Chatgpt, Provider::Chatgpt.default_model().into(), None).unwrap();
    // Invented ordinary prose, not lyrics or copyrighted source material.
    let ctx = LineContext {
        line_id: 1,
        title: "Language exercise".into(),
        artist: "".into(),
        source: "pasted".into(),
        lyrics: "→ Hoy llevo una camisa azul.\n".into(),
        line: "Hoy llevo una camisa azul.".into(),
        untranslated: vec![],
    };
    let grade = ai.grade(&ctx, "Today I am wearing a blue shirt.").await.unwrap();
    assert!((1..=5).contains(&grade.score));
    assert!(!grade.verdict.is_empty());
    assert!(!grade.best_translation.is_empty());
    let followup = ai
        .follow_up(
            &ctx,
            "Today I am wearing a blue shirt.",
            &grade,
            &[],
            "Why does llevo mean wearing here?",
        )
        .await
        .unwrap();
    assert!(!followup.trim().is_empty());
    let tutor = ai
        .tutor(
            "The learner is practicing everyday vocabulary.",
            &[],
            "Give a short explanation of ser versus estar, without song lyrics.",
        )
        .await
        .unwrap();
    assert!(!tutor.trim().is_empty());
    let lines = vec![DraftLine {
        text: "Hoy llevo una camisa azul.".into(),
        start: 0.0,
        end: 2.0,
        stanza: 0,
        words: vec![],
        flags: vec![],
    }];
    let proofread = ai.proofread("Language exercise", "", &lines).await.unwrap();
    assert!(proofread.suggestions.iter().all(|s| s.line == 0));
    let discussion = ai
        .discuss(
            "Language exercise",
            "",
            &lines,
            0,
            &[],
            None,
            &[],
            "Is camisa a Spanish word? Answer briefly without translating the sentence.",
        )
        .await
        .unwrap();
    assert!(!discussion.trim().is_empty());
}
