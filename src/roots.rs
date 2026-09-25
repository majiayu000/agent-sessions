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
        let home = std::env::home_dir();
        fn root(key: &str, home: Option<&Path>, suffix: &str) -> io::Result<Option<PathBuf>> {
            match std::env::var_os(key) {
                Some(v) if v.is_empty() => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{key} is empty"),
                )),
                Some(v) => Ok(Some(PathBuf::from(v))),
                None => Ok(home.map(|h| h.join(suffix))),
            }
        }
        Ok(Self {
            claude: root("CLAUDE_CONFIG_DIR", home.as_deref(), ".claude")?,
            codex: root("CODEX_HOME", home.as_deref(), ".codex")?,
        })
    }
}
