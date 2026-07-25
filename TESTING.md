# Testing Guide

Comprehensive test coverage for OpenCrabs. All tests run with:

```bash
cargo test --all-features
```

## Quick Reference

| Category | Tests | Location |
|----------|------:|----------|
| Tests — A2A Agent Card | 2 | `src/tests/a2a_agent_card_test.rs` |
| Tests — A2A Debate | 8 | `src/tests/a2a_debate_test.rs` |
| Tests — A2A Handler Tasks | 1 | `src/tests/a2a_handler_tasks_test.rs` |
| Tests — A2A Handler | 2 | `src/tests/a2a_handler_test.rs` |
| Tests — A2A Server | 2 | `src/tests/a2a_server_test.rs` |
| Tests — A2A Types | 6 | `src/tests/a2a_types_test.rs` |
| Tests — Active Skill Tracking | 6 | `src/tests/active_skill_tracking_test.rs` |
| Tests — Agent Approval Policies | 7 | `src/tests/agent_approval_policies_test.rs` |
| Tests — Agent Basic | 12 | `src/tests/agent_basic_test.rs` |
| Tests — Agent Context Tracking | 8 | `src/tests/agent_context_tracking_test.rs` |
| Tests — Agent Helpers React Directive | 4 | `src/tests/agent_helpers_react_directive_test.rs` |
| Tests — Agent Model Selection | 6 | `src/tests/agent_model_selection_test.rs` |
| Tests — Agent Parallel Sessions | 5 | `src/tests/agent_parallel_sessions_test.rs` |
| Tests — Agent Streaming Usage | 5 | `src/tests/agent_streaming_usage_test.rs` |
| Tests — Agent Tool Normalization | 10 | `src/tests/agent_tool_normalization_test.rs` |
| Tests — Altgr Input | 8 | `src/tests/altgr_input_test.rs` |
| Tests — Analysis Intent Nudge | 11 | `src/tests/analysis_intent_nudge_test.rs` |
| Tests — Analyze Video Fallback | 9 | `src/tests/analyze_video_fallback_test.rs` |
| Tests — Auto Title E2E | 3 | `src/tests/auto_title_e2e_test.rs` |
| Tests — Auto Title | 28 | `src/tests/auto_title_test.rs` |
| Tests — Background Session | 14 | `src/tests/background_session_test.rs` |
| Tests — Baseline Merge | 10 | `src/tests/baseline_merge_test.rs` |
| Tests — Bash Blocklist | 6 | `src/tests/bash_blocklist_test.rs` |
| Tests — Bash Feedback Enrichment | 14 | `src/tests/bash_feedback_enrichment_test.rs` |
| Tests — Bash Interactive Reject | 37 | `src/tests/bash_interactive_reject_test.rs` |
| Tests — Bash Posix Quote | 9 | `src/tests/bash_posix_quote_test.rs` |
| Tests — Bash Retry Loop | 10 | `src/tests/bash_retry_loop_test.rs` |
| Tests — Bash Ssh Detection | 10 | `src/tests/bash_ssh_detection_test.rs` |
| Tests — Brain Agent Context | 12 | `src/tests/brain_agent_context_test.rs` |
| Tests — Brain Agent Service Phantom Lang | 11 | `src/tests/brain_agent_service_phantom_lang_test.rs` |
| Tests — Brain Agent Service Phantom | 22 | `src/tests/brain_agent_service_phantom_test.rs` |
| Tests — Brain Commands | 6 | `src/tests/brain_commands_test.rs` |
| Tests — Brain File Generic Guard | 4 | `src/tests/brain_file_generic_guard_test.rs` |
| Tests — Brain File Safety | 37 | `src/tests/brain_file_safety_test.rs` |
| Tests — Brain Filter Strip Empty Sections | 15 | `src/tests/brain_filter_strip_empty_sections_test.rs` |
| Tests — Brain Live Rebuild | 5 | `src/tests/brain_live_rebuild_test.rs` |
| Tests — Brain Project Overlay | 3 | `src/tests/brain_project_overlay_test.rs` |
| Tests — Brain Prompt Builder | 28 | `src/tests/brain_prompt_builder_test.rs` |
| Tests — Brain Provider Anthropic | 7 | `src/tests/brain_provider_anthropic_test.rs` |
| Tests — Brain Provider Codex Oauth | 6 | `src/tests/brain_provider_codex_oauth_test.rs` |
| Tests — Brain Provider Copilot | 8 | `src/tests/brain_provider_copilot_test.rs` |
| Tests — Brain Provider Error | 2 | `src/tests/brain_provider_error_test.rs` |
| Tests — Brain Provider Factory | 4 | `src/tests/brain_provider_factory_test.rs` |
| Tests — Brain Provider JSON Repair | 10 | `src/tests/brain_provider_json_repair_test.rs` |
| Tests — Brain Provider Qwen | 13 | `src/tests/brain_provider_qwen_test.rs` |
| Tests — Brain Provider Trait | 2 | `src/tests/brain_provider_trait_test.rs` |
| Tests — Brain Provider Types | 3 | `src/tests/brain_provider_types_test.rs` |
| Tests — Brain Self Update | 1 | `src/tests/brain_self_update_test.rs` |
| Tests — Brain Templates | 7 | `src/tests/brain_templates_test.rs` |
| Tests — Brain Tokenizer | 8 | `src/tests/brain_tokenizer_test.rs` |
| Tests — Brain Tools A2A Send | 16 | `src/tests/brain_tools_a2a_send_test.rs` |
| Tests — Brain Tools Bash | 21 | `src/tests/brain_tools_bash_test.rs` |
| Tests — Brain Tools Brave Search | 12 | `src/tests/brain_tools_brave_search_test.rs` |
| Tests — Brain Tools Browser Manager | 12 | `src/tests/brain_tools_browser_manager_test.rs` |
| Tests — Brain Tools Config Tool | 5 | `src/tests/brain_tools_config_tool_test.rs` |
| Tests — Brain Tools Doc Parser | 10 | `src/tests/brain_tools_doc_parser_test.rs` |
| Tests — Brain Tools Dynamic Loader | 6 | `src/tests/brain_tools_dynamic_loader_test.rs` |
| Tests — Brain Tools Dynamic Tool | 25 | `src/tests/brain_tools_dynamic_tool_test.rs` |
| Tests — Brain Tools Error | 7 | `src/tests/brain_tools_error_test.rs` |
| Tests — Brain Tools Exa Search | 18 | `src/tests/brain_tools_exa_search_test.rs` |
| Tests — Brain Tools Fuzzy | 11 | `src/tests/brain_tools_fuzzy_test.rs` |
| Tests — Brain Tools Hashline Hash | 10 | `src/tests/brain_tools_hashline_hash_test.rs` |
| Tests — Brain Tools Hashline Types | 15 | `src/tests/brain_tools_hashline_types_test.rs` |
| Tests — Brain Tools Load Brain File | 15 | `src/tests/brain_tools_load_brain_file_tests.rs` |
| Tests — Brain Tools Memory Search | 2 | `src/tests/brain_tools_memory_search_test.rs` |
| Tests — Brain Tools Read | 4 | `src/tests/brain_tools_read_test.rs` |
| Tests — Brain Tools Registry | 9 | `src/tests/brain_tools_registry_test.rs` |
| Tests — Brain Tools Slash Command | 8 | `src/tests/brain_tools_slash_command_test.rs` |
| Tests — Brain Tools Subagent Status | 9 | `src/tests/brain_tools_subagent_status_test.rs` |
| Tests — Brain Tools Tool Manage | 11 | `src/tests/brain_tools_tool_manage_test.rs` |
| Tests — Brain Tools Trait | 3 | `src/tests/brain_tools_trait_test.rs` |
| Tests — Brain Tools Whatsapp Send | 19 | `src/tests/brain_tools_whatsapp_send_test.rs` |
| Tests — Brain Tools Write Opencrabs File | 20 | `src/tests/brain_tools_write_opencrabs_file_tests.rs` |
| Tests — Brain Tools Write | 5 | `src/tests/brain_tools_write_test.rs` |
| Tests — Browser CDP Endpoint | 4 | `src/tests/browser_cdp_endpoint_test.rs` |
| Tests — Browser Close | 6 | `src/tests/browser_close_test.rs` |
| Tests — Browser Default Linux | 4 | `src/tests/browser_default_linux_test.rs` |
| Tests — Browser Default | 12 | `src/tests/browser_default_test.rs` |
| Tests — Browser Default Windows | 6 | `src/tests/browser_default_windows_test.rs` |
| Tests — Browser Drop | 2 | `src/tests/browser_drop_test.rs` |
| Tests — Browser E2E | 4 | `src/tests/browser_e2e_test.rs` |
| Tests — Browser Eval Cap | 5 | `src/tests/browser_eval_cap_test.rs` |
| Tests — Browser Find | 9 | `src/tests/browser_find_test.rs` |
| Tests — Browser Health | 3 | `src/tests/browser_health_test.rs` |
| Tests — Browser Locks | 5 | `src/tests/browser_locks_test.rs` |
| Tests — Browser Profile Wait | 4 | `src/tests/browser_profile_wait_test.rs` |
| Tests — Browser Screenshot Surface | 2 | `src/tests/browser_screenshot_surface_test.rs` |
| Tests — Browser Session | 4 | `src/tests/browser_session_test.rs` |
| Tests — Browser Stealth | 6 | `src/tests/browser_stealth_test.rs` |
| Tests — Build User Message Image | 3 | `src/tests/build_user_message_image_test.rs` |
| Tests — Bundled Plans | 20 | `src/tests/bundled_plans_test.rs` |
| Tests — Candle Whisper | 6 | `src/tests/candle_whisper_test.rs` |
| Tests — Channel Action | 4 | `src/tests/channel_action_test.rs` |
| Tests — Channel Command Media Marker | 3 | `src/tests/channel_command_media_marker_test.rs` |
| Tests — Channel Command Owner Gate | 3 | `src/tests/channel_command_owner_gate_test.rs` |
| Tests — Channel Commands | 20 | `src/tests/channel_commands_test.rs` |
| Tests — Channel Search | 32 | `src/tests/channel_search_test.rs` |
| Tests — Channel Session Resolve | 7 | `src/tests/channel_session_resolve_test.rs` |
| Tests — Channels Telegram Cowork | 10 | `src/tests/channels_telegram_cowork_test.rs` |
| Tests — Channels Telegram Session Resolve | 8 | `src/tests/channels_telegram_session_resolve_test.rs` |
| Tests — Channels | 5 | `src/tests/channels_tests.rs` |
| Tests — Channels Voice Service | 10 | `src/tests/channels_voice_service_test.rs` |
| Tests — Claude CLI Model | 7 | `src/tests/claude_cli_model_test.rs` |
| Tests — CLI Arg Too Long | 2 | `src/tests/cli_arg_too_long_test.rs` |
| Tests — CLI Headless Tools | 4 | `src/tests/cli_headless_tools_test.rs` |
| Tests — CLI Session Set Model | 6 | `src/tests/cli_session_set_model_test.rs` |
| Tests — CLI Supported Models | 9 | `src/tests/cli_supported_models_test.rs` |
| Tests — CLI | 28 | `src/tests/cli_test.rs` |
| Tests — Click To Expand | 6 | `src/tests/click_to_expand_test.rs` |
| Tests — Clipboard Image Paste | 2 | `src/tests/clipboard_image_paste_test.rs` |
| Tests — Codex CLI | 5 | `src/tests/codex_cli_test.rs` |
| Tests — Collapse Build Output | 9 | `src/tests/collapse_build_output_test.rs` |
| Tests — Collapse Home | 8 | `src/tests/collapse_home_test.rs` |
| Tests — Command Code CLI | 6 | `src/tests/command_code_cli_test.rs` |
| Tests — Command Handle Strip | 10 | `src/tests/command_handle_strip_test.rs` |
| Tests — Command Rich Table | 5 | `src/tests/command_rich_table_test.rs` |
| Tests — Compaction Prompts | 12 | `src/tests/compaction_prompts_test.rs` |
| Tests — Compaction | 28 | `src/tests/compaction_test.rs` |
| Tests — Config Provider Registry | 3 | `src/tests/config_provider_registry_test.rs` |
| Tests — Config Dotted Caps | 6 | `src/tests/config_dotted_caps_test.rs` |
| Tests — Config Last Good Recovery | 3 | `src/tests/config_last_good_recovery_test.rs` |
| Tests — Config Owner Seed Migration | 5 | `src/tests/config_owner_seed_migration_test.rs` |
| Tests — Config Repair | 7 | `src/tests/config_repair_test.rs` |
| Tests — Config Secrets | 5 | `src/tests/config_secrets_test.rs` |
| Tests — Config Types Loader | 23 | `src/tests/config_types_loader_test.rs` |
| Tests — Config Update | 4 | `src/tests/config_update_test.rs` |
| Tests — Config Watcher | 5 | `src/tests/config_watcher_test.rs` |
| Tests — Context Window | 14 | `src/tests/context_window_test.rs` |
| Tests — Corrupted Tool Call | 7 | `src/tests/corrupted_tool_call_test.rs` |
| Tests — Cowork Connect | 2 | `src/tests/cowork_connect_test.rs` |
| Tests — Cron Profile Isolation | 6 | `src/tests/cron_profile_isolation_test.rs` |
| Tests — Cron Schedule Util | 12 | `src/tests/cron_schedule_util_test.rs` |
| Tests — Cron Scheduler Lock | 4 | `src/tests/cron_scheduler_lock_test.rs` |
| Tests — Cron | 59 | `src/tests/cron_test.rs` |
| Tests — Cron Tool Registry | 2 | `src/tests/cron_tool_registry_test.rs` |
| Tests — Cross Provider Model Leak Guard | 6 | `src/tests/cross_provider_model_leak_guard_test.rs` |
| Tests — Custom Model Paste | 5 | `src/tests/custom_model_paste_test.rs` |
| Tests — Custom Provider Cache Autoenable | 10 | `src/tests/custom_provider_cache_autoenable_test.rs` |
| Tests — Custom Provider Live Fetch Regression | 5 | `src/tests/custom_provider_live_fetch_regression_test.rs` |
| Tests — Custom Provider No Models | 3 | `src/tests/custom_provider_no_models_test.rs` |
| Tests — Custom Provider Rename Keys Toml | 8 | `src/tests/custom_provider_rename_keys_toml_test.rs` |
| Tests — Custom Provider Section Resolver | 2 | `src/tests/custom_provider_section_resolver_test.rs` |
| Tests — Custom Provider | 27 | `src/tests/custom_provider_test.rs` |
| Tests — Daemon Health | 10 | `src/tests/daemon_health_test.rs` |
| Tests — DB Database | 5 | `src/tests/db_database_test.rs` |
| Tests — DB Models | 5 | `src/tests/db_models_test.rs` |
| Tests — DB Repository Channel Message | 1 | `src/tests/db_repository_channel_message_test.rs` |
| Tests — DB Repository File | 2 | `src/tests/db_repository_file_test.rs` |
| Tests — DB Repository Message | 2 | `src/tests/db_repository_message_test.rs` |
| Tests — DB Repository Project | 5 | `src/tests/db_repository_project_test.rs` |
| Tests — DB Repository Session | 2 | `src/tests/db_repository_session_test.rs` |
| Tests — DB Retry | 8 | `src/tests/db_retry_test.rs` |
| Tests — Directive Discovery | 13 | `src/tests/directive_discovery_test.rs` |
| Tests — Discord Handler | 2 | `src/tests/discord_handler_test.rs` |
| Tests — Discord Tool Group | 4 | `src/tests/discord_tool_group_test.rs` |
| Tests — Doc Gen Docx | 7 | `src/tests/doc_gen_docx_test.rs` |
| Tests — Doc Gen PDF | 14 | `src/tests/doc_gen_pdf_test.rs` |
| Tests — Doc Gen Pptx | 5 | `src/tests/doc_gen_pptx_test.rs` |
| Tests — Doc Gen Xlsx | 6 | `src/tests/doc_gen_xlsx_test.rs` |
| Tests — Doc Parser Page Range | 5 | `src/tests/doc_parser_page_range_test.rs` |
| Tests — Dynamic Tool Coerce | 13 | `src/tests/dynamic_tool_coerce_test.rs` |
| Tests — Dynamic Tool Parse Error | 12 | `src/tests/dynamic_tool_parse_error_test.rs` |
| Tests — Eval Baseline | 6 | `src/tests/eval_baseline_test.rs` |
| Tests — Eval Before After | 5 | `src/tests/eval_before_after_test.rs` |
| Tests — Eval Compaction | 5 | `src/tests/eval_compaction_test.rs` |
| Tests — Eval Live Resolver | 4 | `src/tests/eval_live_resolver_test.rs` |
| Tests — Eval Manifest | 3 | `src/tests/eval_manifest_test.rs` |
| Tests — Eval Panel | 8 | `src/tests/eval_panel_test.rs` |
| Tests — Eval Produce | 4 | `src/tests/eval_produce_test.rs` |
| Tests — Eval Recall | 7 | `src/tests/eval_recall_test.rs` |
| Tests — Eval Replay Provider | 5 | `src/tests/eval_replay_provider_test.rs` |
| Tests — Eval Runner | 6 | `src/tests/eval_runner_test.rs` |
| Tests — Eval Scorer | 4 | `src/tests/eval_scorer_test.rs` |
| Tests — Evolve Diagnose | 7 | `src/tests/evolve_diagnose_test.rs` |
| Tests — Evolve Systemd Restart | 14 | `src/tests/evolve_systemd_restart_test.rs` |
| Tests — Evolve | 23 | `src/tests/evolve_test.rs` |
| Tests — Exa Search | 4 | `src/tests/exa_search_test.rs` |
| Tests — Fallback Streak | 7 | `src/tests/fallback_streak_test.rs` |
| Tests — Fallback Vision | 57 | `src/tests/fallback_vision_test.rs` |
| Tests — Feedback Policy | 6 | `src/tests/feedback_policy_test.rs` |
| Tests — File Extract | 37 | `src/tests/file_extract_test.rs` |
| Tests — Follow Up Intermediate Flush | 5 | `src/tests/follow_up_intermediate_flush_test.rs` |
| Tests — Follow Up Question | 10 | `src/tests/follow_up_question_test.rs` |
| Tests — Force Default | 2 | `src/tests/force_default_test.rs` |
| Tests — Format User Error | 9 | `src/tests/format_user_error_test.rs` |
| Tests — Gemini Fetch | 3 | `src/tests/gemini_fetch_test.rs` |
| Tests — Gemini Schema Sanitize | 10 | `src/tests/gemini_schema_sanitize_test.rs` |
| Tests — Generate Image Backend | 5 | `src/tests/generate_image_backend_test.rs` |
| Tests — Git Branch | 8 | `src/tests/git_branch_test.rs` |
| Tests — Github Provider | 38 | `src/tests/github_provider_test.rs` |
| Tests — Glob Tool | 3 | `src/tests/glob_tool_test.rs` |
| Tests — Goal Command | 11 | `src/tests/goal_command_test.rs` |
| Tests — Goal Judge | 12 | `src/tests/goal_judge_test.rs` |
| Tests — Goal Manage | 6 | `src/tests/goal_manage_test.rs` |
| Tests — Handshake Timeout | 4 | `src/tests/handshake_timeout_test.rs` |
| Tests — Hashline | 32 | `src/tests/hashline_test.rs` |
| Tests — Html Comment Strip | 14 | `src/tests/html_comment_strip_test.rs` |
| Tests — Http Request | 5 | `src/tests/http_request_test.rs` |
| Tests — Image Util | 9 | `src/tests/image_util_test.rs` |
| Tests — Incident Log Dedup | 10 | `src/tests/incident_log_dedup_test.rs` |
| Tests — Kimi Plan | 6 | `src/tests/kimi_plan_test.rs` |
| Tests — Kimi Reasoning Map | 7 | `src/tests/kimi_reasoning_map_test.rs` |
| Tests — Kimi Reasoning | 9 | `src/tests/kimi_reasoning_test.rs` |
| Tests — Lazy Tools | 8 | `src/tests/lazy_tools_test.rs` |
| Tests — Local Provider Gate | 6 | `src/tests/local_provider_gate_test.rs` |
| Tests — Logging Log Files | 4 | `src/tests/logging_log_files_test.rs` |
| Tests — Logging Logger | 4 | `src/tests/logging_logger_test.rs` |
| Tests — Markdown Render | 10 | `src/tests/markdown_render_test.rs` |
| Tests — Memory Search | 3 | `src/tests/memory_search_test.rs` |
| Tests — Memory Store | 6 | `src/tests/memory_store_test.rs` |
| Tests — Merge Provider Keys | 5 | `src/tests/merge_provider_keys_test.rs` |
| Tests — Mimo Tool Call Hint | 3 | `src/tests/mimo_tool_call_hint_test.rs` |
| Tests — Mission Control Activity Service | 8 | `src/tests/mission_control_activity_service_test.rs` |
| Tests — Mission Control Command | 2 | `src/tests/mission_control_command_test.rs` |
| Tests — Mission Control Dedup Detail | 5 | `src/tests/mission_control_dedup_detail_test.rs` |
| Tests — Mission Control Inbox Service | 6 | `src/tests/mission_control_inbox_service_test.rs` |
| Tests — Mission Control Input | 23 | `src/tests/mission_control_input_test.rs` |
| Tests — Mission Control Layout | 7 | `src/tests/mission_control_layout_test.rs` |
| Tests — Mission Control Report | 2 | `src/tests/mission_control_report_test.rs` |
| Tests — Mission Control Schedule Service | 5 | `src/tests/mission_control_schedule_service_test.rs` |
| Tests — Mission Control Skill Inbox | 8 | `src/tests/mission_control_skill_inbox_test.rs` |
| Tests — Model Fetch | 11 | `src/tests/model_fetch_test.rs` |
| Tests — Mouse Fragment Filter | 13 | `src/tests/mouse_fragment_filter_test.rs` |
| Tests — New Session Pane Binding | 3 | `src/tests/new_session_pane_binding_test.rs` |
| Tests — Nonstream Compat | 5 | `src/tests/nonstream_compat_test.rs` |
| Tests — Onboard Channel | 13 | `src/tests/onboard_channel_test.rs` |
| Tests — Onboarding Brain | 23 | `src/tests/onboarding_brain_test.rs` |
| Tests — Onboarding Channel Deep Link | 7 | `src/tests/onboarding_channel_deep_link_test.rs` |
| Tests — Onboarding Custom Model Input | 9 | `src/tests/onboarding_custom_model_input_test.rs` |
| Tests — Onboarding Field Nav | 53 | `src/tests/onboarding_field_nav_test.rs` |
| Tests — Onboarding Keys | 4 | `src/tests/onboarding_keys_test.rs` |
| Tests — Onboarding Navigation | 26 | `src/tests/onboarding_navigation_test.rs` |
| Tests — Onboarding No Silent Commit | 8 | `src/tests/onboarding_no_silent_commit_test.rs` |
| Tests — Onboarding Types | 17 | `src/tests/onboarding_types_test.rs` |
| Tests — Onboarding User Scroll | 8 | `src/tests/onboarding_user_scroll_test.rs` |
| Tests — Onboarding Welcome | 9 | `src/tests/onboarding_welcome_test.rs` |
| Tests — Onboarding Wizard | 67 | `src/tests/onboarding_wizard_test.rs` |
| Tests — Openai Provider | 16 | `src/tests/openai_provider_test.rs` |
| Tests — Opencode Provider | 21 | `src/tests/opencode_provider_test.rs` |
| Tests — Orphan Close Tag Strip | 9 | `src/tests/orphan_close_tag_strip_test.rs` |
| Tests — Orphan Think Close Tag | 13 | `src/tests/orphan_think_close_tag_test.rs` |
| Tests — Owner Plus Normalization | 5 | `src/tests/owner_plus_normalization_test.rs` |
| Tests — Owner Resolve | 6 | `src/tests/owner_resolve_test.rs` |
| Tests — Parallel Tools | 3 | `src/tests/parallel_tools_test.rs` |
| Tests — PDF Page Range Parser | 25 | `src/tests/pdf_page_range_parser_test.rs` |
| Tests — PDF Smart Routing | 5 | `src/tests/pdf_smart_routing_test.rs` |
| Tests — PDF To Images | 4 | `src/tests/pdf_to_images_test.rs` |
| Tests — PDF Vision | 3 | `src/tests/pdf_vision_test.rs` |
| Tests — Pending Request Age | 4 | `src/tests/pending_request_age_test.rs` |
| Tests — Phantom Cleanup Intent | 7 | `src/tests/phantom_cleanup_intent_test.rs` |
| Tests — Phantom DB Persistence | 2 | `src/tests/phantom_db_persistence_test.rs` |
| Tests — Phantom Deferment | 11 | `src/tests/phantom_deferment_test.rs` |
| Tests — Phantom Going To | 3 | `src/tests/phantom_going_to_test.rs` |
| Tests — Phantom Post Success Exemption | 11 | `src/tests/phantom_post_success_exemption_test.rs` |
| Tests — Phantom Pronoun Drop | 8 | `src/tests/phantom_pronoun_drop_test.rs` |
| Tests — Phantom Work Announcement | 13 | `src/tests/phantom_work_announcement_test.rs` |
| Tests — Plan Document | 15 | `src/tests/plan_document_test.rs` |
| Tests — Plan Files | 15 | `src/tests/plan_files_test.rs` |
| Tests — Plan Flow Keyboard Gate | 6 | `src/tests/plan_flow_keyboard_gate_test.rs` |
| Tests — Plan Gate | 6 | `src/tests/plan_gate_test.rs` |
| Tests — Plan Mode Command | 13 | `src/tests/plan_mode_command_test.rs` |
| Tests — Plan Reminder | 6 | `src/tests/plan_reminder_test.rs` |
| Tests — Plan Tool Contract | 16 | `src/tests/plan_tool_contract_test.rs` |
| Tests — Plan Tool Description | 8 | `src/tests/plan_tool_description_test.rs` |
| Tests — Plan Tool | 33 | `src/tests/plan_tool_test.rs` |
| Tests — Plan Window | 21 | `src/tests/plan_window_test.rs` |
| Tests — Post Evolve | 5 | `src/tests/post_evolve_test.rs` |
| Tests — Profile Pid Lock | 3 | `src/tests/profile_pid_lock_test.rs` |
| Tests — Profile Preempt | 4 | `src/tests/profile_preempt_test.rs` |
| Tests — Profile | 61 | `src/tests/profile_test.rs` |
| Tests — Profiles Dialog | 49 | `src/tests/profiles_dialog_test.rs` |
| Tests — Project File Archive | 3 | `src/tests/project_file_archive_test.rs` |
| Tests — Project File Slug | 4 | `src/tests/project_file_slug_test.rs` |
| Tests — Project Skills | 5 | `src/tests/project_skills_test.rs` |
| Tests — Project | 25 | `src/tests/project_test.rs` |
| Tests — Prompt Analyzer | 29 | `src/tests/prompt_analyzer_test.rs` |
| Tests — Prompt Compiled Features | 13 | `src/tests/prompt_compiled_features_test.rs` |
| Tests — Prompt Inline Edit Directive | 5 | `src/tests/prompt_inline_edit_directive_test.rs` |
| Tests — Prompt Known Paths | 8 | `src/tests/prompt_known_paths_test.rs` |
| Tests — Provider By Name Restore | 5 | `src/tests/provider_by_name_restore_test.rs` |
| Tests — Provider Config Regression | 29 | `src/tests/provider_config_regression_test.rs` |
| Tests — Provider Context Window Override | 2 | `src/tests/provider_context_window_override_test.rs` |
| Tests — Provider Error Proxy | 27 | `src/tests/provider_error_proxy_test.rs` |
| Tests — Provider Factory Regression | 31 | `src/tests/provider_factory_regression_test.rs` |
| Tests — Provider Picker Setup Hint | 4 | `src/tests/provider_picker_setup_hint_test.rs` |
| Tests — Provider Registry | 8 | `src/tests/provider_registry_test.rs` |
| Tests — Provider Retry Consolidation | 9 | `src/tests/provider_retry_consolidation_test.rs` |
| Tests — Provider Sync | 8 | `src/tests/provider_sync_test.rs` |
| Tests — QR Render | 10 | `src/tests/qr_render_test.rs` |
| Tests — Queued Message | 15 | `src/tests/queued_message_test.rs` |
| Tests — Qwen Detect | 18 | `src/tests/qwen_detect_test.rs` |
| Tests — Qwen Tool Extractor | 72 | `src/tests/qwen_tool_extractor_test.rs` |
| Tests — Qwen Tool Marker Strip | 7 | `src/tests/qwen_tool_marker_strip_test.rs` |
| Tests — Rate Limiter | 8 | `src/tests/rate_limiter_test.rs` |
| Tests — React Marker | 36 | `src/tests/react_marker_test.rs` |
| Tests — Read Media Redirect | 3 | `src/tests/read_media_redirect_test.rs` |
| Tests — Reasoning Lines | 7 | `src/tests/reasoning_lines_test.rs` |
| Tests — Rebuild Notify | 5 | `src/tests/rebuild_notify_test.rs` |
| Tests — Recent Paths | 17 | `src/tests/recent_paths_test.rs` |
| Tests — Rename Session | 7 | `src/tests/rename_session_test.rs` |
| Tests — Respond To Group Persist | 3 | `src/tests/respond_to_group_persist_test.rs` |
| Tests — RSI Brain Dedup | 27 | `src/tests/rsi_brain_dedup_test.rs` |
| Tests — RSI Command Patterns | 5 | `src/tests/rsi_command_patterns_test.rs` |
| Tests — RSI Fallback Wrap | 4 | `src/tests/rsi_fallback_wrap_test.rs` |
| Tests — RSI Git History | 12 | `src/tests/rsi_git_history_test.rs` |
| Tests — RSI Notification Redaction | 5 | `src/tests/rsi_notification_redaction_test.rs` |
| Tests — RSI Prompt Triage | 3 | `src/tests/rsi_prompt_triage_test.rs` |
| Tests — RSI Proposals | 19 | `src/tests/rsi_proposals_test.rs` |
| Tests — RSI Pruned | 23 | `src/tests/rsi_pruned_test.rs` |
| Tests — RSI Self Improve Dedup | 2 | `src/tests/rsi_self_improve_dedup_test.rs` |
| Tests — RSI Skill Proposals | 9 | `src/tests/rsi_skill_proposals_test.rs` |
| Tests — RSI Skill Sequences | 6 | `src/tests/rsi_skill_sequences_test.rs` |
| Tests — RSI Staleness | 6 | `src/tests/rsi_staleness_test.rs` |
| Tests — RSI Subsystem | 23 | `src/tests/rsi_subsystem_test.rs` |
| Tests — RSI Sync Cap Bail | 9 | `src/tests/rsi_sync_cap_bail_test.rs` |
| Tests — RSI Sync | 15 | `src/tests/rsi_sync_test.rs` |
| Tests — RSI | 83 | `src/tests/rsi_test.rs` |
| Tests — Rtk Autodownload | 4 | `src/tests/rtk_autodownload_test.rs` |
| Tests — Rtk Rewrite | 15 | `src/tests/rtk_rewrite_test.rs` |
| Tests — Rtk Sysadmin Supported | 6 | `src/tests/rtk_sysadmin_supported_test.rs` |
| Tests — Rtk Tracker | 5 | `src/tests/rtk_tracker_test.rs` |
| Tests — Runtime Info Home Anchor | 6 | `src/tests/runtime_info_home_anchor_test.rs` |
| Tests — Sanitize Code Edit Block | 10 | `src/tests/sanitize_code_edit_block_test.rs` |
| Tests — Sanitize Redaction | 31 | `src/tests/sanitize_redaction_test.rs` |
| Tests — Self Healing | 88 | `src/tests/self_healing_test.rs` |
| Tests — Self Improve Failure Log Guard | 3 | `src/tests/self_improve_failure_log_guard_test.rs` |
| Tests — Self Improve Guard | 6 | `src/tests/self_improve_guard_test.rs` |
| Tests — Self Update Path | 6 | `src/tests/self_update_path_test.rs` |
| Tests — Service Scope | 11 | `src/tests/service_scope_test.rs` |
| Tests — Services Context | 2 | `src/tests/services_context_test.rs` |
| Tests — Services File | 11 | `src/tests/services_file_test.rs` |
| Tests — Services Message | 10 | `src/tests/services_message_test.rs` |
| Tests — Services Project | 7 | `src/tests/services_project_test.rs` |
| Tests — Services Session | 10 | `src/tests/services_session_test.rs` |
| Tests — Session Chat ID Lookup | 8 | `src/tests/session_chat_id_lookup_test.rs` |
| Tests — Session Provider Wrap | 9 | `src/tests/session_provider_wrap_test.rs` |
| Tests — Session Working Dir | 19 | `src/tests/session_working_dir_test.rs` |
| Tests — Skill Slash Dispatch | 8 | `src/tests/skill_slash_dispatch_test.rs` |
| Tests — Skills Dialog | 18 | `src/tests/skills_dialog_test.rs` |
| Tests — Skills | 17 | `src/tests/skills_test.rs` |
| Tests — Slack Blocks | 7 | `src/tests/slack_blocks_test.rs` |
| Tests — Slack Fmt | 21 | `src/tests/slack_fmt_test.rs` |
| Tests — Slack Handler | 2 | `src/tests/slack_handler_test.rs` |
| Tests — Slack Reactions | 2 | `src/tests/slack_reactions_test.rs` |
| Tests — Slack Send Content Type | 2 | `src/tests/slack_send_content_type_test.rs` |
| Tests — Slack Tool Group | 5 | `src/tests/slack_tool_group_test.rs` |
| Tests — Slash Autocomplete Dimensions | 18 | `src/tests/slash_autocomplete_dimensions_test.rs` |
| Tests — Split Pane | 21 | `src/tests/split_pane_test.rs` |
| Tests — Startup Checks | 6 | `src/tests/startup_checks_test.rs` |
| Tests — Stream Loop | 19 | `src/tests/stream_loop_test.rs` |
| Tests — Streaming Active Secs | 2 | `src/tests/streaming_active_secs_test.rs` |
| Tests — Streaming Tok Per Sec Guard | 10 | `src/tests/streaming_tok_per_sec_guard_test.rs` |
| Tests — Streaming Tps Accumulator | 12 | `src/tests/streaming_tps_accumulator_test.rs` |
| Tests — STT Fallback Chain | 6 | `src/tests/stt_fallback_chain_test.rs` |
| Tests — Subagent | 84 | `src/tests/subagent_test.rs` |
| Tests — Subagent Tool Description | 7 | `src/tests/subagent_tool_description_test.rs` |
| Tests — Suggest Followups | 9 | `src/tests/suggest_followups_test.rs` |
| Tests — System Continuation | 6 | `src/tests/system_continuation_test.rs` |
| Tests — Systemd Unit | 3 | `src/tests/systemd_unit_test.rs` |
| Tests — Telegram Command Sanitize | 12 | `src/tests/telegram_command_sanitize_test.rs` |
| Tests — Telegram Flow Chrome | 39 | `src/tests/telegram_flow_chrome_test.rs` |
| Tests — Telegram Handler | 17 | `src/tests/telegram_handler_test.rs` |
| Tests — Telegram Impersonation | 7 | `src/tests/telegram_impersonation_test.rs` |
| Tests — Telegram Join Detection | 4 | `src/tests/telegram_join_detection_test.rs` |
| Tests — Telegram Last Intermediate Footer | 7 | `src/tests/telegram_last_intermediate_footer_test.rs` |
| Tests — Telegram Model Callback Data | 3 | `src/tests/telegram_model_callback_data_test.rs` |
| Tests — Telegram Newest Msg ID | 4 | `src/tests/telegram_newest_msg_id_test.rs` |
| Tests — Telegram Pending Question Steer | 5 | `src/tests/telegram_pending_question_steer_test.rs` |
| Tests — Telegram Photo Batching | 8 | `src/tests/telegram_photo_batching_test.rs` |
| Tests — Telegram Plan Render | 9 | `src/tests/telegram_plan_render_test.rs` |
| Tests — Telegram Pre Tool Rolling | 1 | `src/tests/telegram_pre_tool_rolling_test.rs` |
| Tests — Telegram Quote Reply | 19 | `src/tests/telegram_quote_reply_test.rs` |
| Tests — Telegram Raw Update Parse | 2 | `src/tests/telegram_raw_update_parse_test.rs` |
| Tests — Telegram Reaction Map | 5 | `src/tests/telegram_reaction_map_test.rs` |
| Tests — Telegram Reaction Prompt | 11 | `src/tests/telegram_reaction_prompt_test.rs` |
| Tests — Telegram Reaction Queue | 7 | `src/tests/telegram_reaction_queue_test.rs` |
| Tests — Telegram Reply Context Recovery | 8 | `src/tests/telegram_reply_context_recovery_test.rs` |
| Tests — Telegram Resume | 65 | `src/tests/telegram_resume_test.rs` |
| Tests — Telegram Rich Decode | 17 | `src/tests/telegram_rich_decode_test.rs` |
| Tests — Telegram Rich JSON | 10 | `src/tests/telegram_rich_json_test.rs` |
| Tests — Telegram Rich Parse | 29 | `src/tests/telegram_rich_parse_test.rs` |
| Tests — Telegram Rich | 14 | `src/tests/telegram_rich_test.rs` |
| Tests — Telegram Send Caption | 9 | `src/tests/telegram_send_caption_test.rs` |
| Tests — Telegram Send Input File | 5 | `src/tests/telegram_send_input_file_test.rs` |
| Tests — Telegram Send Retry | 4 | `src/tests/telegram_send_retry_test.rs` |
| Tests — Telegram Send Thread ID Override | 9 | `src/tests/telegram_send_thread_id_override_test.rs` |
| Tests — Telegram Session Resolve | 18 | `src/tests/telegram_session_resolve_test.rs` |
| Tests — Telegram Status Message | 15 | `src/tests/telegram_status_message_test.rs` |
| Tests — Telegram Thread ID Lookup | 8 | `src/tests/telegram_thread_id_lookup_test.rs` |
| Tests — Telegram Tool Group | 65 | `src/tests/telegram_tool_group_test.rs` |
| Tests — Telegram Topic Listing | 6 | `src/tests/telegram_topic_listing_test.rs` |
| Tests — Template Governance | 15 | `src/tests/template_governance_test.rs` |
| Tests — Text Complete | 21 | `src/tests/text_complete_test.rs` |
| Tests — Token Tracking | 29 | `src/tests/token_tracking_test.rs` |
| Tests — Tool Arg Unescape | 10 | `src/tests/tool_arg_unescape_test.rs` |
| Tests — Tool Description Redaction | 6 | `src/tests/tool_description_redaction_test.rs` |
| Tests — Tool Execution Repo | 4 | `src/tests/tool_execution_repo_test.rs` |
| Tests — Tool Execution Stats | 2 | `src/tests/tool_execution_stats_test.rs` |
| Tests — Tool Loop Helpers | 34 | `src/tests/tool_loop_helpers_test.rs` |
| Tests — Tool Name Heal | 11 | `src/tests/tool_name_heal_test.rs` |
| Tests — Tools Md Regression | 9 | `src/tests/tools_md_regression_test.rs` |
| Tests — TTS Fallback Chain | 6 | `src/tests/tts_fallback_chain_test.rs` |
| Tests — TUI App State | 2 | `src/tests/tui_app_state_test.rs` |
| Tests — TUI Components Logo | 2 | `src/tests/tui_components_logo_test.rs` |
| Tests — TUI Drop Path | 4 | `src/tests/tui_drop_path_test.rs` |
| Tests — TUI Error | 21 | `src/tests/tui_error_test.rs` |
| Tests — TUI Events | 4 | `src/tests/tui_events_test.rs` |
| Tests — TUI Highlight | 8 | `src/tests/tui_highlight_test.rs` |
| Tests — TUI Markdown | 9 | `src/tests/tui_markdown_test.rs` |
| Tests — TUI Plan Tests | 27 | `src/tests/tui_plan_tests_test.rs` |
| Tests — TUI Render Clear | 4 | `src/tests/tui_render_clear_test.rs` |
| Tests — TUI Render Utils | 12 | `src/tests/tui_render_utils_test.rs` |
| Tests — TUI Tool Stack | 6 | `src/tests/tui_tool_stack_test.rs` |
| Tests — Usage Activity Columns | 9 | `src/tests/usage_activity_columns_test.rs` |
| Tests — Usage Cache | 15 | `src/tests/usage_cache_test.rs` |
| Tests — Usage Categorizer | 4 | `src/tests/usage_categorizer_test.rs` |
| Tests — Usage Cosmetic Alias | 13 | `src/tests/usage_cosmetic_alias_test.rs` |
| Tests — Usage Dashboard | 6 | `src/tests/usage_dashboard_test.rs` |
| Tests — Usage Data | 7 | `src/tests/usage_data_test.rs` |
| Tests — Usage Grouping | 18 | `src/tests/usage_grouping_test.rs` |
| Tests — Usage Ledger | 5 | `src/tests/usage_ledger_test.rs` |
| Tests — User Correction Metadata | 3 | `src/tests/user_correction_metadata_test.rs` |
| Tests — Utils File Extract | 8 | `src/tests/utils_file_extract_test.rs` |
| Tests — Utils Install | 6 | `src/tests/utils_install_test.rs` |
| Tests — Utils Retry | 8 | `src/tests/utils_retry_test.rs` |
| Tests — Utils Sanitize | 41 | `src/tests/utils_sanitize_test.rs` |
| Tests — Utils String | 22 | `src/tests/utils_string_test.rs` |
| Tests — Voice Local TTS | 15 | `src/tests/voice_local_tts_test.rs` |
| Tests — Voice Local Whisper | 25 | `src/tests/voice_local_whisper_test.rs` |
| Tests — Voice Onboarding | 65 | `src/tests/voice_onboarding_test.rs` |
| Tests — Voice Openai Compatible | 12 | `src/tests/voice_openai_compatible_test.rs` |
| Tests — Voice Service | 14 | `src/tests/voice_service_test.rs` |
| Tests — Voice STT Dispatch | 21 | `src/tests/voice_stt_dispatch_test.rs` |
| Tests — Voice Voicebox | 15 | `src/tests/voice_voicebox_test.rs` |
| Tests — Wait Agent Resolver | 12 | `src/tests/wait_agent_resolver_test.rs` |
| Tests — Web Browser Routing | 9 | `src/tests/web_browser_routing_test.rs` |
| Tests — Web Scrape Benchmark | 1 | `src/tests/web_scrape_benchmark_test.rs` |
| Tests — Web Scrape Clean | 7 | `src/tests/web_scrape_clean_test.rs` |
| Tests — Web Scrape Export | 5 | `src/tests/web_scrape_export_test.rs` |
| Tests — Web Scrape Extract | 5 | `src/tests/web_scrape_extract_test.rs` |
| Tests — Web Scrape Fetch | 4 | `src/tests/web_scrape_fetch_test.rs` |
| Tests — Web Scrape Markdown | 6 | `src/tests/web_scrape_markdown_test.rs` |
| Tests — Web Scrape Sitemap | 6 | `src/tests/web_scrape_sitemap_test.rs` |
| Tests — Web Scrape SSRF | 8 | `src/tests/web_scrape_ssrf_test.rs` |
| Tests — Web Scrape Tool | 6 | `src/tests/web_scrape_tool_test.rs` |
| Tests — Web Search | 4 | `src/tests/web_search_test.rs` |
| Tests — Whatsapp Handler | 6 | `src/tests/whatsapp_handler_test.rs` |
| Tests — Whatsapp Owner Filter | 15 | `src/tests/whatsapp_owner_filter_test.rs` |
| Tests — Whatsapp Photo Batching | 11 | `src/tests/whatsapp_photo_batching_test.rs` |
| Tests — Whatsapp QR Replay | 4 | `src/tests/whatsapp_qr_replay_test.rs` |
| Tests — Whatsapp State | 7 | `src/tests/whatsapp_state_test.rs` |
| Tests — Whatsapp Store | 15 | `src/tests/whatsapp_store_test.rs` |
| Tests — Word Delete Keybinding | 7 | `src/tests/word_delete_keybinding_test.rs` |
| **Total** | **5,071** | Authoritative count from `cargo test --all-features` (lib test binary): 5,071 run by default + 25 `#[ignore]`d. The 453 per-module rows above are regenerated from `src/tests/`; re-run `cargo test` for the live number. |

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