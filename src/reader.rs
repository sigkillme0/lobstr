use crate::api::StoryDetail;
use crate::util::{OutputFormat, extract_domain, print_json, style};
use colored::Colorize;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::sync::LazyLock;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReadError {
    #[error("unsupported content type: {0}")]
    UnsupportedType(String),
    #[error("failed to fetch: {0}")]
    Fetch(#[from] reqwest::Error),
    #[error("failed to extract content")]
    Extract,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ReadError>;

pub struct ReadOpts {
    pub width: usize,
    pub full: bool,
    pub raw: bool,
    pub format: OutputFormat,
}

#[derive(Serialize)]
struct ArticleOutput<'a> {
    title: &'a str,
    url: &'a str,
    source: &'a str,
    content: String,
}

const TIMEOUT: Duration = Duration::from_secs(20);

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .timeout(TIMEOUT)
        .gzip(true)
        .brotli(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("failed to build http client")
});

fn default_max_lines() -> usize {
    if !io::stdout().is_terminal() {
        return usize::MAX;
    }
    crossterm::terminal::size()
        .map(|(_, rows)| (rows as usize).saturating_sub(10))
        .unwrap_or(200)
}

fn is_youtube_url(url: &str) -> Option<String> {
    let domain = extract_domain(url).to_lowercase();
    if !domain.contains("youtube.com") && !domain.contains("youtu.be") {
        return None;
    }
    if let Some(id) = url
        .strip_prefix("https://youtu.be/")
        .or_else(|| url.strip_prefix("http://youtu.be/"))
    {
        return Some(id.split(['?', '&', '/']).next().unwrap_or(id).to_string());
    }
    if url.contains("youtube.com") {
        if let Some(pos) = url.find("v=") {
            let after_v = &url[pos + 2..];
            return Some(
                after_v
                    .split(['&', '/', '?'])
                    .next()
                    .unwrap_or(after_v)
                    .to_string(),
            );
        }
    }
    None
}

fn is_unsupported_url(url: &str) -> Option<&'static str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    match std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("pdf") => Some("pdf"),
        Some("mp4" | "webm" | "mov") => Some("video file"),
        Some("mp3" | "wav" | "ogg") => Some("audio file"),
        Some("zip" | "tgz") => Some("archive"),
        Some("gz") if lower.ends_with(".tar.gz") => Some("archive"),
        _ => None,
    }
}

fn is_github_repo(url: &str) -> bool {
    let dominated = extract_domain(url).to_lowercase();
    if !dominated.contains("github.com") {
        return false;
    }
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .unwrap_or("");
    let parts: Vec<_> = path.split('/').filter(|s| !s.is_empty()).collect();
    parts.len() == 2
        || (parts.len() > 2
            && ![
                "blob", "tree", "issues", "pull", "releases", "actions", "wiki",
            ]
            .contains(&parts[2]))
}

async fn fetch_github_readme(url: &str) -> Result<String> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .ok_or_else(|| ReadError::Other("invalid github url".into()))?;

    let parts: Vec<_> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return Err(ReadError::Other("invalid github repo path".into()));
    }

    let (owner, repo) = (parts[0], parts[1]);

    for readme in [
        "README.md",
        "readme.md",
        "README",
        "README.rst",
        "README.txt",
    ] {
        let raw_url = format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/{readme}");
        if let Ok(resp) = CLIENT.get(&raw_url).send().await {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    return Ok(text);
                }
            }
        }
    }

    Err(ReadError::Other("could not find readme".into()))
}

async fn fetch_youtube_transcript(video_id: &str) -> Result<String> {
    use yt_transcript_rs::YouTubeTranscriptApi;

    let api = YouTubeTranscriptApi::new(None, None, None)
        .map_err(|e| ReadError::Other(format!("youtube api init: {e}")))?;

    let transcript = api
        .fetch_transcript(video_id, &["en", "en-US", "en-GB"], false)
        .await
        .map_err(|e| ReadError::Other(format!("transcript fetch: {e}")))?;

    Ok(transcript.text())
}

fn extract_with_llm_readability(html: &str, url_str: &str) -> Result<(Option<String>, String)> {
    let url = url::Url::parse(url_str).map_err(|_| ReadError::Extract)?;
    match llm_readability::extractor::extract(&mut html.as_bytes(), &url) {
        Ok(product) => {
            let content = if product.text.trim().is_empty() {
                html2text::from_read(product.content.as_bytes(), 76).unwrap_or_default()
            } else {
                product.text
            };
            if content.trim().is_empty() {
                Err(ReadError::Extract)
            } else {
                let title = extract_title_from_html(html);
                Ok((title, content))
            }
        }
        Err(_) => Err(ReadError::Extract),
    }
}

