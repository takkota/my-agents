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
    60
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            default_agent_cli: default_agent_cli(),
            tick_rate_ms: default_tick_rate(),
            monitor_interval_secs: default_monitor_interval(),
            pr_monitor_interval_secs: default_pr_monitor_interval(),
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
        // Always save to the canonical location (same as load)
        let data_dir = default_data_dir();
        let config_path = data_dir.join("config.toml");
        fs::create_dir_all(&data_dir)?;
        let content = toml::to_string_pretty(self)?;
        fs::write(config_path, content)?;
        Ok(())
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }
}
