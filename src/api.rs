use chrono::{DateTime, FixedOffset};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;

#[derive(Debug, Clone, Copy, Default)]
pub enum TagMode {
    #[default]
    All,
    Any,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SearchWhat {
    #[default]
    Stories,
    Comments,
}

impl std::fmt::Display for SearchWhat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stories => write!(f, "stories"),
            Self::Comments => write!(f, "comments"),
        }
    }
}

impl std::str::FromStr for SearchWhat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stories" | "story" | "s" => Ok(Self::Stories),
            "comments" | "comment" | "c" => Ok(Self::Comments),
            _ => Err(format!("unknown search type: {s} (try: stories, comments)")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SearchOrder {
    #[default]
    Relevance,
    Newest,
    Score,
}

impl std::fmt::Display for SearchOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relevance => write!(f, "relevance"),
            Self::Newest => write!(f, "newest"),
            Self::Score => write!(f, "score"),
        }
    }
}

impl std::str::FromStr for SearchOrder {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "relevance" | "rel" | "r" => Ok(Self::Relevance),
            "newest" | "new" | "n" => Ok(Self::Newest),
            "score" | "top" | "s" => Ok(Self::Score),
            _ => Err(format!(
                "unknown order: {s} (try: relevance, newest, score)"
            )),
        }
    }
}

fn valid_username(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_csv(s: &str) -> Vec<&str> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn tags_match(haystack: &[String], needle: &str) -> bool {
    haystack.iter().any(|t| t.eq_ignore_ascii_case(needle))
}

fn valid_story_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

const BASE: &str = "https://lobste.rs";
const UA: &str = concat!("lobstr/", env!("CARGO_PKG_VERSION"));
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RETRIES: u32 = 3;
const RETRY_BASE_MS: u64 = 500;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(TIMEOUT)
        .gzip(true)
        .brotli(true)
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(2)
        .build()
        .expect("failed to build http client")
});

#[derive(Error, Debug)]
pub enum Error {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("'{0}' not found")]
    NotFound(String),
    #[error("rate limited, try again later")]
    RateLimited,
    #[error("failed to parse html")]
    ParseHtml,
}

pub type Result<T> = std::result::Result<T, Error>;

async fn get<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let text = get_text(path).await?;
    serde_json::from_str(&text).map_err(|_| Error::ParseHtml)
}

async fn get_text(path: &str) -> Result<String> {
    let url = format!("{BASE}{path}");
    let mut last_err = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            sleep(Duration::from_millis(RETRY_BASE_MS * (1 << attempt))).await;
        }

        match CLIENT.get(&url).send().await {
            Ok(resp) => {
                return match resp.status() {
                    s if s == reqwest::StatusCode::NOT_FOUND => {
                        Err(Error::NotFound(path.to_string()))
                    }
                    s if s == reqwest::StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
                    s if s.is_server_error() && attempt < MAX_RETRIES - 1 => {
                        last_err = Some(Error::Http(resp.error_for_status().unwrap_err()));
                        continue;
                    }
                    _ => resp.error_for_status()?.text().await.map_err(Error::from),
                };
            }
            Err(e) if e.is_timeout() || e.is_connect() => {
                last_err = Some(Error::Http(e));
                continue;
            }
            Err(e) => return Err(Error::Http(e)),
        }
    }

    Err(last_err.unwrap_or_else(|| Error::NotFound(path.to_string())))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Story {
    pub short_id: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    pub score: i32,
    pub comment_count: u32,
    pub created_at: String,
    pub submitter_user: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoryDetail {
    pub short_id: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    pub score: i32,
    pub comment_count: u32,
    pub created_at: String,
    pub submitter_user: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub description_plain: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub created_at: String,
    pub score: i32,
    pub depth: u32,
    pub comment: String,
    pub commenting_user: String,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default)]
    pub is_moderated: bool,
}