fn extract_title_from_html(html: &str) -> Option<String> {
    use scraper::{Html, Selector};
    let doc = Html::parse_document(html);
    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = doc.select(&sel).next() {
            let title = el.text().collect::<String>().trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    if let Ok(sel) = Selector::parse("h1") {
        if let Some(el) = doc.select(&sel).next() {
            let title = el.text().collect::<String>().trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

fn extract_with_selectors(html: &str, width: usize) -> Result<String> {
    use scraper::{Html, Selector};

    const SELECTORS: &[&str] = &[
        "article",
        "main article",
        "[role='main']",
        ".post-content",
        ".article-content",
        ".entry-content",
        ".content",
        ".post-body",
        ".article-body",
        ".story-body",
        "main",
        "#content",
        ".markdown-body",
    ];

    let doc = Html::parse_document(html);

    for sel_str in SELECTORS {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = doc.select(&sel).next() {
                let inner = el.html();
                let text = html2text::from_read(inner.as_bytes(), width).unwrap_or_default();
                let cleaned = clean_text(&text);
                if cleaned.split_whitespace().count() > 50 {
                    return Ok(cleaned);
                }
            }
        }
    }

    if let Ok(sel) = Selector::parse("body") {
        if let Some(el) = doc.select(&sel).next() {
            let inner = el.html();
            let text = html2text::from_read(inner.as_bytes(), width).unwrap_or_default();
            let cleaned = clean_text(&text);
            if cleaned.split_whitespace().count() > 30 {
                return Ok(cleaned);
            }
        }
    }

    Err(ReadError::Extract)
}

fn clean_text(text: &str) -> String {
    let lines: Vec<_> = text
        .lines()
        .map(str::trim_end)
        .fold(Vec::new(), |mut acc, line| {
            let is_blank = line.trim().is_empty();
            let last_blank = acc.last().is_some_and(|l: &&str| l.trim().is_empty());
            if !(is_blank && last_blank) {
                acc.push(line);
            }
            acc
        });

    lines.join("\n").trim().to_string()
}

fn render_markdown(text: &str, width: usize) -> String {
    use termimad::MadSkin;
    let skin = MadSkin::default();
    let area_width = width.min(120);
    let fmt = termimad::FmtText::from(&skin, text, Some(area_width));
    fmt.to_string()
}

fn wrap_text(text: &str, width: usize) -> String {
    text.lines()
        .flat_map(|line| {
            if line.len() <= width {
                vec![line.to_string()]
            } else {
                let mut result = Vec::new();
                let mut current = String::new();
                for word in line.split_whitespace() {
                    if current.is_empty() {
                        current = word.to_string();
                    } else if current.len() + 1 + word.len() <= width {
                        current.push(' ');
                        current.push_str(word);
                    } else {
                        result.push(current);
                        current = word.to_string();
                    }
                }
                if !current.is_empty() {
                    result.push(current);
                }
                if result.is_empty() {
                    vec![String::new()]
                } else {
                    result
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_lines, clippy::future_not_send)]
pub async fn read_article(story: &StoryDetail, opts: &ReadOpts) -> Result<()> {
    let json = matches!(opts.format, OutputFormat::Json);
    let mut out = io::stdout().lock();

    if story.url.is_empty() {
        if json {
            print_article_json(&story.title, "", "self", &story.description_plain);
            return Ok(());
        }

        print_header(&mut out, &story.title, "self post", &story.tags);
        if story.description_plain.is_empty() {
            writeln!(out, "{}", style!("(no content - discussion post)", dimmed)).ok();
        } else {
            let text = wrap_text(&story.description_plain, opts.width);
            writeln!(out, "{text}").ok();
        }
        print_footer(&mut out, story);
        return Ok(());
    }

    if let Some(content_type) = is_unsupported_url(&story.url) {
        if json {
            print_article_json(
                &story.title,
                &story.url,
                extract_domain(&story.url),
                &format!("[unsupported: {content_type}]"),
            );
            return Ok(());
        }

        print_header(
            &mut out,
            &story.title,
            extract_domain(&story.url),
            &story.tags,
        );
        writeln!(
            out,
            "{}",
            style!(
                format!("unsupported content type: {content_type}").as_str(),
                red
            )
        )
        .ok();
        writeln!(out).ok();
        writeln!(out, "url: {}", style!(story.url.as_str(), blue)).ok();
        print_footer(&mut out, story);
        return Err(ReadError::UnsupportedType(content_type.to_string()));
    }

    let source = extract_domain(&story.url);

    if let Some(video_id) = is_youtube_url(&story.url) {
        match fetch_youtube_transcript(&video_id).await {
            Ok(transcript) => {
                if json {
                    print_article_json(&story.title, &story.url, "youtube", &transcript);
                    return Ok(());
                }

                print_header(&mut out, &story.title, "youtube [transcript]", &story.tags);
                let content = wrap_text(&transcript, opts.width);
                print_content(&mut out, &content, opts.full);
                print_footer(&mut out, story);
                return Ok(());
            }
            Err(e) => {
                if !json {
                    writeln!(
                        out,
                        "{}",
                        style!(format!("transcript unavailable: {e}").as_str(), yellow)
                    )
                    .ok();
                }
                print_header(&mut out, &story.title, "youtube", &story.tags);
                writeln!(
                    out,
                    "{}",
                    style!("no transcript available for this video", dimmed)
                )
                .ok();
                writeln!(out).ok();
                writeln!(out, "url: {}", style!(story.url.as_str(), blue)).ok();
                print_footer(&mut out, story);
                return Err(e);
            }
        }
    }

    if is_github_repo(&story.url) {
        match fetch_github_readme(&story.url).await {
            Ok(readme) => {
                if json {
                    print_article_json(&story.title, &story.url, "github", &readme);
                    return Ok(());
                }

                print_header(&mut out, &story.title, "github", &story.tags);
                let content = if opts.raw {
                    readme
                } else {
                    render_markdown(&readme, opts.width)
                };
                print_content(&mut out, &content, opts.full);
                print_footer(&mut out, story);
                return Ok(());
            }
            Err(e) => {
                if !json {
                    writeln!(
                        out,
                        "{}",
                        style!(format!("github readme: {e}").as_str(), yellow)
                    )
                    .ok();
                }
            }
        }
    }

    let resp = CLIENT.get(&story.url).send().await?;

    if !resp.status().is_success() {
        return Err(ReadError::Other(format!("http {}", resp.status())));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
        return Err(ReadError::UnsupportedType(content_type.to_string()));
    }

    let html = resp.text().await?;

    let (title, content) = match extract_with_llm_readability(&html, &story.url) {
        Ok((t, c)) => (t, c),
        Err(_) => match extract_with_selectors(&html, opts.width) {
            Ok(c) => (None, c),
            Err(e) => return Err(e),
        },
    };

    let display_title = title.as_deref().unwrap_or(&story.title);

    if json {
        print_article_json(display_title, &story.url, source, &content);
        return Ok(());
    }

    print_header(&mut out, display_title, source, &story.tags);

    let final_content = if opts.raw {
        content
    } else {
        wrap_text(&content, opts.width)
    };

    print_content(&mut out, &final_content, opts.full);
    print_footer(&mut out, story);

    Ok(())
}

fn print_article_json(title: &str, url: &str, source: &str, content: &str) {
    print_json(&ArticleOutput {
        title,
        url,
        source,
        content: content.to_string(),
    });
}

fn print_header<W: Write>(out: &mut W, title: &str, source: &str, tags: &[String]) {
    let sep = "─".repeat(title.chars().count().min(78));

    writeln!(out).ok();
    writeln!(out, "{}", style!(title, bold)).ok();
    writeln!(out, "{}", style!(sep.as_str(), dimmed)).ok();
    writeln!(
        out,
        "{} │ {}",
        style!(source, blue),
        style!(tags.join(", ").as_str(), cyan)
    )
    .ok();
    writeln!(out).ok();
}

fn print_content<W: Write>(out: &mut W, content: &str, full: bool) {
    let lines: Vec<_> = content.lines().collect();
    let max = if full {
        usize::MAX
    } else {
        default_max_lines()
    };

    for line in lines.iter().take(max) {
        writeln!(out, "{line}").ok();
    }

    if lines.len() > max {
        writeln!(out).ok();
        writeln!(
            out,
            "{}",
            style!(
                format!(
                    "[... {} more lines, use --full to show all]",
                    lines.len() - max
                )
                .as_str(),
                dimmed
            )
        )
        .ok();
    }
}

fn print_footer<W: Write>(out: &mut W, story: &StoryDetail) {
    writeln!(out).ok();
    writeln!(out, "{}", style!("─".repeat(40).as_str(), dimmed)).ok();
    writeln!(
        out,
        "{} pts │ {} comments │ {}",
        style!(story.score.to_string().as_str(), green),
        story.comment_count,
        style!(format!("lobste.rs/s/{}", story.short_id).as_str(), dimmed)
    )
    .ok();
    if !story.url.is_empty() {
        writeln!(out, "{}", style!(story.url.as_str(), blue)).ok();
    }
}
