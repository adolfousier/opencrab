# Testing Guide

Comprehensive test coverage for OpenCrabs. All tests run with:

```bash
cargo test --all-features
```

## Quick Reference

| Category | Tests | Location |
|----------|------:|----------|
| Tests — Streaming Active Secs | 2 | `src/tests/streaming_active_secs_test.rs` |
| Tests — Tool Execution Stats | 2 | `src/tests/tool_execution_stats_test.rs` |
| Tests — Mission Control Report | 2 | `src/tests/mission_control_report_test.rs` |
| Tests — Custom Provider No Models | 2 | `src/tests/custom_provider_no_models_test.rs` |
| Tests — Provider Context Window Override | 2 | `src/tests/provider_context_window_override_test.rs` |
| Tests — Slack Handler | 2 | `src/tests/slack_handler_test.rs` |
| Tests — Custom Provider Section Resolver | 2 | `src/tests/custom_provider_section_resolver_test.rs` |
| Tests — Mission Control Command | 2 | `src/tests/mission_control_command_test.rs` |
| Tests — Rsi Self Improve Dedup | 2 | `src/tests/rsi_self_improve_dedup_test.rs` |
| Tests — Clipboard Image Paste | 2 | `src/tests/clipboard_image_paste_test.rs` |
| Tests — Cowork Connect | 2 | `src/tests/cowork_connect_test.rs` |
| Tests — Discord Handler | 2 | `src/tests/discord_handler_test.rs` |
| Tests — Cron Tool Registry | 2 | `src/tests/cron_tool_registry_test.rs` |
| Tests — User Correction Metadata | 3 | `src/tests/user_correction_metadata_test.rs` |
| Tests — Pdf Vision | 3 | `src/tests/pdf_vision_test.rs` |
| Tests — Xiaomi Config Default | 3 | `src/tests/xiaomi_config_default_test.rs` |
| Tests — Telegram Model Callback Data | 3 | `src/tests/telegram_model_callback_data_test.rs` |
| Tests — Telegram Caption | 3 | `src/tests/telegram_caption_test.rs` |
| Tests — New Session Pane Binding | 3 | `src/tests/new_session_pane_binding_test.rs` |
| Tests — Mimo Tool Call Hint | 3 | `src/tests/mimo_tool_call_hint_test.rs` |
| Tests — Brain Project Overlay | 3 | `src/tests/brain_project_overlay_test.rs` |
| Tests — Systemd Unit | 3 | `src/tests/systemd_unit_test.rs` |
| Tests — Profile Pid Lock | 3 | `src/tests/profile_pid_lock_test.rs` |
| Tests — Phantom Going To | 3 | `src/tests/phantom_going_to_test.rs` |
| Tests — Auto Title E2E | 3 | `src/tests/auto_title_e2e_test.rs` |
| Tests — Glob Tool | 3 | `src/tests/glob_tool_test.rs` |
| Tests — Project File Archive | 3 | `src/tests/project_file_archive_test.rs` |
| Tests — Brain Live Rebuild | 3 | `src/tests/brain_live_rebuild_test.rs` |
| Tests — Build User Message Image | 3 | `src/tests/build_user_message_image_test.rs` |
| Tests — Read Media Redirect | 3 | `src/tests/read_media_redirect_test.rs` |
| Tests — Provider Picker Setup Hint | 4 | `src/tests/provider_picker_setup_hint_test.rs` |
| Tests — Project File Slug | 4 | `src/tests/project_file_slug_test.rs` |
| Tests — Browser Cdp Endpoint | 4 | `src/tests/browser_cdp_endpoint_test.rs` |
| Tests — Tui Drop Path | 4 | `src/tests/tui_drop_path_test.rs` |
| Tests — Telegram Join Detection | 4 | `src/tests/telegram_join_detection_test.rs` |
| Tests — Pdf To Images | 4 | `src/tests/pdf_to_images_test.rs` |
| Tests — Plan Reminder | 4 | `src/tests/plan_reminder_test.rs` |
| Tests — Xiaomi Keyed Provider Regression | 4 | `src/tests/xiaomi_keyed_provider_regression_test.rs` |
| Tests — Rsi Fallback Wrap | 4 | `src/tests/rsi_fallback_wrap_test.rs` |
| Tests — Rtk Autodownload | 4 | `src/tests/rtk_autodownload_test.rs` |
| Tests — Profile Preempt | 4 | `src/tests/profile_preempt_test.rs` |
| Tests — Lazy Tools | 4 | `src/tests/lazy_tools_test.rs` |
| Tests — Logging Log Files | 4 | `src/tests/logging_log_files_test.rs` |
| Tests — Telegram Send Input File | 5 | `src/tests/telegram_send_input_file_test.rs` |
| Tests — Prompt Inline Edit Directive | 5 | `src/tests/prompt_inline_edit_directive_test.rs` |
| Tests — Doc Parser Page Range | 5 | `src/tests/doc_parser_page_range_test.rs` |
| Tests — Mission Control Dedup Detail | 5 | `src/tests/mission_control_dedup_detail_test.rs` |
| Tests — Follow Up Intermediate Flush | 5 | `src/tests/follow_up_intermediate_flush_test.rs` |
| Tests — Project Skills | 5 | `src/tests/project_skills_test.rs` |
| Tests — Command Rich Table | 5 | `src/tests/command_rich_table_test.rs` |
| Tests — Custom Provider Live Fetch Regression | 5 | `src/tests/custom_provider_live_fetch_regression_test.rs` |
| Tests — Rtk Tracker | 5 | `src/tests/rtk_tracker_test.rs` |
| Tests — Pdf Smart Routing | 5 | `src/tests/pdf_smart_routing_test.rs` |
| Tests — Xiaomi Onboarding | 5 | `src/tests/xiaomi_onboarding_test.rs` |
| Tests — Self Update Path | 6 | `src/tests/self_update_path_test.rs` |
| Tests — Telegram Send Thread Id Override | 6 | `src/tests/telegram_send_thread_id_override_test.rs` |
| Tests — Active Skill Tracking | 6 | `src/tests/active_skill_tracking_test.rs` |
| Tests — Config Dotted Caps | 6 | `src/tests/config_dotted_caps_test.rs` |
| Tests — Tts Fallback Chain | 6 | `src/tests/tts_fallback_chain_test.rs` |
| Tests — Rtk Sysadmin Supported | 6 | `src/tests/rtk_sysadmin_supported_test.rs` |
| Tests — Stt Fallback Chain | 6 | `src/tests/stt_fallback_chain_test.rs` |
| Tests — Telegram Topic Listing | 6 | `src/tests/telegram_topic_listing_test.rs` |
| Tests — Bash Blocklist | 6 | `src/tests/bash_blocklist_test.rs` |
| Tests — Whatsapp Handler | 6 | `src/tests/whatsapp_handler_test.rs` |
| Tests — Cron Profile Isolation | 6 | `src/tests/cron_profile_isolation_test.rs` |
| Tests — Evolve Diagnose | 7 | `src/tests/evolve_diagnose_test.rs` |
| Tests — Channel Session Resolve | 7 | `src/tests/channel_session_resolve_test.rs` |
| Tests — Phantom Cleanup Intent | 7 | `src/tests/phantom_cleanup_intent_test.rs` |
| Tests — Qwen Tool Marker Strip | 7 | `src/tests/qwen_tool_marker_strip_test.rs` |
| Tests — Telegram Impersonation | 7 | `src/tests/telegram_impersonation_test.rs` |
| Tests — Subagent Tool Description | 7 | `src/tests/subagent_tool_description_test.rs` |
| Tests — Fallback Streak | 7 | `src/tests/fallback_streak_test.rs` |
| Tests — Phantom Work Announcement | 7 | `src/tests/phantom_work_announcement_test.rs` |
| Tests — Word Delete Keybinding | 7 | `src/tests/word_delete_keybinding_test.rs` |
| Tests — Telegram Thread Id Lookup | 8 | `src/tests/telegram_thread_id_lookup_test.rs` |
| Tests — Telegram Reply Context Recovery | 8 | `src/tests/telegram_reply_context_recovery_test.rs` |
| Tests — Onboarding User Scroll | 8 | `src/tests/onboarding_user_scroll_test.rs` |
| Tests — Prompt Known Paths | 8 | `src/tests/prompt_known_paths_test.rs` |
| Tests — Cli Supported Models | 8 | `src/tests/cli_supported_models_test.rs` |
| Tests — Custom Provider Rename Keys Toml | 8 | `src/tests/custom_provider_rename_keys_toml_test.rs` |
| Tests — Phantom Pronoun Drop | 8 | `src/tests/phantom_pronoun_drop_test.rs` |
| Tests — Provider Registry | 8 | `src/tests/provider_registry_test.rs` |
| Tests — Telegram Photo Batching | 8 | `src/tests/telegram_photo_batching_test.rs` |
| Tests — Mission Control Skill Inbox | 8 | `src/tests/mission_control_skill_inbox_test.rs` |
| Tests — Plan Tool Description | 8 | `src/tests/plan_tool_description_test.rs` |
| Tests — Onboarding No Silent Commit | 8 | `src/tests/onboarding_no_silent_commit_test.rs` |
| Tests — Telegram Handler | 9 | `src/tests/telegram_handler_test.rs` |
| Tests — Orphan Close Tag Strip | 9 | `src/tests/orphan_close_tag_strip_test.rs` |
| Tests — Onboarding Custom Model Input | 9 | `src/tests/onboarding_custom_model_input_test.rs` |
| Tests — Telegram Rich | 9 | `src/tests/telegram_rich_test.rs` |
| Tests — Rsi Sync Cap Bail | 9 | `src/tests/rsi_sync_cap_bail_test.rs` |
| Tests — Integration | 9 | `src/tests/integration_test.rs` |
| Tests — Format User Error | 9 | `src/tests/format_user_error_test.rs` |
| Tests — Telegram Plan Render | 9 | `src/tests/telegram_plan_render_test.rs` |
| Tests — Plan Mode Integration | 9 | `src/tests/plan_mode_integration_test.rs` |
| Tests — Error Scenarios | 9 | `src/tests/error_scenarios_test.rs` |
| Tests — Web Browser Routing | 9 | `src/tests/web_browser_routing_test.rs` |
| Tests — Rsi Skill Proposals | 9 | `src/tests/rsi_skill_proposals_test.rs` |
| Tests — Baseline Merge | 10 | `src/tests/baseline_merge_test.rs` |
| Tests — Sanitize Code Edit Block | 10 | `src/tests/sanitize_code_edit_block_test.rs` |
| Tests — Markdown Render | 10 | `src/tests/markdown_render_test.rs` |
| Tests — Tool Arg Unescape | 10 | `src/tests/tool_arg_unescape_test.rs` |
| Tests — Custom Provider Cache Autoenable | 10 | `src/tests/custom_provider_cache_autoenable_test.rs` |
| Tests — Streaming | 10 | `src/tests/streaming_test.rs` |
| Tests — Incident Log Dedup | 10 | `src/tests/incident_log_dedup_test.rs` |
| Tests — Phantom Deferment | 11 | `src/tests/phantom_deferment_test.rs` |
| Tests — Analysis Intent Nudge | 11 | `src/tests/analysis_intent_nudge_test.rs` |
| Tests — Phantom Post Success Exemption | 11 | `src/tests/phantom_post_success_exemption_test.rs` |
| Tests — Goal Command | 11 | `src/tests/goal_command_test.rs` |
| Tests — Whatsapp Photo Batching | 11 | `src/tests/whatsapp_photo_batching_test.rs` |
| Tests — Prompt Compiled Features | 11 | `src/tests/prompt_compiled_features_test.rs` |
| Tests — Telegram Pre Tool Rolling | 11 | `src/tests/telegram_pre_tool_rolling_test.rs` |
| Tests — Service Scope | 11 | `src/tests/service_scope_test.rs` |
| Tests — Telegram Quote Reply | 11 | `src/tests/telegram_quote_reply_test.rs` |
| Tests — Dynamic Tool Parse Error | 12 | `src/tests/dynamic_tool_parse_error_test.rs` |
| Tests — Onboard Channel | 12 | `src/tests/onboard_channel_test.rs` |
| Tests — Streaming Tps Accumulator | 12 | `src/tests/streaming_tps_accumulator_test.rs` |
| Tests — Compaction Prompts | 12 | `src/tests/compaction_prompts_test.rs` |
| Tests — Cron Schedule Util | 12 | `src/tests/cron_schedule_util_test.rs` |
| Tests — Orphan Think Close Tag | 13 | `src/tests/orphan_think_close_tag_test.rs` |
| Tests — Usage Cosmetic Alias | 13 | `src/tests/usage_cosmetic_alias_test.rs` |
| Tests — Voice Service | 14 | `src/tests/voice_service_test.rs` |
| Tests — Evolve Systemd Restart | 14 | `src/tests/evolve_systemd_restart_test.rs` |
| Tests — Background Session | 14 | `src/tests/background_session_test.rs` |
| Tests — Bash Feedback Enrichment | 14 | `src/tests/bash_feedback_enrichment_test.rs` |
| Tests — Voice Voicebox | 15 | `src/tests/voice_voicebox_test.rs` |
| Tests — Brain Filter Strip Empty Sections | 15 | `src/tests/brain_filter_strip_empty_sections_test.rs` |
| Tests — Voice Local Tts | 15 | `src/tests/voice_local_tts_test.rs` |
| Tests — Template Governance | 15 | `src/tests/template_governance_test.rs` |
| Tests — Telegram Status Message | 15 | `src/tests/telegram_status_message_test.rs` |
| Tests — Rtk Rewrite | 15 | `src/tests/rtk_rewrite_test.rs` |
| Tests — Channel Commands | 16 | `src/tests/channel_commands_test.rs` |
| Tests — Telegram Session Resolve | 18 | `src/tests/telegram_session_resolve_test.rs` |
| Tests — Qwen Detect | 18 | `src/tests/qwen_detect_test.rs` |
| Tests — Bundled Plans | 20 | `src/tests/bundled_plans_test.rs` |
| Tests — Text Complete | 21 | `src/tests/text_complete_test.rs` |
| Tests — Plan Window | 21 | `src/tests/plan_window_test.rs` |
| Tests — Rsi Pruned | 23 | `src/tests/rsi_pruned_test.rs` |
| Tests — Rsi Subsystem | 23 | `src/tests/rsi_subsystem_test.rs` |
| Tests — Pdf Page Range Parser | 25 | `src/tests/pdf_page_range_parser_test.rs` |
| Tests — Project | 25 | `src/tests/project_test.rs` |
| Tests — Voice Local Whisper | 25 | `src/tests/voice_local_whisper_test.rs` |
| Tests — Rsi Brain Dedup | 27 | `src/tests/rsi_brain_dedup_test.rs` |
| Tests — Telegram Rich Parse | 28 | `src/tests/telegram_rich_parse_test.rs` |
| Tests — Plan Tool | 31 | `src/tests/plan_tool_test.rs` |
| Tests — Profiles Dialog | 49 | `src/tests/profiles_dialog_test.rs` |
| **Brain — Agent Service** | 26 | `src/brain/agent/service/` |
| **Brain — Prompt Builder** | 20 | `src/brain/prompt_builder.rs` |
| **Brain — Agent Context** | 12 | `src/brain/agent/context.rs` |
| **Brain — Provider (Anthropic)** | 9 | `src/brain/provider/anthropic.rs` |
| **Provider Retry (consolidated)** | 19 | `src/utils/retry.rs` (8 inline) + `src/tests/provider_retry_consolidation_test.rs` (11) — `brain/provider/retry.rs` was deleted and folded onto `utils::retry`; covers patient backoff schedule, hard-down fast-fail, Retry-After clamp, and retry-notify surfacing |
| **Brain — Provider (Custom OpenAI)** | 9 | `src/brain/provider/custom_openai_compatible.rs` |
| **Brain — Provider (Copilot)** | 8 | `src/brain/provider/copilot.rs` |
| **Brain — Provider (Factory)** | 4 | `src/brain/provider/factory.rs` |
| **Brain — Provider (Claude CLI)** | 4 | `src/brain/provider/claude_cli.rs` |
| **Brain — Provider (Types/Error/Trait)** | 7 | `src/brain/provider/` |
| **Brain — Provider (Qwen)** | 13 | `src/brain/provider/qwen.rs` |
| **Brain — Provider (JSON Repair)** | 10 | `src/brain/provider/json_repair.rs` |
| **Brain — Provider (Codex OAuth)** | 6 | `src/brain/provider/codex_oauth.rs` |
| **Brain — Tokenizer** | 8 | `src/brain/tokenizer.rs` |
| **Brain — Commands** | 6 | `src/brain/commands.rs` |
| **Brain — Self-Update** | 1 | `src/brain/self_update.rs` |
| **Brain Tools — Bash** | 21 | `src/brain/tools/bash.rs` |
| **Brain Tools — Plan Security** | 20 | `src/brain/tools/plan_tool.rs` |
| **Brain Tools — Exa Search** | 18 | `src/brain/tools/exa_search.rs` |
| **Brain Tools — Write File** | 17 | `src/brain/tools/write_opencrabs_file.rs` |
| **Brain Tools — A2A Send** | 16 | `src/brain/tools/a2a_send.rs` |
| **Brain Tools — Load Brain File** | 15 | `src/brain/tools/load_brain_file.rs` |
| **Brain Tools — Brave Search** | 12 | `src/brain/tools/brave_search.rs` |
| **Brain Tools — Browser Manager** | 12 | `src/brain/tools/browser/manager.rs` |
| **Brain Tools — Tool Manage** | 11 | `src/brain/tools/tool_manage.rs` |
| **Brain Tools — Dynamic Tools** | 17 | `src/brain/tools/dynamic/` |
| **Brain Tools — Doc Parser** | 10 | `src/brain/tools/doc_parser.rs` |
| **Brain Tools — Registry** | 7 | `src/brain/tools/registry.rs` |
| **Brain Tools — Slash Command** | 6 | `src/brain/tools/slash_command.rs` |
| **Brain Tools — Write/Read/Config/Memory/Error** | 20 | `src/brain/tools/` |
| **Brain Tools — Subagent** | 9 | `src/brain/tools/subagent.rs` |
| **Brain Tools — Error** | 6 | `src/brain/tools/error.rs` |
| **Brain Tools — Config Tool** | 5 | `src/brain/tools/config_tool.rs` |
| **Brain Tools — Write** | 5 | `src/brain/tools/write.rs` |
| **Brain Tools — Read** | 4 | `src/brain/tools/read.rs` |
| **Brain Tools — Memory Search** | 2 | `src/brain/tools/memory_search.rs` |
| **Brain Tools — Trait** | 3 | `src/brain/tools/trait.rs` |
| **Channels — Voice Local Whisper** | 25 | `src/channels/voice/local_whisper.rs` |
| **Channels — Voice Service** | 14 | `src/channels/voice/service.rs` |
| **Channels — Voice Local TTS** | 14 | `src/channels/voice/local_tts.rs` |
| **Channels — Commands** | 15 | `src/channels/commands.rs` |
| **Channels — WhatsApp Store** | 15 | `src/channels/whatsapp/store.rs` |
| **Channels — Telegram Handler** | 8 | `src/channels/telegram/handler.rs` |
| **Channels — WhatsApp Handler** | 5 | `src/channels/whatsapp/handler.rs` |
| **Channels — General** | 5 | `src/channels/` |
| **Channels — Slack Handler** | 2 | `src/channels/slack/handler.rs` |
| **Channels — Discord Handler** | 2 | `src/channels/discord/handler.rs` |
| **Config — Types** | 19 | `src/config/types.rs` |
| **Config — Secrets** | 5 | `src/config/secrets.rs` |
| **Config — Update** | 4 | `src/config/update.rs` |
| **Config — Crabrace** | 3 | `src/config/crabrace.rs` |
| **DB — Repository (Plan)** | 15 | `src/db/repository/plan.rs` |
| **DB — Retry** | 8 | `src/db/retry.rs` |
| **DB — Repository (Other)** | 9 | `src/db/repository/` |
| **DB — Database** | 5 | `src/db/database.rs` |
| **DB — Models** | 4 | `src/db/models.rs` |
| **Services — Plan** | 11 | `src/services/plan.rs` |
| **Services — File** | 11 | `src/services/file.rs` |
| **Services — Message** | 10 | `src/services/message.rs` |
| **Services — Session** | 10 | `src/services/session.rs` |
| **Services — Context** | 2 | `src/services/context.rs` |
| **TUI — Onboarding** | 67 | `src/tui/onboarding/` |
| **TUI — Plan** | 25 | `src/tui/plan.rs` |
| **TUI — Render Utils** | 12 | `src/tui/render/utils.rs` |
| **TUI — Prompt Analyzer** | 8 | `src/tui/prompt_analyzer.rs` |
| **TUI — Highlight** | 8 | `src/tui/highlight.rs` |
| **TUI — Markdown** | 7 | `src/tui/markdown.rs` |
| **TUI — Error** | 5 | `src/tui/error.rs` |
| **TUI — Events** | 4 | `src/tui/events.rs` |
| **TUI — Components** | 2 | `src/tui/components/` |
| **TUI — App State** | 1 | `src/tui/app/state.rs` |
| **A2A — Debate** | 8 | `src/a2a/debate.rs` |
| **A2A — Types** | 6 | `src/a2a/types.rs` |
| **A2A — Server/Handler/Agent Card** | 7 | `src/a2a/` |
| **Memory — Store** | 6 | `src/memory/store.rs` |
| **Memory — Search** | 3 | `src/memory/search.rs` |
| **Pricing** | 17 | `src/pricing.rs` |
| **Utils — Sanitize** | 41 | `src/utils/sanitize.rs` + `src/tests/sanitize_redaction_test.rs` |
| **Utils — Retry** | 8 | `src/utils/retry.rs` |
| **Utils — String** | 6 | `src/utils/string.rs` |
| **Utils — Install** | 6 | `src/utils/install.rs` |
| **Utils — Config Watcher** | 2 | `src/utils/config_watcher.rs` |
| **Logging** | 4 | `src/logging/logger.rs` |
| Tests — RSI Template Sync | 15 | `src/tests/rsi_sync_test.rs` |
| Tests — Model Fetching | 11 | `src/tests/model_fetch_test.rs` |
| Tests — Provider Factory Regression | 31 | `src/tests/provider_factory_regression_test.rs` |
| Tests — Onboarding Welcome | 9 | `src/tests/onboarding_welcome_test.rs` |
| Tests — Voice STT Dispatch | 21 | `src/tests/voice_stt_dispatch_test.rs` |
| Tests — Voice Onboarding | 65 | `src/tests/voice_onboarding_test.rs` |
| Tests — Voice OpenAI Compatible | 12 | `src/tests/voice_openai_compatible_test.rs` |
| Tests — Cron Jobs & Scheduling | 58 | `src/tests/cron_test.rs` |
| Tests — Onboarding Field Nav | 49 | `src/tests/onboarding_field_nav_test.rs` |
| Tests — GitHub Copilot Provider | 38 | `src/tests/github_provider_test.rs` |
| Tests — File Extract | 37 | `src/tests/file_extract_test.rs` |
| Tests — Fallback Vision | 40 | `src/tests/fallback_vision_test.rs` |
| Tests — CLI Parsing | 28 | `src/tests/cli_test.rs` |
| Tests — Custom Provider | 27 | `src/tests/custom_provider_test.rs` |
| Tests — Onboarding Navigation | 26 | `src/tests/onboarding_navigation_test.rs` |
| Tests — Message Compaction | 28 | `src/tests/compaction_test.rs` |
| Tests — Channel Search | 27 | `src/tests/channel_search_test.rs` |
| Tests — Evolve (Self-Update) | 23 | `src/tests/evolve_test.rs` |
| Tests — Slack Formatting | 21 | `src/tests/slack_fmt_test.rs` |
| Tests — Split Pane | 21 | `src/tests/split_pane_test.rs` |
| Tests — OpenCode CLI Provider | 21 | `src/tests/opencode_provider_test.rs` |
| Tests — Onboarding Brain | 23 | `src/tests/onboarding_brain_test.rs` |
| Tests — Onboarding Types | 17 | `src/tests/onboarding_types_test.rs` |
| Tests — OpenAI Provider | 16 | `src/tests/openai_provider_test.rs` |
| Tests — TUI Error | 16 | `src/tests/tui_error_test.rs` |
| Tests — Queued Messages | 15 | `src/tests/queued_message_test.rs` |
| Tests — Plan Document | 15 | `src/tests/plan_document_test.rs` |
| Tests — Session & Working Dir | 15 | `src/tests/session_working_dir_test.rs` |
| Tests — Stream Loop Detection | 19 | `src/tests/stream_loop_test.rs` |
| Tests — Context Window | 14 | `src/tests/context_window_test.rs` |
| Tests — HTML Comment Strip | 14 | `src/tests/html_comment_strip_test.rs` |
| Tests — Daemon Health & Config | 10 | `src/tests/daemon_health_test.rs` |
| Tests — Collapse Build Output | 9 | `src/tests/collapse_build_output_test.rs` |
| Tests — Image Utils | 9 | `src/tests/image_util_test.rs` |
| Tests — Brain Templates | 7 | `src/tests/brain_templates_test.rs` |
| Tests — AltGr Input | 8 | `src/tests/altgr_input_test.rs` |
| Tests — QR Render | 10 | `src/tests/qr_render_test.rs` |
| Tests — Provider Sync | 8 | `src/tests/provider_sync_test.rs` |
| Tests — WhatsApp State | 7 | `src/tests/whatsapp_state_test.rs` |
| Tests — Reasoning Lines | 7 | `src/tests/reasoning_lines_test.rs` |
| Tests — System Continuation | 6 | `src/tests/system_continuation_test.rs` |
| Tests — Candle Whisper | 6 | `src/tests/candle_whisper_test.rs` |
| Tests — Post-Evolve | 5 | `src/tests/post_evolve_test.rs` |
| Tests — Onboarding Keys | 4 | `src/tests/onboarding_keys_test.rs` |
| Tests — TUI Render Clear | 4 | `src/tests/tui_render_clear_test.rs` |
| Tests — TUI Tool Stack | 6 | `src/tests/tui_tool_stack_test.rs` |
| Tests — Gemini Fetch | 3 | `src/tests/gemini_fetch_test.rs` |
| Tests — Profiles | 61 | `src/tests/profile_test.rs` |
| Tests — Subagent / Swarm | 84 | `src/tests/subagent_test.rs` |
| Tests — Telegram Resume & Helpers | 57 | `src/tests/telegram_resume_test.rs` |
| Tests — Token Tracking | 29 | `src/tests/token_tracking_test.rs` |
| Tests — wait_agent Resolver | 12 | `src/tests/wait_agent_resolver_test.rs` |
| Tests — Browser Default (macOS LSHandlers parser) | 12 | `src/tests/browser_default_test.rs` |
| Tests — Browser Default (Linux xdg-settings parser) | 4 | `src/tests/browser_default_linux_test.rs` (Linux-only) |
| Tests — Browser Default (Windows reg-query parser) | 6 | `src/tests/browser_default_windows_test.rs` (Windows-only) |
| Tests — Browser Profile Lock Sweeper | 5 | `src/tests/browser_locks_test.rs` |
| Tests — Browser CDP Handler Health | 4 | `src/tests/browser_health_test.rs` |
| Tests — Browser Stealth JS Regression Guards | 6 | `src/tests/browser_stealth_test.rs` |
| Tests — Browser Manager Drop | 2 | `src/tests/browser_drop_test.rs` |
| Tests — Browser Session-Scoped Tabs | 4 | `src/tests/browser_session_test.rs` |
| Tests — Browser Profile Unlock Backoff | 4 | `src/tests/browser_profile_wait_test.rs` |
| Tests — Browser Eval Output Cap | 5 | `src/tests/browser_eval_cap_test.rs` |
| Tests — Browser Screenshot-Failure Surface | 2 | `src/tests/browser_screenshot_surface_test.rs` |
| Tests — Browser Find JS Builder | 9 | `src/tests/browser_find_test.rs` |
| Tests — exa_search MCP Handshake | 4 | `src/tests/exa_search_test.rs` |
| Tests — http_request User-Agent | 3 | `src/tests/http_request_test.rs` |
| Tests — Self-Healing (Phantom Detection + stuck-intent loop) | 88 | `src/tests/self_healing_test.rs` |
| Tests — Bash SSH Detection | 10 | `src/tests/bash_ssh_detection_test.rs` |
| Tests — Bash POSIX Quote (askpass) | 9 | `src/tests/bash_posix_quote_test.rs` |
| Tests — RSI Proposals Inbox | 16 | `src/tests/rsi_proposals_test.rs` |
| Tests — Skills Loader | 15 | `src/tests/skills_test.rs` |
| Tests — Skill Slash Dispatch | 7 | `src/tests/skill_slash_dispatch_test.rs` |
| Tests — Slash Autocomplete Dimensions | 18 | `src/tests/slash_autocomplete_dimensions_test.rs` |
| Tests — Mission Control Layout | 7 | `src/tests/mission_control_layout_test.rs` |
| Tests — Mission Control Inbox Service | 6 | `src/tests/mission_control_inbox_service_test.rs` |
| Tests — Mission Control Activity Service | 8 | `src/tests/mission_control_activity_service_test.rs` |
| Tests — Mission Control Schedule Service | 5 | `src/tests/mission_control_schedule_service_test.rs` |
| Tests — Mission Control Input | 23 | `src/tests/mission_control_input_test.rs` |
| Tests — Skills Dialog | 18 | `src/tests/skills_dialog_test.rs` |
| Tests — merge_provider_keys (OpenCode persistence regression) | 4 | `src/tests/merge_provider_keys_test.rs` |
| Tests — Onboarding Wizard | 67 | `src/tests/onboarding_wizard_test.rs` |
| Tests — RSI (Recursive Self-Improvement) | 83 | `src/tests/rsi_test.rs` |
| Tests — Dynamic Tool Coercion | 13 | `src/tests/dynamic_tool_coerce_test.rs` |
| Tests — Follow-Up Question Tool | 9 | `src/tests/follow_up_question_test.rs` |
| Tests — Gemini Schema Sanitization | 10 | `src/tests/gemini_schema_sanitize_test.rs` |
| Tests — Rename Session Tool | 7 | `src/tests/rename_session_test.rs` |
| Tests — Custom Model Paste | 5 | `src/tests/custom_model_paste_test.rs` |
| Tests — Brain File Generic Guard | 4 | `src/tests/brain_file_generic_guard_test.rs` |
| Tests — Phantom DB Persistence | 2 | `src/tests/phantom_db_persistence_test.rs` |
| Tests — Bash Interactive Reject | 37 | `src/tests/bash_interactive_reject_test.rs` |
| Tests — Qwen Tool-Call Extractor | 64 | `src/tests/qwen_tool_extractor_test.rs` |
| Tests — Brain File Safety (append-only enforcement, cleanup_intent) | 37 | `src/tests/brain_file_safety_test.rs` |
| Tests — Provider Config Regression | 28 | `src/tests/provider_config_regression_test.rs` |
| Tests — Tool-Loop Helpers (Linor P0 hotspot) | 30 | `src/tests/tool_loop_helpers_test.rs` |
| Tests — Recent Paths | 17 | `src/tests/recent_paths_test.rs` |
| Tests — Provider Error Proxy | 21 | `src/tests/provider_error_proxy_test.rs` |
| Tests — Mouse Fragment Filter | 13 | `src/tests/mouse_fragment_filter_test.rs` |
| Tests — Agent Basic | 12 | `src/tests/agent_basic_test.rs` |
| Tests — RSI Git History | 12 | `src/tests/rsi_git_history_test.rs` |
| Tests — Bash Retry Loop | 10 | `src/tests/bash_retry_loop_test.rs` |
| Tests — Agent Tool Normalization | 10 | `src/tests/agent_tool_normalization_test.rs` |
| Tests — Kimi Reasoning Markers | 9 | `src/tests/kimi_reasoning_test.rs` |
| Tests — Usage Activity Columns | 9 | `src/tests/usage_activity_columns_test.rs` |
| Tests — Agent Context Tracking | 8 | `src/tests/agent_context_tracking_test.rs` |
| Tests — Collapse `$HOME` to `~` | 8 | `src/tests/collapse_home_test.rs` |
| Tests — Rate Limiter | 8 | `src/tests/rate_limiter_test.rs` |
| Tests — Session Chat-ID Lookup | 8 | `src/tests/session_chat_id_lookup_test.rs` |
| Tests — Agent Approval Policies | 7 | `src/tests/agent_approval_policies_test.rs` |
| Tests — Agent Model Selection | 6 | `src/tests/agent_model_selection_test.rs` |
| Tests — Browser Close Tool | 6 | `src/tests/browser_close_test.rs` |
| Tests — Local-Provider Gate | 6 | `src/tests/local_provider_gate_test.rs` |
| Tests — Agent Parallel Sessions | 5 | `src/tests/agent_parallel_sessions_test.rs` |
| Tests — Agent Streaming Usage | 5 | `src/tests/agent_streaming_usage_test.rs` |
| Tests — Claude CLI Model Selection | 7 | `src/tests/claude_cli_model_test.rs` |
| Tests — Codex CLI Provider | 5 | `src/tests/codex_cli_test.rs` |
| Tests — Non-stream Compatibility | 5 | `src/tests/nonstream_compat_test.rs` |
| Tests — Runtime Info `$HOME` Anchor | 6 | `src/tests/runtime_info_home_anchor_test.rs` |
| Tests — Handshake Timeout | 4 | `src/tests/handshake_timeout_test.rs` |
| Tests — Usage Ledger | 4 | `src/tests/usage_ledger_test.rs` |
| Tests — Browser E2E (opt-in, `#[ignore]`) | 4 | `src/tests/browser_e2e_test.rs` |
| Tests — CLI Arg Length Cap | 2 | `src/tests/cli_arg_too_long_test.rs` |
| Tests — Config Watcher (integration) | 3 | `src/tests/config_watcher_test.rs` |
| Tests — Usage Model Grouping (`.gguf` + provider-prefix normalization) | 18 | `src/tests/usage_grouping_test.rs` |
| Tests — Tool-Execution Repo (empty `tool_name` guard) | 4 | `src/tests/tool_execution_repo_test.rs` |
| Tests — Generate Image Backend Dispatch (Gemini vs OpenAI-compatible) | 5 | `src/tests/generate_image_backend_test.rs` |
| **Usage — Categorizer** | 4 | `src/usage/categorizer.rs` |
| **Usage — Dashboard** | 6 | `src/usage/dashboard.rs` |
| **Usage — Data** | 7 | `src/usage/data.rs` |
| **Hashline Edit** | 53 | `src/tests/hashline_test.rs` (30 integration tests) + inline unit tests in `hash.rs` (9) and `types.rs` (14) — hash computation, HashRef parsing, edit operations (replace/append/prepend), hash mismatch detection, overlap detection, batch edits, prefix stripping, read_file hashline mode |
| Tests — Auto-Title (channel prefix preservation) | 28 | `src/tests/auto_title_test.rs` |
| Tests — Self-Improve Failure Log Guard | 3 | `src/tests/self_improve_failure_log_guard_test.rs` |
| Tests — Provider Retry Consolidation | 9 | `src/tests/provider_retry_consolidation_test.rs` — patient backoff, hard-down/DNS fast-fail, Retry-After clamp, retry-notify surfacing |
| Tests — Tool-Name Self-Heal (#176) | 11 | `src/tests/tool_name_heal_test.rs` — maps a model's near-miss tool name (`tg_send_message` → `telegram_send`) to the registered tool |
| Tests — Secret Redaction (tool summary) | 6 | `src/tests/tool_description_redaction_test.rs` — redacts Bearer/api_key/URL-password from the one-line tool display |
| Tests — Secret Redaction (RSI notifications) | 5 | `src/tests/rsi_notification_redaction_test.rs` — redacts secrets from RSI TUI alerts |
| Tests — Provider Error / HTML-page retry | (in proxy test) | `src/tests/provider_error_proxy_test.rs` — 4xx HTML infra error pages classified retryable (modelscope 405) |
| Tests — Cross-Provider Model Leak Guard | 6 | `src/tests/cross_provider_model_leak_guard_test.rs` |
| Tests — Session Provider Wrap / model pairing | — | `src/tests/session_provider_wrap_test.rs` — swap wraps in FallbackProvider; provider+model set atomically, never invents a default |
| Tests — Streaming tok/s Guard | 10 | `src/tests/streaming_tok_per_sec_guard_test.rs` — floors/ceilings burst-delivery tok/s artifacts |
| Tests — Telegram Last-Intermediate Footer | 7 | `src/tests/telegram_last_intermediate_footer_test.rs` — ctx/tok-s footer appended to the last completion message |
| Tests — analyze_video Frame-Extraction Fallback | 6 | `src/tests/analyze_video_fallback_test.rs` |
| Tests — Git Branch Footer | 8 | `src/tests/git_branch_test.rs` |
| Tests — TOOLS.md Slim Regression | 9 | `src/tests/tools_md_regression_test.rs` |
| Tests — Telegram Command Sanitize | 12 | `src/tests/telegram_command_sanitize_test.rs` |
| Tests — Usage Cache | 15 | `src/tests/usage_cache_test.rs` |
| Tests — Config Auto-Repair | 7 | `src/tests/config_repair_test.rs` — closes unterminated arrays/inline tables in a broken `config.toml`, gated on the result re-parsing; leaves valid/nested/string cases and unfixable errors alone |
| Tests — Config Last-Good Recovery | 3 | `src/tests/config_last_good_recovery_test.rs` — a broken config never poisons the last-good snapshot; fixable configs auto-repair in place; unfixable ones recover from last-good (preserves auto-always so yolo mode survives a typo) |
| **Total** | **4,248** | Authoritative count from `cargo test --all-features` (lib test binary): 4,248 run by default + 24 `#[ignore]`d. The per-category rows above are a maintained snapshot. Re-run `cargo test` for the live number. |

---

## Feature-Gated Tests

Some tests only compile/run with specific feature flags:

| Feature | Tests |
|---------|-------|
| `local-stt` | Local whisper inline tests, candle whisper tests, STT dispatch local-mode tests, codec tests, availability cycling tests |
| `local-tts` | TTS voice cycling, Piper voice Up/Down |

All feature-gated tests use `#[cfg(feature = "...")]` and are automatically included when running with `--all-features`.

---

## Running Tests

```bash
# Run all tests (recommended)
cargo test --all-features

# Run a specific test module
cargo test --all-features -- voice_onboarding_test

# Run a single test
cargo test --all-features -- is_newer_major_bump

# Run with output (for debugging)
cargo test --all-features -- --nocapture

# Run only local-stt tests
cargo test --features local-stt -- local_whisper
```

---

## Profile Tests

Profile tests live in `src/tests/profile_test.rs` and cover multi-instance isolation:

| Area | What's tested |
|------|--------------|
| Name validation | Reserved names, length bounds, special characters |
| Token hashing | Determinism, uniqueness, fixed length, hex output |
| Registry (in-memory) | CRUD, serde roundtrip, touch timestamps |
| Path resolution | Base dir, env var override, default vs named profiles |
| Filesystem CRUD | Create/delete lifecycle, duplicate detection, registry sync |
| Export/Import | Roundtrip with config files, nested memory directories |
| Migration | Copy `.md`/`.toml` files, skip/force behavior, default source |
| Token locks | Acquire/release, stale PID cleanup, cross-profile conflict |
| Profile isolation | Separate directories, concurrent writes, default vs named |
| Concurrent writes | Tokio tasks creating 5 profiles simultaneously |

```bash
# Run profile tests only
cargo test --all-features -p opencrabs -- profile_test
```

**Note:** All filesystem-touching tests acquire a global `fs_lock()` mutex to prevent concurrent write corruption of `~/.opencrabs/profiles.toml`. The mutex uses `unwrap_or_else(|p| p.into_inner())` to recover from poison (a prior test panic won't cascade-fail every subsequent test). In-memory tests run in parallel without the lock. The `test_set_and_get_active_profile` test accounts for `OnceLock` semantics (can only be set once per process).

---

## Disabled Test Modules

These modules exist but are commented out in `src/tests/mod.rs` (require network or external services):

| Module | Reason |
|--------|--------|
| `error_scenarios_test` | Requires mock API server |
| `integration_test` | End-to-end with LLM provider |
| `plan_mode_integration_test` | End-to-end plan workflow |
| `streaming_test` | Requires streaming API endpoint |

---

## Phantom Detection Tests

The self-healing phantom detector prevents the agent from dropping requests mid-stream when it says it will investigate something but never calls tools.

### Coverage

Tests in `src/tests/self_healing_test.rs` verify detection of investigative intent phrases:

| Phrase Pattern | Examples |
|---------------|----------|
| `let me hunt/trace/track` | "let me hunt down the bug", "let me trace the request" |
| `let me look into/check into` | "let me look into that", "let me check into the logs" |
| `let me find out/dig into` | "let me find out why", "let me dig into the code" |
| `i'll hunt/trace/track` | "i'll hunt that down", "i'll trace the flow" |
| `i'll look into/check into` | "i'll look into it", "i'll check into the error" |
| `i'll find out/dig into` | "i'll find out what's wrong", "i'll dig into the issue" |

### Behavior

When the agent outputs one of these phrases with zero tool calls, the phantom detector:
1. Catches the mismatch between intent and action
2. Injects a correction forcing tool invocation
3. Prevents the response from ending with unexecuted promises

### Test Count

88 tests covering phrase detection, edge cases, and integration with the tool loop.