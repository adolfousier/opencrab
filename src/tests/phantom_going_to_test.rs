//! Regression: the phantom self-heal must catch the "I'm going to use <tool>"
//! narration shape — in every supported language.
//!
//! 2026-06-12: mimo-v2.5-pro narrated "I'm going to use write_file to create the
//! new file", emitted zero structured tool calls, and the self-heal never fired.
//! Root cause: the English patterns had `let me`/`I'll`/`now`/`I need to` but NOT
//! the `I'm going to <verb>` form — the single clearest "100% about to act"
//! signal. es/fr/pt/ru already carried their equivalent (`voy a`, `je vais`,
//! `vou`, `сейчас`), which is why Russian worked; this pins all five plus the
//! `use`/`write` verbs the others were missing.

use crate::brain::agent::service::phantom::has_phantom_tool_intent_no_tools;

#[test]
fn english_im_going_to_is_phantom() {
    assert!(has_phantom_tool_intent_no_tools(
        "I'm going to use write_file to create the new file."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "I am going to update the config and commit the changes."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Let me just go ahead and write the file now."
    ));
}

#[test]
fn going_to_use_detected_in_every_language() {
    assert!(
        has_phantom_tool_intent_no_tools("Voy a usar write_file para crear el archivo nuevo."),
        "es"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Je vais utiliser write_file pour créer le fichier."),
        "fr"
    );
    assert!(
        has_phantom_tool_intent_no_tools("Vou usar write_file para criar o arquivo novo."),
        "pt"
    );
    assert!(
        has_phantom_tool_intent_no_tools(
            "Сейчас использую write_file чтобы создать новый файл проекта."
        ),
        "ru"
    );
}

#[test]
fn ordinary_answer_is_not_phantom() {
    // A normal explanation that merely mentions a tool must NOT trip it.
    assert!(!has_phantom_tool_intent_no_tools(
        "The write_file tool writes content to disk. It takes a path and the text."
    ));
}
