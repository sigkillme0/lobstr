use serde::Serialize;
use std::io::{self, IsTerminal};
use std::sync::LazyLock;

pub static USE_COLOR: LazyLock<bool> = LazyLock::new(|| {
    if let Some(color) = crate::config::CONFIG.color {
        return color;
    }
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
});

macro_rules! style {
    ($text:expr, $method:ident) => {
        if *$crate::util::USE_COLOR {
            $text.$method()
        } else {
            $text.normal()
        }
    };
    ($text:expr, $method:ident, $method2:ident) => {
        if *$crate::util::USE_COLOR {
            $text.$method().$method2()
        } else {
            $text.normal()
        }
    };
}
pub(crate) use style;

pub fn extract_domain(url: &str) -> &str {
    if url.is_empty() {
        return "self";
    }
    let s = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    s.strip_prefix("www.")
        .unwrap_or(s)
        .split(['/', ':', '?'])
        .next()
        .unwrap_or("self")
}

pub fn print_json<T: Serialize + ?Sized>(data: &T) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        println!("{json}");
    }
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Json,
    Tsv,
    Ids,
}
