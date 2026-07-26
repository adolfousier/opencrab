use crate::brain::agent::service::phantom::*;

#[test]
fn english_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Let me check the logs and fix the issue."
    ));
}

#[test]
fn russian_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Давайте проверю логи и исправлю ошибку."
    ));
}

#[test]
fn spanish_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Déjame revisar el archivo y voy a actualizar la configuración ¿ok?"
    ));
}

#[test]
fn portuguese_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Vou verificar o arquivo e corrigir a configuração do irmão"
    ));
}

#[test]
fn french_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Laissez-moi vérifier le fichierête et corriger l'erreur être"
    ));
}

#[test]
fn structured_answer_not_phantom() {
    // A table/list answer should never be flagged
    let table = "| Commit | Message |\n|---|---|\n| abc123 | fix stuff |\n";
    assert!(!has_phantom_tool_intent_no_tools(table));
}

#[test]
fn short_text_not_phantom() {
    assert!(!has_phantom_tool_intent_no_tools("ok"));
}

#[test]
fn english_completion_claim() {
    assert!(has_phantom_tool_intent(
        "I've updated the file and all changes have been applied."
    ));
}

#[test]
fn english_trailing_colon() {
    assert!(has_phantom_tool_intent(
        "Let me check the logs and verify the configuration settings:"
    ));
}

#[test]
fn english_intent_with_path() {
    assert!(has_phantom_tool_intent(
        "Let me update src/main.rs with the new configuration."
    ));
}

#[test]
fn investigative_intent_english() {
    assert!(has_investigative_intent("Let me dig into this issue."));
}

#[test]
fn stuck_in_intent_loop_english() {
    // A genuine loop: the SAME intent line repeated.
    let text = "Let me check the logs\nLet me check the logs\nLet me check the logs\n";
    assert!(is_stuck_in_intent_loop(text));
}

#[test]
fn varied_multistep_plan_is_not_a_loop() {
    // Distinct intent lines = a legitimate plan, NOT a phantom loop. This
    // is the false positive that killed planful replies (Telegram 16:13).
    let text = "Let me check the logs\nLet me verify the config\nLet me read the file\n";
    assert!(!is_stuck_in_intent_loop(text));
}

#[test]
fn not_stuck_single_intent() {
    let text = "Let me check the logs and see what happened.";
    assert!(!is_stuck_in_intent_loop(text));
}

#[test]
fn looks_truncated() {
    assert!(looks_truncated_mid_sentence(
        "This is a long response that got cut off in the middle of a wor"
    ));
}

#[test]
fn not_truncated_with_period() {
    assert!(!looks_truncated_mid_sentence(
        "This is a complete response that ends with a period."
    ));
}

// ── #463: language coverage and the language-agnostic tool-name tell ──

#[test]
fn past_tense_loaded_claim_is_phantom() {
    // "loaded" was missing from action_verbs; the live hallucination closed
    // with this exact claim and zero tool calls.
    assert!(has_phantom_tool_intent_no_tools(
        "Both loaded properly into context via load_brain_file. Ready for the next task."
    ));
}

#[test]
fn indonesian_intent_phrases_detected() {
    // Indonesian is ASCII-Latin, so char-set language detection cannot route
    // it; the intent scan checks all loaded languages at once.
    assert!(has_phantom_tool_intent_no_tools(
        "Sekarang gue jalankan tool-nya buat load file itu ke context."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Biar gue load dua-duanya dengan benar sekarang."
    ));
}

#[test]
fn registered_tool_mention_detected_word_boundary() {
    let names = vec![
        "load_brain_file".to_string(),
        "read_file".to_string(),
        "bash".to_string(),
        "plan".to_string(),
    ];
    // Underscore names match at word boundaries, any language.
    assert!(mentions_registered_tool(
        "Both loaded properly into context via load_brain_file.",
        &names
    ));
    assert!(mentions_registered_tool(
        "Gue udah load pakai read_file which is file-level access.",
        &names
    ));
    // Substrings of larger identifiers do not match.
    assert!(!mentions_registered_tool(
        "the loader_read_filesystem module handles this",
        &names
    ));
    // Single-word tool names are ordinary prose words: never matched.
    assert!(!mentions_registered_tool(
        "run it in bash and plan accordingly",
        &names
    ));
}