impl Comment {
    #[inline]
    pub fn is_visible(&self) -> bool {
        !self.is_deleted && !self.is_moderated
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum CommentSort {
    #[default]
    Default,
    Score,
    ScoreAsc,
    Newest,
    Oldest,
}

impl std::str::FromStr for CommentSort {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" | "thread" => Ok(Self::Default),
            "score" | "top" => Ok(Self::Score),
            "score-asc" | "bottom" => Ok(Self::ScoreAsc),
            "newest" | "new" => Ok(Self::Newest),
            "oldest" | "old" => Ok(Self::Oldest),
            _ => Err(format!("unknown sort: {s} (try: score, newest, oldest)")),
        }
    }
}

pub fn sort_comments(comments: &mut [Comment], sort: CommentSort) {
    match sort {
        CommentSort::Default => {}
        CommentSort::Score => {
            stable_sort_preserving_threads(comments, |a, b| b.score.cmp(&a.score));
        }
        CommentSort::ScoreAsc => {
            stable_sort_preserving_threads(comments, |a, b| a.score.cmp(&b.score));
        }
        CommentSort::Newest => {
            stable_sort_preserving_threads(comments, |a, b| b.created_at.cmp(&a.created_at));
        }
        CommentSort::Oldest => {
            stable_sort_preserving_threads(comments, |a, b| a.created_at.cmp(&b.created_at));
        }
    }
}

