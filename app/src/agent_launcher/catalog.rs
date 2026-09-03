use std::borrow::Cow;

use crate::settings::AgentApprovalMode;
use crate::terminal::cli_agent::{
    CLIAgent, CLINE_ICON_PATH, DEVIN_ICON_PATH, HERMES_ICON_PATH, MISTRAL_VIBE_ICON_PATH,
    QWEN_CODE_ICON_PATH, TRAE_ICON_PATH,
};
use crate::ui_components::icons::Icon;

pub enum AgentIcon {
    Glyph(Icon),
    Image(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentLaunchRequest {
    pub catalog_index: usize,
    pub approval_mode: AgentApprovalMode,
}

pub struct AgentDefinition {
    pub cli_agent: CLIAgent,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub command: &'static str,
    pub yolo_args: Option<&'static str>,
    pub icon: AgentIcon,
    pub install_docs_url: &'static str,
}

impl AgentDefinition {
    pub fn launch_command(&self, mode: AgentApprovalMode) -> Cow<'static, str> {
        match (mode, self.yolo_args) {
            (AgentApprovalMode::Yolo, Some(args)) => Cow::Owned(format!("{} {args}", self.command)),
            (AgentApprovalMode::Yolo, None)
            | (AgentApprovalMode::Normal, Some(_))
            | (AgentApprovalMode::Normal, None) => Cow::Borrowed(self.command),
        }
    }
}

pub fn agent_catalog() -> &'static [AgentDefinition] {
    &[
        AgentDefinition {
            cli_agent: CLIAgent::Claude,
            display_name: "Claude Code",
            binary: "claude",
            command: "claude",
            yolo_args: Some("--dangerously-skip-permissions"),
            icon: AgentIcon::Glyph(Icon::ClaudeLogo),
            install_docs_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Codex,
            display_name: "Codex",
            binary: "codex",
            command: "codex -c model_reasoning_summary=\"auto\" -c model_supports_reasoning_summaries=true",
            yolo_args: Some("--dangerously-bypass-approvals-and-sandbox"),
            icon: AgentIcon::Glyph(Icon::OpenAILogo),
            install_docs_url: "https://developers.openai.com/codex/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Amp,
            display_name: "Amp",
            binary: "amp",
            command: "amp",
            yolo_args: Some("--dangerously-allow-all"),
            icon: AgentIcon::Glyph(Icon::AmpLogo),
            install_docs_url: "https://ampcode.com/manual#getting-started",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Gemini,
            display_name: "Gemini CLI",
            binary: "gemini",
            command: "gemini",
            yolo_args: Some("--yolo"),
            icon: AgentIcon::Glyph(Icon::GeminiLogo),
            install_docs_url: "https://github.com/google-gemini/gemini-cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::CursorCli,
            display_name: "Cursor Agent",
            binary: "cursor-agent",
            command: "cursor-agent",
            yolo_args: Some("--yolo"),
            icon: AgentIcon::Glyph(Icon::CursorLogo),
            install_docs_url: "https://cursor.com/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Pi,
            display_name: "Pi",
            binary: "pi",
            command: "pi",
            yolo_args: None,
            icon: AgentIcon::Glyph(Icon::PiLogo),
            install_docs_url: "https://github.com/badlogic/pi-mono",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Copilot,
            display_name: "Copilot CLI",
            binary: "copilot",
            command: "copilot",
            yolo_args: Some("--allow-all"),
            icon: AgentIcon::Glyph(Icon::CopilotLogo),
            install_docs_url: "https://github.com/github/copilot-cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::OpenCode,
            display_name: "OpenCode",
            binary: "opencode",
            command: "opencode",
            yolo_args: None,
            icon: AgentIcon::Glyph(Icon::OpenCodeLogo),
            install_docs_url: "https://opencode.ai/docs",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Grok,
            display_name: "Grok",
            binary: "grok",
            command: "grok",
            yolo_args: Some("--permission-mode bypassPermissions"),
            icon: AgentIcon::Glyph(Icon::GrokLogo),
            install_docs_url: "https://x.ai/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::OhMyPi,
            display_name: "OMP",
            binary: "omp",
            command: "omp",
            yolo_args: None,
            icon: AgentIcon::Glyph(Icon::OhMyPiLogo),
            install_docs_url: "https://omp.sh",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Trae,
            display_name: "Trae",
            binary: "traecli",
            command: "traecli",
            yolo_args: Some("--yolo"),
            icon: AgentIcon::Image(TRAE_ICON_PATH),
            install_docs_url: "https://docs.trae.cn/cli_get-started-with-trae-cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Antigravity,
            display_name: "Antigravity",
            binary: "agy",
            command: "agy",
            yolo_args: Some("--dangerously-skip-permissions"),
            icon: AgentIcon::Glyph(Icon::AntigravityLogo),
            install_docs_url: "https://antigravity.google/docs/cli-overview",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Cline,
            display_name: "Cline",
            binary: "cline",
            command: "cline",
            yolo_args: Some("--auto-approve true"),
            icon: AgentIcon::Image(CLINE_ICON_PATH),
            install_docs_url: "https://docs.cline.bot/cline-cli/overview",
        },
        AgentDefinition {
            cli_agent: CLIAgent::QwenCode,
            display_name: "Qwen Code",
            binary: "qwen",
            command: "qwen",
            yolo_args: Some("--approval-mode yolo"),
            icon: AgentIcon::Image(QWEN_CODE_ICON_PATH),
            install_docs_url: "https://github.com/QwenLM/qwen-code",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Vibe,
            display_name: "Mistral Vibe",
            binary: "vibe",
            command: "vibe",
            yolo_args: Some("--agent auto-approve"),
            icon: AgentIcon::Image(MISTRAL_VIBE_ICON_PATH),
            install_docs_url: "https://github.com/mistralai/mistral-vibe",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Devin,
            display_name: "Devin",
            binary: "devin",
            command: "devin",
            yolo_args: Some("--permission-mode bypass"),
            icon: AgentIcon::Image(DEVIN_ICON_PATH),
            install_docs_url: "https://devin.ai/cli",
        },
        AgentDefinition {
            cli_agent: CLIAgent::Hermes,
            display_name: "Hermes",
            binary: "hermes",
            command: "hermes",
            yolo_args: Some("--yolo"),
            icon: AgentIcon::Image(HERMES_ICON_PATH),
            install_docs_url: "https://hermes-agent.nousresearch.com/docs/",
        },
    ]
}

/// A bundled app launched from Finder inherits a minimal PATH, so `path_env` (captured from the
/// user's shell) must be supplied for detection to see most agent installs.
pub fn is_installed(def: &AgentDefinition, path_env: Option<&str>) -> bool {
    #[cfg(not(target_family = "wasm"))]
    {
        match path_env {
            Some(path) => {
                warp_util::path::resolve_executable_in_path(def.binary, std::ffi::OsStr::new(path))
                    .is_some()
            }
            None => warp_util::path::resolve_executable(def.binary).is_some(),
        }
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = (def, path_env);
        false
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