// Regression fixture (#589): a reasoning-model planning monologue that leads
// with a structured task-restatement (a numbered list) BEFORE the real
// narration. `prose_lead_in` truncates at the first numbered line, so the
// lead-in-only detector `has_phantom_tool_intent_no_tools` sees only a benign
// description and misses it. The strict full-text `has_phantom_tool_intent`
// must catch it via the ">=2 'Let me' line-starts" and ">=2 numbered action
// steps" signals. Synthetic content — same structure, no real request quoted.
const STRUCTURED_PREAMBLE_PHANTOM: &str = "The user wants a full audit of the dashboard module. They described the intended behaviour:\n\n1. **Widgets tab** structure:\n   - list view: items created by the user.\n   - detail view: interactions from other users.\n\nThey want me to: break down this context, verify what's in the codebase, and audit for gaps.\n\nThe system reminder says I described changes but did not execute tool calls.\n\nLet me explore the dashboard section of the codebase:\n- lib/dashboard/ probably exists\n\nLet me start by exploring the directory structure.\n\nPlan of attack:\n1. ls the dashboard directory structure\n2. Look at the main dashboard screen\n3. Check each sub-view\n4. Check notification logic\n5. Check the integration\n\nLet me start. I'll make parallel independent calls where possible.\n\nFirst batch:\n- ls lib/dashboard/ recursive\n\nLet me go.\n\nLet me start exploring.";

#[test]
fn structured_preamble_phantom_slips_leadin_but_strict_catches() {
    // Premise changed by #783, deliberately. The lead-in detector used to MISS
    // this: the numbered list truncated prose_lead_in to a benign description
    // and every "Let me start" below it was invisible. #589 worked around that
    // by ORing the strict full-text detector into ONE of the five gates, so the
    // other four stayed blind. The window now covers both ends of a turn, so
    // the lead-in detector catches it directly and every gate is closed.
    assert!(
        has_phantom_tool_intent_no_tools(STRUCTURED_PREAMBLE_PHANTOM),
        "lead-in detector must now catch a buried announcement (#783)"
    );
    // The fix (#589): the strict full-text detector DOES catch it, so wiring it
    // into the retry gate closes the hole.
    assert!(
        has_phantom_tool_intent(STRUCTURED_PREAMBLE_PHANTOM),
        "strict detector must catch the buried 'Let me ...' + numbered-step narration"
    );
}

// Regression: a leading-imminence work announcement after a real tool call
// ("Issue #<n> filed. Now downloading the font files ... as local assets.").
// The work-announcement regex needs a TRAILING imminence marker (now / … / :),
// so an announcement that leads with "Now" and ends on a plain period slipped
// every detector, and the post-success eligibility gate never let phantom
// detection run — so the model narrated "Now downloading ..." with no tool call
// and no retry fired. `matches_now_gerund` + the broadened gerund verb list fix
// it (downloading was also missing from the gerund verb set). Synthetic issue
// number, no real request quoted.
#[test]
fn leading_now_gerund_announcement_detected() {
    let t = "Issue filed. Now downloading the font files (Fraunces, Inter, DM Mono, Source Code Pro) as local assets.";
    // Zero-tool relaxed detector catches it.
    assert!(
        has_phantom_tool_intent_no_tools(t),
        "no_tools must catch leading-now gerund announcement"
    );
    // Post-success eligibility gate (tools already ran this turn) catches it —
    // this is the gate that let the live case through.
    assert!(
        has_forward_intent_post_success(t),
        "post-success gate must treat 'Now downloading ...' as forward-looking"
    );
    // The shared helper matches the leading-now form directly.
    assert!(matches_now_gerund(t), "matches_now_gerund must fire");
}

// Regression: an English planning monologue with a single accented word
// ("Activité") was mis-detected as French by detect_language (which flips on any
// accent), so the strict detector used French patterns and missed the English
// "Let me ..." narration — the phantom slipped and no retry fired (Kimi K3).
// The strict detector now scans ALL languages, so a mis-detected language can't
// disable it. Synthetic content, no real request quoted.
#[test]
fn english_monologue_with_accent_still_detected() {
    let t = "New task: investigate why the \"Activité\" sub-tab shows no properties even though the MATCH counter header shows a number.\n\nLet me parse the report carefully.\n\nLet me investigate the widget.\n\nLet me start by reading the activity feed widget and finding where the match counter gets its number.";
    assert!(
        has_phantom_tool_intent(t),
        "strict detector must catch the English 'Let me ...' narration despite the accented word mis-detecting as French"
    );
    // And it still works with the accent stripped (plain English path).
    assert!(has_phantom_tool_intent(&t.replace('é', "e")));
}