fn stable_sort_preserving_threads<F>(comments: &mut [Comment], cmp: F)
where
    F: Fn(&Comment, &Comment) -> Ordering,
{
    let mut top_level: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < comments.len() {
        if comments[i].depth == 0 {
            let start = i;
            i += 1;
            while i < comments.len() && comments[i].depth > 0 {
                i += 1;
            }
            top_level.push((start, i));
        } else {
            i += 1;
        }
    }

    top_level.sort_by(|a, b| cmp(&comments[a.0], &comments[b.0]));

    let old_comments: Vec<Comment> = comments.to_vec();
    let mut write_idx = 0;
    for (start, end) in top_level {
        for c in old_comments.iter().take(end).skip(start) {
            comments[write_idx] = c.clone();
            write_idx += 1;
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub created_at: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub is_moderator: bool,
    #[serde(default)]
    pub about: String,
    pub invited_by_user: Option<String>,
    pub github_username: Option<String>,
    pub mastodon_username: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct UserStats {
    pub stories_count: Option<u32>,
    pub comments_count: Option<u32>,
    pub most_common_tag: Option<String>,
    pub story_karma: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {
    pub tag: String,
    pub description: String,
    #[serde(default)]
    pub is_media: bool,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub category: String,
}

pub struct ListOpts {
    pub limit: usize,
    pub page: u32,
    pub min_score: Option<i32>,
    pub after: Option<DateTime<FixedOffset>>,
    pub before: Option<DateTime<FixedOffset>>,
}

impl Default for ListOpts {
    fn default() -> Self {
        Self {
            limit: 25,
            page: 1,
            min_score: None,
            after: None,
            before: None,
        }
    }
}

fn parse_time(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).ok()
}

fn in_date_range(created: &str, opts: &ListOpts) -> bool {
    let Some(dt) = parse_time(created) else {
        return true;
    };
    if let Some(ref after) = opts.after {
        if dt < *after {
            return false;
        }
    }
    if let Some(ref before) = opts.before {
        if dt > *before {
            return false;
        }
    }
    true
}

fn is_before_range(created: &str, opts: &ListOpts) -> bool {
    if let Some(ref after) = opts.after {
        if let Some(dt) = parse_time(created) {
            return dt < *after;
        }
    }
    false
}

fn filter_stories(stories: Vec<Story>, opts: &ListOpts) -> Vec<Story> {
    let min = opts.min_score;
    stories
        .into_iter()
        .filter(|s| min.is_none_or(|m| s.score >= m))
        .filter(|s| in_date_range(&s.created_at, opts))
        .take(opts.limit)
        .collect()
}

const MAX_PAGES_FETCH: u32 = 10;

async fn fetch_stories<F>(opts: &ListOpts, make_path: F) -> Result<Vec<Story>>
where
    F: Fn(u32) -> String,
{
    if opts.after.is_some() || opts.before.is_some() {
        return fetch_pages_filtered(opts, &make_path).await;
    }
    get::<Vec<Story>>(&make_path(opts.page))
        .await
        .map(|v| filter_stories(v, opts))
}

pub async fn hottest(opts: &ListOpts) -> Result<Vec<Story>> {
    fetch_stories(opts, |p| {
        if p == 1 {
            "/hottest.json".into()
        } else {
            format!("/page/{p}.json")
        }
    })
    .await
}

pub async fn newest(opts: &ListOpts) -> Result<Vec<Story>> {
    fetch_stories(opts, |p| format!("/newest/page/{p}.json")).await
}

pub async fn active(opts: &ListOpts) -> Result<Vec<Story>> {
    get::<Vec<Story>>("/active.json")
        .await
        .map(|v| filter_stories(v, opts))
}

async fn fetch_pages_filtered(
    opts: &ListOpts,
    make_path: &impl Fn(u32) -> String,
) -> Result<Vec<Story>> {
    let mut all = Vec::new();
    let mut page = opts.page;

    for _ in 0..MAX_PAGES_FETCH {
        let stories: Vec<Story> = get(&make_path(page)).await?;
        if stories.is_empty() {
            break;
        }

        let mut should_stop = false;
        for s in stories {
            if is_before_range(&s.created_at, opts) {
                should_stop = true;
                break;
            }
            if in_date_range(&s.created_at, opts) && opts.min_score.is_none_or(|m| s.score >= m) {
                all.push(s);
            }
            if all.len() >= opts.limit {
                break;
            }
        }

        if should_stop || all.len() >= opts.limit {
            break;
        }
        page += 1;
    }

    all.truncate(opts.limit);
    Ok(all)
}

pub async fn by_tag(
    tags_input: &str,
    opts: &ListOpts,
    mode: TagMode,
    exclude: Option<&str>,
) -> Result<Vec<Story>> {
    let tags = parse_csv(tags_input);
    if tags.is_empty() {
        return Err(Error::NotFound("no tags specified".into()));
    }

    let exclude_tags = exclude.map(parse_csv).unwrap_or_default();
    let primary = tags[0];
    let path = match opts.page {
        1 => format!("/t/{primary}.json"),
        p => format!("/t/{primary}/page/{p}.json"),
    };

    get::<Vec<Story>>(&path).await.map(|stories| {
        let filtered = stories
            .into_iter()
            .filter(|s| {
                if exclude_tags.iter().any(|t| tags_match(&s.tags, t)) {
                    return false;
                }
                if tags.len() == 1 {
                    return true;
                }
                let check = |t: &&str| tags_match(&s.tags, t);
                match mode {
                    TagMode::All => tags.iter().all(check),
                    TagMode::Any => tags.iter().any(check),
                }
            })
            .collect();
        filter_stories(filtered, opts)
    })
}

pub async fn story(id: &str) -> Result<StoryDetail> {
    if !valid_story_id(id) {
        return Err(Error::NotFound(format!("invalid story id: {id}")));
    }
    get(&format!("/s/{id}.json")).await
}

pub async fn user(name: &str) -> Result<User> {
    if !valid_username(name) {
        return Err(Error::NotFound(format!("invalid username: {name}")));
    }
    get(&format!("/~{name}.json")).await
}

pub async fn user_stats(name: &str) -> Result<UserStats> {
    if !valid_username(name) {
        return Err(Error::NotFound(format!("invalid username: {name}")));
    }

    let html = get_text(&format!("/~{name}")).await?;
    let doc = Html::parse_document(&html);

    let stories_sel = Selector::parse(r#"a[href$="/stories"]"#).map_err(|_| Error::ParseHtml)?;
    let comments_sel = Selector::parse(r#"a[href$="/threads"]"#).map_err(|_| Error::ParseHtml)?;
    let tag_sel = Selector::parse(r#"a.tag"#).map_err(|_| Error::ParseHtml)?;

    let stories_count = doc
        .select(&stories_sel)
        .next()
        .and_then(|el| el.text().collect::<String>().trim().parse().ok());

    let comments_count = doc
        .select(&comments_sel)
        .next()
        .and_then(|el| el.text().collect::<String>().trim().parse().ok());

    let most_common_tag = doc
        .select(&tag_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string());

    let story_karma = user_stories(
        name,
        &ListOpts {
            limit: 100,
            ..Default::default()
        },
    )
    .await
    .ok()
    .map(|stories| stories.iter().map(|s| s.score).sum());

    Ok(UserStats {
        stories_count,
        comments_count,
        most_common_tag,
        story_karma,
    })
}

pub async fn tags() -> Result<Vec<Tag>> {
    get::<Vec<Tag>>("/tags.json").await.map(|mut v| {
        v.retain(|t| t.active);
        v.sort_unstable_by(|a, b| a.tag.cmp(&b.tag));
        v
    })
}

pub async fn related(story: &StoryDetail, limit: usize) -> Result<Vec<Story>> {
    if story.tags.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen = HashSet::new();
    let mut related = Vec::new();

    for tag in &story.tags {
        let stories: Vec<Story> = match get(&format!("/t/{tag}.json")).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        for s in stories {
            if s.short_id != story.short_id && seen.insert(s.short_id.clone()) {
                related.push(s);
                if related.len() >= limit {
                    related.sort_unstable_by(|a, b| b.score.cmp(&a.score));
                    return Ok(related);
                }
            }
        }
    }

    related.sort_unstable_by(|a, b| b.score.cmp(&a.score));
    related.truncate(limit);
    Ok(related)
}

pub async fn user_stories(name: &str, opts: &ListOpts) -> Result<Vec<Story>> {
    if !valid_username(name) {
        return Err(Error::NotFound(format!("invalid username: {name}")));
    }
    fetch_stories(opts, |p| {
        if p == 1 {
            format!("/~{name}/stories.json")
        } else {
            format!("/~{name}/stories/page/{p}.json")
        }
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct UserComment {
    pub score: i32,
    pub created_at: String,
    pub story_title: String,
    pub story_url: String,
    pub comment_text: String,
}

#[derive(Debug, Serialize)]
pub struct SearchStory {
    pub short_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub score: i32,
    pub submitter_user: String,
    pub created_at: String,
    pub comment_count: u32,
    pub tags: Vec<String>,
    pub has_description: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchComment {
    pub short_id: String,
    pub score: i32,
    pub commenting_user: String,
    pub created_at: String,
    pub story_title: String,
    pub story_short_id: String,
    pub comment_text: String,
}

pub struct SearchOpts {
    pub query: String,
    pub what: SearchWhat,
    pub order: SearchOrder,
    pub page: u32,
    pub limit: usize,
}

impl Default for SearchOpts {
    fn default() -> Self {
        Self {
            query: String::new(),
            what: SearchWhat::Stories,
            order: SearchOrder::Relevance,
            page: 1,
            limit: 25,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum SearchResult {
    Stories(Vec<SearchStory>),
    Comments(Vec<SearchComment>),
}

impl SearchResult {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Stories(v) => v.is_empty(),
            Self::Comments(v) => v.is_empty(),
        }
    }
}

pub async fn user_comments(name: &str, opts: &ListOpts) -> Result<Vec<UserComment>> {
    if !valid_username(name) {
        return Err(Error::NotFound(format!("invalid username: {name}")));
    }

    let path = match opts.page {
        1 => format!("/search?q=commenter%3A{name}&what=comments&order=newest"),
        p => format!("/search?q=commenter%3A{name}&what=comments&order=newest&page={p}"),
    };

    let html = get_text(&path).await?;
    let doc = Html::parse_document(&html);

    let comment_sel = Selector::parse("div.comment").map_err(|_| Error::ParseHtml)?;
    let score_sel = Selector::parse("a.upvoter").map_err(|_| Error::ParseHtml)?;
    let time_sel = Selector::parse("time").map_err(|_| Error::ParseHtml)?;
    let story_link_sel =
        Selector::parse("div.byline a[href^='/s/']").map_err(|_| Error::ParseHtml)?;
    let body_sel = Selector::parse("div.comment_text").map_err(|_| Error::ParseHtml)?;

    let mut comments = Vec::new();

    for el in doc.select(&comment_sel) {
        let score = el
            .select(&score_sel)
            .next()
            .and_then(|s| s.value().attr("title"))
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);

        let created_at = el
            .select(&time_sel)
            .next()
            .map(|t| t.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let story_link = el.select(&story_link_sel).next();
        let story_title = story_link
            .map(|a| a.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let story_url = story_link
            .and_then(|a| a.value().attr("href"))
            .map(|h| format!("{BASE}{h}"))
            .unwrap_or_default();

        let comment_text = el
            .select(&body_sel)
            .next()
            .and_then(|b| html2text::from_read(b.inner_html().as_bytes(), 76).ok())
            .unwrap_or_default()
            .trim()
            .to_string();

        if !comment_text.is_empty() {
            comments.push(UserComment {
                score,
                created_at,
                story_title,
                story_url,
                comment_text,
            });
        }
    }

    let min = opts.min_score;
    Ok(comments
        .into_iter()
        .filter(move |c| min.is_none_or(|m| c.score >= m))
        .take(opts.limit)
        .collect())
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

pub async fn search(opts: &SearchOpts) -> Result<SearchResult> {
    if opts.query.trim().is_empty() {
        return Err(Error::NotFound("empty search query".into()));
    }

    match opts.what {
        SearchWhat::Stories => search_stories(opts).await.map(SearchResult::Stories),
        SearchWhat::Comments => search_comments(opts).await.map(SearchResult::Comments),
    }
}

async fn search_stories(opts: &SearchOpts) -> Result<Vec<SearchStory>> {
    let encoded = url_encode(opts.query.trim());
    let path = match opts.page {
        1 => format!("/search?q={}&what=stories&order={}", encoded, opts.order),
        p => format!(
            "/search?q={}&what=stories&order={}&page={}",
            encoded, opts.order, p
        ),
    };

    let html = get_text(&path).await?;
    let doc = Html::parse_document(&html);

    let story_sel = Selector::parse("li.story").map_err(|_| Error::ParseHtml)?;
    let score_sel = Selector::parse("a.upvoter").map_err(|_| Error::ParseHtml)?;
    let title_sel = Selector::parse("span.link a.u-url").map_err(|_| Error::ParseHtml)?;
    let tag_sel = Selector::parse("span.tags a.tag").map_err(|_| Error::ParseHtml)?;
    let domain_sel = Selector::parse("a.domain").map_err(|_| Error::ParseHtml)?;
    let user_sel = Selector::parse("div.byline a.u-author").map_err(|_| Error::ParseHtml)?;
    let time_sel = Selector::parse("div.byline time").map_err(|_| Error::ParseHtml)?;
    let comments_sel = Selector::parse("span.comments_label a").map_err(|_| Error::ParseHtml)?;
    let desc_sel = Selector::parse("a.description_present").map_err(|_| Error::ParseHtml)?;

    let mut stories = Vec::new();

    for el in doc.select(&story_sel) {
        let short_id = el
            .value()
            .attr("data-shortid")
            .unwrap_or_default()
            .to_string();

        if short_id.is_empty() {
            continue;
        }

        let score = el
            .select(&score_sel)
            .next()
            .map(|s| s.text().collect::<String>().trim().parse().unwrap_or(0))
            .unwrap_or(0);

        let (title, url) = el
            .select(&title_sel)
            .next()
            .map(|a| {
                let t = a.text().collect::<String>().trim().to_string();
                let u = a.value().attr("href").unwrap_or_default();
                let url = if u.starts_with('/') {
                    format!("{BASE}{u}")
                } else {
                    u.to_string()
                };
                (t, url)
            })
            .unwrap_or_default();

        let tags: Vec<String> = el
            .select(&tag_sel)
            .map(|t| t.text().collect::<String>().trim().to_string())
            .collect();

        let domain = el
            .select(&domain_sel)
            .next()
            .map(|d| d.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "self".to_string());

        let submitter_user = el
            .select(&user_sel)
            .next()
            .map(|u| u.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let created_at = el
            .select(&time_sel)
            .next()
            .and_then(|t| t.value().attr("datetime"))
            .unwrap_or_default()
            .to_string();

        let comment_count = el
            .select(&comments_sel)
            .next()
            .and_then(|c| {
                let text = c.text().collect::<String>();
                text.split_whitespace().next().and_then(|n| n.parse().ok())
            })
            .unwrap_or(0);

        let has_description = el.select(&desc_sel).next().is_some();

        stories.push(SearchStory {
            short_id,
            title,
            url,
            domain,
            score,
            submitter_user,
            created_at,
            comment_count,
            tags,
            has_description,
        });

        if stories.len() >= opts.limit {
            break;
        }
    }

    Ok(stories)
}

async fn search_comments(opts: &SearchOpts) -> Result<Vec<SearchComment>> {
    let encoded = url_encode(opts.query.trim());
    let path = match opts.page {
        1 => format!("/search?q={}&what=comments&order={}", encoded, opts.order),
        p => format!(
            "/search?q={}&what=comments&order={}&page={}",
            encoded, opts.order, p
        ),
    };

    let html = get_text(&path).await?;
    let doc = Html::parse_document(&html);

    let comment_sel = Selector::parse("div.comment").map_err(|_| Error::ParseHtml)?;
    let score_sel = Selector::parse("a.upvoter").map_err(|_| Error::ParseHtml)?;
    let user_sel = Selector::parse("div.byline a[href^='/~']:not([aria-hidden])")
        .map_err(|_| Error::ParseHtml)?;
    let time_sel = Selector::parse("div.byline time").map_err(|_| Error::ParseHtml)?;
    let story_sel = Selector::parse("div.byline a[href^='/s/']").map_err(|_| Error::ParseHtml)?;
    let text_sel = Selector::parse("div.comment_text").map_err(|_| Error::ParseHtml)?;

    let mut comments = Vec::new();

    for el in doc.select(&comment_sel) {
        let short_id = el
            .value()
            .attr("data-shortid")
            .unwrap_or_default()
            .to_string();

        if short_id.is_empty() {
            continue;
        }

        let score = el
            .select(&score_sel)
            .next()
            .and_then(|s| s.value().attr("title"))
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);

        let commenting_user = el
            .select(&user_sel)
            .next()
            .map(|u| u.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let created_at = el
            .select(&time_sel)
            .next()
            .and_then(|t| t.value().attr("datetime"))
            .unwrap_or_default()
            .to_string();

        let (story_title, story_short_id) = el
            .select(&story_sel)
            .next()
            .map(|a| {
                let title = a.text().collect::<String>().trim().to_string();
                let href = a.value().attr("href").unwrap_or_default();
                let sid = href
                    .strip_prefix("/s/")
                    .and_then(|s| s.split('/').next())
                    .unwrap_or_default()
                    .to_string();
                (title, sid)
            })
            .unwrap_or_default();

        let comment_text = el
            .select(&text_sel)
            .next()
            .and_then(|b| html2text::from_read(b.inner_html().as_bytes(), 76).ok())
            .unwrap_or_default()
            .trim()
            .to_string();

        if comment_text.is_empty() {
            continue;
        }

        comments.push(SearchComment {
            short_id,
            score,
            commenting_user,
            created_at,
            story_title,
            story_short_id,
            comment_text,
        });

        if comments.len() >= opts.limit {
            break;
        }
    }

    Ok(comments)
}
