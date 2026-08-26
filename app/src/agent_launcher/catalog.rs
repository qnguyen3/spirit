use crate::terminal::cli_agent::CLIAgent;
use crate::ui_components::icons::Icon;

pub struct AgentDefinition {
    pub cli_agent: CLIAgent,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub command: &'static str,
    pub icon: Icon,
    pub install_docs_url: &'static str,
}

pub fn agent_catalog() -> &'static [AgentDefinition] {
    &[
        AgentDefinition {
            cli_agent: CLIAgent::Claude,
            display_name: "Claude Code",
            binary: "claude",
            command: "claude --dangerously-skip-permissions",
            icon: Icon::ClaudeLogo,
            install_docs_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Codex,
            display_name: "Codex",
            binary: "codex",
            command: "codex --dangerously-bypass-approvals-and-sandbox -c model_reasoning_summary=\"auto\" -c model_supports_reasoning_summaries=true",
            icon: Icon::OpenAILogo,
            install_docs_url: "https://developers.openai.com/codex/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Amp,
            display_name: "Amp",
            binary: "amp",
            command: "amp",
            icon: Icon::AmpLogo,
            install_docs_url: "https://ampcode.com/manual#getting-started",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Gemini,
            display_name: "Gemini CLI",
            binary: "gemini",
            command: "gemini --yolo",
            icon: Icon::GeminiLogo,
            install_docs_url: "https://github.com/google-gemini/gemini-cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::CursorCli,
            display_name: "Cursor Agent",
            binary: "cursor-agent",
            command: "cursor-agent",
            icon: Icon::CursorLogo,
            install_docs_url: "https://cursor.com/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Pi,
            display_name: "Pi",
            binary: "pi",
            command: "pi",
            icon: Icon::PiLogo,
            install_docs_url: "https://github.com/badlogic/pi-mono",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Copilot,
            display_name: "Copilot CLI",
            binary: "copilot",
            command: "copilot --allow-all",
            icon: Icon::CopilotLogo,
            install_docs_url: "https://github.com/github/copilot-cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::OpenCode,
            display_name: "OpenCode",
            binary: "opencode",
            command: "opencode",
            icon: Icon::OpenCodeLogo,
            install_docs_url: "https://opencode.ai/docs",
        },
    ]
}

pub fn is_installed(def: &AgentDefinition) -> bool {
    #[cfg(not(target_family = "wasm"))]
    {
        warp_util::path::resolve_executable(def.binary).is_some()
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = def;
        false
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
