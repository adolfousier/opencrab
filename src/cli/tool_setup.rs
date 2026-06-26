//! Shared agent tool-registry construction.
//!
//! The interactive startup (TUI + channels) and the headless multi-profile
//! cron daemon both need an `AgentService` backed by the same core tools.
//! Keeping the registration in ONE place is what stops the daemon from running
//! cron jobs against an empty registry ("No tools registered" →
//! "Tool not found: bash") and keeps the two entry points from drifting.

use std::sync::Arc;

use crate::brain::tools::registry::ToolRegistry;
use crate::config::Config;
use crate::db::Database;

/// Register the dependency-light CORE agent tools: file ops, shell, search,
/// workflow, memory/brain, session/channel/cron/a2a/config/slash/rename,
/// follow-up, tool discovery, the config-dependent tools, sub-agent + team
/// orchestration, and RSI. These need only the DB pool + config (managers are
/// created inline and used nowhere else), so both entry points can share one
/// registry definition.
///
/// Browser, channel-send, media, and rebuild/evolve tools are registered
/// separately by the interactive path — they need managers the daemon lacks.
pub(crate) fn register_core_agent_tools(
    tool_registry: &Arc<ToolRegistry>,
    db: &Database,
    config: &Config,
) {
    use crate::brain::tools::{
        bash::BashTool, code_exec::CodeExecTool, config_tool::ConfigTool, context::ContextTool,
        doc_parser::DocParserTool, edit::EditTool, follow_up_question::FollowUpQuestionTool,
        glob::GlobTool, grep::GrepTool, http::HttpClientTool, load_brain_file::LoadBrainFileTool,
        ls::LsTool, memory_search::MemorySearchTool, notebook::NotebookEditTool,
        pdf_to_images::PdfToImagesTool, plan_tool::PlanTool, read::ReadTool,
        rename_session::RenameSessionTool, session_search::SessionSearchTool,
        slash_command::SlashCommandTool, task::TaskTool, web_search::WebSearchTool,
        write::WriteTool, write_opencrabs_file::WriteOpenCrabsFileTool,
    };
    // Phase 1: Essential file operations
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(crate::brain::tools::hashline::HashlineEditTool));
    tool_registry.register(Arc::new(BashTool));
    tool_registry.register(Arc::new(LsTool));
    tool_registry.register(Arc::new(GlobTool));
    tool_registry.register(Arc::new(GrepTool));
    // Phase 2: Advanced features
    tool_registry.register(Arc::new(WebSearchTool));
    tool_registry.register(Arc::new(CodeExecTool));
    tool_registry.register(Arc::new(NotebookEditTool));
    tool_registry.register(Arc::new(DocParserTool));
    tool_registry.register(Arc::new(PdfToImagesTool));
    // Phase 3: Workflow & integration
    tool_registry.register(Arc::new(TaskTool));
    tool_registry.register(Arc::new(ContextTool));
    tool_registry.register(Arc::new(HttpClientTool));
    tool_registry.register(Arc::new(PlanTool));
    // Memory search (built-in FTS5, always available)
    tool_registry.register(Arc::new(MemorySearchTool));
    // On-demand brain file loader — agent fetches USER.md, MEMORY.md etc. only when needed
    tool_registry.register(Arc::new(LoadBrainFileTool));
    // OpenCrabs file writer — agent can edit/append/overwrite any file in ~/.opencrabs/
    tool_registry.register(Arc::new(WriteOpenCrabsFileTool));
    // Session search — hybrid QMD search across all session message history
    tool_registry.register(Arc::new(SessionSearchTool::new(db.pool().clone())));
    // Mission control report — shareable analytics/activity/inbox/schedule the agent can send to a chat
    tool_registry.register(Arc::new(
        crate::brain::tools::mission_control_report::MissionControlReportTool::new(
            db.pool().clone(),
        ),
    ));
    // Channel search — search passively captured channel messages (Telegram groups, etc.)
    tool_registry.register(Arc::new(
        crate::brain::tools::channel_search::ChannelSearchTool::new(
            crate::db::ChannelMessageRepository::new(db.pool().clone()),
        ),
    ));
    // Cron job management — agent can create/list/delete/enable/disable scheduled jobs
    tool_registry.register(Arc::new(
        crate::brain::tools::cron_manage::CronManageTool::new(crate::db::CronJobRepository::new(
            db.pool().clone(),
        )),
    ));
    // A2A send — agent can communicate with remote A2A agents
    tool_registry.register(Arc::new(crate::brain::tools::a2a_send::A2aSendTool::new()));
    // Config management (read/write config.toml, commands.toml)
    tool_registry.register(Arc::new(ConfigTool));
    // Slash command invocation (agent can call any slash command)
    tool_registry.register(Arc::new(SlashCommandTool));
    // Session rename — agent can update the current session's title
    tool_registry.register(Arc::new(RenameSessionTool));
    // Follow-up question — agent asks the user a multi-choice question
    // mid-task and blocks until they click an option button.
    tool_registry.register(Arc::new(FollowUpQuestionTool));
    // Tool discovery — lets the agent activate extended tools on demand
    // (lazy-tools mode). Holds the registry Arc so it can search all tools;
    // harmless when lazy_tools is off (just one more always-available tool).
    tool_registry.register(Arc::new(
        crate::brain::tools::tool_search::ToolSearchTool::new(tool_registry.clone()),
    ));
    // Tools whose availability depends on config / keys (EXA, Brave, image
    // generation, vision/video). Shared with the config watcher so a key added
    // to keys.toml is picked up at runtime with no restart, in the daemon too.
    super::ui::register_config_dependent_tools(tool_registry, config);

    // Phase 5: Multi-agent orchestration
    let subagent_manager = Arc::new(crate::brain::tools::subagent::SubAgentManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::SpawnAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::WaitAgentTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::SendInputTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::CloseAgentTool::new(subagent_manager.clone()),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::ResumeAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));

    // Phase 6: Team orchestration
    let team_manager = Arc::new(crate::brain::tools::subagent::TeamManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamCreateTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamDeleteTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamBroadcastTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));
    tracing::info!("Registered 8 sub-agent + team orchestration tools");

    // Recursive Self-Improvement tools
    tool_registry.register(Arc::new(
        crate::brain::tools::feedback_record::FeedbackRecordTool,
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool,
    ));
    tool_registry.register(Arc::new(crate::brain::tools::self_improve::SelfImproveTool));
    tracing::info!("Registered 3 recursive self-improvement tools");
}
