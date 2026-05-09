use serde::Serialize;
use std::io::{self, IsTerminal};
use std::sync::LazyLock;

/// Default wrap width for HTML-to-text conversion. 76 cols leaves slack for
/// indentation in comment threads while staying readable in 80-col terminals.
pub const WRAP_WIDTH: usize = 76;

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
    println!(
        "{}",
        serde_json::to_string_pretty(data).expect("serialization failed")
    );
}

/// Convert HTML to plain text and strip the U+0336 combining-overlay characters
/// that `html2text` emits for `<s>`/`<del>` strikethrough. The overlay multiplies
/// byte-count without adding information; for agent consumers it's pure token waste.
pub fn html_to_text(html: &[u8], width: usize) -> String {
    html2text::from_read(html, width)
        .unwrap_or_default()
        .chars()
        .filter(|&c| c != '\u{336}')
        .collect()
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Json,
    Tsv,
    Ids,
}
