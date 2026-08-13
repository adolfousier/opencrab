//! The `preserve_thinking` host gate (#1033).
//!
//! DashScope reasoning models need the flag to carry reasoning across turns.
//! Without it the model re-derives its reasoning every turn, which shows up as
//! drifting and invented intermediate steps on long sessions.

use crate::brain::provider::qwen::is_dashscope_host;
use crate::brain::provider::qwen_reasoning::preserve_thinking_for;

#[test]
fn preserve_thinking_follows_the_host_not_the_model() {
    for url in [
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        "https://dashscope-us.aliyuncs.com/compatible-mode/v1",
        "https://bailian.console.aliyun.com/v1",
    ] {
        assert!(preserve_thinking_for(url), "{url} is a DashScope endpoint");
        assert!(is_dashscope_host(url));
    }
}

#[test]
fn a_locally_served_qwen_gets_no_dashscope_fields() {
    // Matches on model name but not on host: llama.cpp / LM Studio take
    // thinking through `chat_template_kwargs`, not DashScope request fields.
    for url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:8080/v1",
        "https://api.openai.com/v1",
    ] {
        assert!(!preserve_thinking_for(url), "{url} is not DashScope");
        assert!(!is_dashscope_host(url));
    }
}
