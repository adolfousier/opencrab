//! The Telegram model picker encodes each model button in `callback_data`,
//! which Telegram caps at 64 BYTES. Long custom-provider model names overflow:
//! modelscope's `deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B` pushes
//! `model:custom:modelscope|<name>` to 65 bytes. Telegram then rejects the
//! ENTIRE inline keyboard (BUTTON_DATA_INVALID), and because the
//! `edit_message_text` error was swallowed the picker rendered nothing and
//! spun on "loading" forever — while short-name providers (dialagram) worked.
//!
//! The fix encodes a compact index (`model:<provider>|#<i>`) whenever the
//! literal would overflow; the parser resolves it back via the same config
//! list. These pin the 64-byte guarantee and the round-trip.

use crate::channels::commands::model_button_callback_data;

#[test]
fn long_modelscope_names_stay_within_telegrams_64_byte_callback_limit() {
    let overflowing = [
        "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B",
        "deepseek-ai/DeepSeek-R1-Distill-Llama-70B",
    ];
    for (i, model) in overflowing.iter().enumerate() {
        let literal = format!("model:custom:modelscope|{model}");
        assert!(
            literal.len() > 64,
            "precondition: {model} literal must overflow, was {} bytes",
            literal.len()
        );

        let data = model_button_callback_data("custom:modelscope", model, i);
        assert!(
            data.len() <= 64,
            "callback_data must fit Telegram's 64-byte limit, got {} bytes: {data}",
            data.len()
        );
        // Overflowing names fall back to the index form the parser resolves.
        assert_eq!(data, format!("model:custom:modelscope|#{i}"));
    }
}

#[test]
fn short_names_keep_the_literal_form_for_backward_compat() {
    // dialagram's names are short — they stay literal so existing buttons and
    // any other caller of the `model:` callback keep working unchanged.
    let data = model_button_callback_data("custom:dialagram", "qwen-3.7-max", 0);
    assert_eq!(data, "model:custom:dialagram|qwen-3.7-max");
    assert!(data.len() <= 64);
}

#[test]
fn index_token_round_trips_through_the_parser_shape() {
    // Mirror the handler's parse: strip "model:" → split '|' → strip '#' → idx.
    let data = model_button_callback_data(
        "custom:modelscope",
        "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B",
        7,
    );
    let rest = data.strip_prefix("model:").expect("model: prefix");
    let (provider, token) = rest.split_once('|').expect("pipe separator");
    assert_eq!(provider, "custom:modelscope");
    let idx: usize = token
        .strip_prefix('#')
        .expect("index form")
        .parse()
        .expect("parseable index");
    assert_eq!(idx, 7);
}
