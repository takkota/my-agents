use crate::components::task_tree::SortMode;
use crate::domain::task::AgentCli;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_agent_cli")]
    pub default_agent_cli: AgentCli,
    #[serde(default = "default_tick_rate")]
    pub tick_rate_ms: u64,
    #[serde(default = "default_monitor_interval")]
    pub monitor_interval_secs: u64,
    #[serde(default = "default_pr_monitor_interval")]
    pub pr_monitor_interval_secs: u64,
    #[serde(default)]
    pub default_sort_mode: SortMode,
    #[serde(default = "default_pr_prompt")]
    pub pr_prompt: String,
    #[serde(default = "default_review_prompt")]
    pub review_prompt: String,
    #[serde(default)]
    pub custom_prompts: Vec<String>,
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".my-agents")
}

fn default_agent_cli() -> AgentCli {
    AgentCli::Claude
}

fn default_tick_rate() -> u64 {
    250
}

fn default_monitor_interval() -> u64 {
    10
}

fn default_pr_monitor_interval() -> u64 {
    30
}

fn default_pr_prompt() -> String {
    "If any code changes were made during this task, you MUST create a Pull Request before marking the task as completed.".to_string()
}

fn default_review_prompt() -> String {
    "このタスクの変更内容について設計・実装面のレビューを行い、フィードバックがあれば対応してください。".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            default_agent_cli: default_agent_cli(),
            tick_rate_ms: default_tick_rate(),
            monitor_interval_secs: default_monitor_interval(),
            pr_monitor_interval_secs: default_pr_monitor_interval(),
            default_sort_mode: SortMode::default(),
            pr_prompt: default_pr_prompt(),
            review_prompt: default_review_prompt(),
            custom_prompts: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> AppResult<Self> {
        let data_dir = default_data_dir();
        let config_path = data_dir.join("config.toml");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> AppResult<()> {
        let config_path = default_data_dir().join("config.toml");
        let content = toml::to_string_pretty(self)?;
        fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }
}
