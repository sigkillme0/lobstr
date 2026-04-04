use serde::Deserialize;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_limit: Option<usize>,
    pub color: Option<bool>,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
});

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("lobstr").join("config.toml"))
}
