//! Tests for `agent::service::compaction_prompts`.
//!
//! Regression context (2026-05-29): a Telegram user (leshchenko1979)
//! forwarded a post-compaction one-liner — the model dropping into
//! Russian мат when expressing frustration after recovering context —
//! to friends, calling it a "Skynet" moment. Earlier we had silenced
//! all post-compaction narration via commit fb325fb5; this test
//! locks in that the fun variant is the DEFAULT (so those delight
//! moments keep happening) and that the silent variant is reachable
//! when `[agent] silent_compaction = true`.
//!
//! These tests are deliberately string-sentinel based, not full
//! equality. The exact wording will drift; what must not drift is:
//!
//! - Default (silent=false) keeps the explicit "POST-COMPACTION
//!   PROTOCOL" header and at least one invitation to acknowledge.
//! - Silent (silent=true) keeps the explicit "Silently" directive
//!   and excludes the fun acknowledgement invitation.
//! - The auto-approve tail is appended in both modes when
//!   auto_approve=false.

use crate::brain::agent::service::compaction_prompts::{CompactionKind, build_continuation};

const APPROVAL_TAIL: &str = "Tool approval is REQUIRED";
const SESSION_RECOVERY: &str = "SESSION RECOVERY";

#[test]
fn fun_regular_keeps_post_compaction_protocol_header() {
    let body = build_continuation(CompactionKind::Regular, false, true);
    assert!(
        body.contains("POST-COMPACTION PROTOCOL"),
        "fun regular must include the numbered protocol header so the \
         model picks up the task; got: {body}"
    );
    assert!(
        !body.contains("Silently continue"),
        "fun variant must not carry the silent directive: {body}"
    );
}

#[test]
fn silent_regular_uses_silent_directive() {
    let body = build_continuation(CompactionKind::Regular, true, true);
    assert!(
        body.contains("Silently continue"),
        "silent regular must explicitly tell the model to continue silently: {body}"
    );
    assert!(
        !body.contains("POST-COMPACTION PROTOCOL"),
        "silent variant must drop the verbose protocol header: {body}"
    );
}

#[test]
fn fun_emergency_invites_fun_cheeky_remark() {
    // The emergency path is where the most-shared one-liners came
    // from. The fun variant MUST carry an explicit invitation, not
    // just allow it implicitly — otherwise the model defaults to
    // silent recovery and the personality moment never fires.
    let body = build_continuation(CompactionKind::Emergency, false, true);
    assert!(
        body.contains("fun/cheeky remark"),
        "fun emergency must explicitly invite a fun/cheeky remark: {body}"
    );
}

#[test]
fn silent_emergency_suppresses_acknowledgement() {
    let body = build_continuation(CompactionKind::Emergency, true, true);
    assert!(
        body.contains("Silently resume"),
        "silent emergency must direct silent resumption: {body}"
    );
    assert!(
        !body.contains("fun/cheeky remark"),
        "silent variant must not invite a fun remark: {body}"
    );
}

#[test]
fn fun_post_tool_keeps_cursing_allowed_explicit() {
    // The post-tool prompt historically carried "cursing allowed" as
    // an explicit license. That's the line that produces the kind
    // of in-character output users have called out. If we ever
    // trim it we break the documented feature.
    let body = build_continuation(CompactionKind::PostTool, false, true);
    assert!(
        body.contains("cursing allowed"),
        "fun post-tool must keep the explicit cursing-allowed license: {body}"
    );
    assert!(body.contains("IMMEDIATE TASK"));
}

#[test]
fn silent_post_tool_drops_cursing_invitation() {
    let body = build_continuation(CompactionKind::PostTool, true, true);
    assert!(!body.contains("cursing allowed"));
    assert!(body.contains("Silently continue"));
}

#[test]
fn mid_loop_variants_diverge_on_silent_flag() {
    let fun = build_continuation(CompactionKind::MidLoop, false, true);
    let silent = build_continuation(CompactionKind::MidLoop, true, true);
    assert_ne!(fun, silent, "the two modes must produce different prompts");
    assert!(fun.contains("POST-COMPACTION PROTOCOL"));
    assert!(silent.contains("Silently continue"));
}

#[test]
fn approval_tail_appended_when_auto_approve_disabled_in_both_modes() {
    for kind in [
        CompactionKind::Regular,
        CompactionKind::MidLoop,
        CompactionKind::Emergency,
        CompactionKind::PostTool,
    ] {
        let fun = build_continuation(kind, false, false);
        let silent = build_continuation(kind, true, false);
        assert!(
            fun.contains(APPROVAL_TAIL),
            "fun {kind:?} must append the approval reminder when auto_approve=false: {fun}"
        );
        assert!(
            silent.contains(APPROVAL_TAIL),
            "silent {kind:?} must append the approval reminder when auto_approve=false: {silent}"
        );
    }
}

#[test]
fn approval_tail_omitted_when_auto_approve_enabled() {
    for kind in [
        CompactionKind::Regular,
        CompactionKind::MidLoop,
        CompactionKind::Emergency,
        CompactionKind::PostTool,
    ] {
        let fun = build_continuation(kind, false, true);
        let silent = build_continuation(kind, true, true);
        assert!(
            !fun.contains(APPROVAL_TAIL),
            "fun {kind:?} must NOT carry the approval tail when auto_approve=true: {fun}"
        );
        assert!(
            !silent.contains(APPROVAL_TAIL),
            "silent {kind:?} must NOT carry the approval tail when auto_approve=true: {silent}"
        );
    }
}

#[test]
fn default_agent_config_is_fun_mode() {
    // The `silent_compaction` flag is what selects between modes;
    // verify the config default keeps fun mode active so a fresh
    // install gets the personality moments out of the box.
    let cfg = crate::config::AgentConfig::default();
    assert!(
        !cfg.silent_compaction,
        "AgentConfig::default() must keep silent_compaction=false so fun \
         post-compaction narration is the out-of-the-box behaviour"
    );
}

#[test]
fn session_recovery_hint_present_in_all_variants() {
    // After compaction the agent must check for an active plan and,
    // if coding, load CODE.md. This hint is appended to ALL variants
    // (fun + silent, all 5 kinds) so it never gets lost.
    for kind in [
        CompactionKind::Regular,
        CompactionKind::MidLoop,
        CompactionKind::Emergency,
        CompactionKind::PostTool,
        CompactionKind::Manual,
    ] {
        for silent in [false, true] {
            let body = build_continuation(kind, silent, true);
            assert!(
                body.contains(SESSION_RECOVERY),
                "{kind:?} silent={silent} must include the SESSION RECOVERY hint: {body}"
            );
            assert!(
                body.contains("plan") && body.contains("start"),
                "{kind:?} silent={silent} must tell the agent to call plan with start: {body}"
            );
            assert!(
                body.contains("CODE.md"),
                "{kind:?} silent={silent} must mention CODE.md for coding sessions: {body}"
            );
        }
    }
}

#[test]
fn manual_compaction_is_brief() {
    // Manual /compact uses a short sentence, not the full POST-COMPACTION
    // PROTOCOL with numbered steps. The user explicitly triggered it.
    for silent in [false, true] {
        let body = build_continuation(CompactionKind::Manual, silent, true);
        assert!(
            !body.contains("POST-COMPACTION PROTOCOL"),
            "Manual compaction must NOT use the full protocol: {body}"
        );
        assert!(
            !body.contains("session_search"),
            "Manual compaction must NOT instruct session_search (brief only): {body}"
        );
        assert!(
            body.contains("IMMEDIATE TASK"),
            "Manual compaction must still reference the IMMEDIATE TASK: {body}"
        );
    }
}
