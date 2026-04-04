use serde::Deserialize;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const DEFAULT_LIMIT: usize = 25;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_limit: Option<usize>,
    pub color: Option<bool>,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let Some(path) = config_path() else {
        return Config::default();
    };
    if !path.exists() {
        return Config::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                Config::default()
            }
        },
        Err(e) => {
            eprintln!("warning: failed to read {}: {e}", path.display());
            Config::default()
        }
    }
});

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("lobstr").join("config.toml"))
}
