use crate::Agent;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
}
impl Roots {
    pub fn from_home(home: &Path) -> Self {
        Self {
            claude: Some(home.join(".claude")),
            codex: Some(home.join(".codex")),
        }
    }
    pub fn from_env() -> io::Result<Self> {
        Ok(Self {
            claude: Self::from_env_for(Agent::ClaudeCode)?.claude,
            codex: Self::from_env_for(Agent::Codex)?.codex,
        })
    }
    /// Resolve only the selected host so unrelated environment errors cannot
    /// disable a source-specific consumer.
    pub fn from_env_for(agent: Agent) -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| !cfg!(windows) || p.is_absolute())
            .or_else(std::env::home_dir);
        let (key, suffix) = match agent {
            Agent::ClaudeCode => ("CLAUDE_CONFIG_DIR", ".claude"),
            Agent::Codex => ("CODEX_HOME", ".codex"),
        };
        let path = match std::env::var_os(key) {
            Some(v) if v.is_empty() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{key} is empty"),
                ));
            }
            Some(v) => Some(PathBuf::from(v)),
            None => home.map(|h| h.join(suffix)),
        };
        Ok(match agent {
            Agent::ClaudeCode => Self {
                claude: path,
                codex: None,
            },
            Agent::Codex => Self {
                claude: None,
                codex: path,
            },
        })
    }
}
