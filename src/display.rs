use crate::api::{
    Comment, SearchComment, SearchResult, SearchStory, Story, StoryDetail, Tag, User, UserComment,
    UserStats,
};
use crate::util::{OutputFormat, extract_domain, print_json, style};
use colored::{ColoredString, Colorize};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, Write};

const WRAP_WIDTH: usize = 76;
const COMMENT_PREVIEW_LINES: usize = 12;

fn score_color(score: i32, text: &str) -> ColoredString {
    match score {
        n if n >= 50 => style!(text, bright_green, bold),
        n if n >= 20 => style!(text, green),
        n if n < 0 => style!(text, red),
        _ => text.normal(),
    }
}

fn comment_score_color(score: i32) -> ColoredString {
    let text = format!("{score:+}");
    match score {
        n if n >= 10 => style!(text.as_str(), green),
        n if n < 0 => style!(text.as_str(), red),
        _ => style!(text.as_str(), dimmed),
    }
}

fn relative_time(datetime_str: &str) -> String {
    let dt = chrono::DateTime::parse_from_rfc3339(datetime_str)
        .map(|dt| dt.to_utc())
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
                .map(|naive| naive.and_utc())
        });

    dt.map_or_else(
        |_| "?".into(),
        |dt| {
            let d = chrono::Utc::now().signed_duration_since(dt);
            match (d.num_days(), d.num_hours(), d.num_minutes()) {
                (days, _, _) if days > 0 => format!("{days}d"),
                (_, hours, _) if hours > 0 => format!("{hours}h"),
                (_, _, mins) if mins > 0 => format!("{mins}m"),
                _ => "now".into(),
            }
        },
    )
}

#[derive(Default)]
pub struct DisplayOpts {
    pub format: OutputFormat,
    pub full: bool,
}

pub fn stories(items: &[Story], opts: &DisplayOpts, page: u32) {
    print_stories(items, opts, Some(page), None);
}

pub fn related_stories(items: &[Story], opts: &DisplayOpts) {
    print_stories(items, opts, None, Some("--- related stories ---"));
}

fn print_stories(items: &[Story], opts: &DisplayOpts, page: Option<u32>, header: Option<&str>) {
    match opts.format {
        OutputFormat::Json => return print_json(items),
        OutputFormat::Ids => {
            for s in items {
                println!("{}", s.short_id);
            }
            return;
        }
        OutputFormat::Tsv => {
            for s in items {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    s.short_id,
                    s.title,
                    s.score,
                    s.comment_count,
                    s.submitter_user,
                    s.tags.join(","),
                    s.url
                );
            }
            return;
        }
        OutputFormat::Pretty => {}
    }

    if items.is_empty() && header.is_none() {
        return println!("no stories found");
    }

    let mut out = io::stdout().lock();

    if let Some(h) = header {
        writeln!(out).ok();
        writeln!(out, "{}", style!(h, dimmed)).ok();
        writeln!(out).ok();
    }

    for s in items {
        let score = format!("{:4}", s.score);
        writeln!(
            out,
            "{} {} {}",
            score_color(s.score, &score),
            style!(s.title.as_str(), bold),
            style!(format!("({})", s.short_id).as_str(), dimmed)
        )
        .ok();
        writeln!(
            out,
            "     {} | {} | {} | {}c | {}",
            style!(s.tags.join(",").as_str(), cyan),
            style!(s.submitter_user.as_str(), yellow),
            relative_time(&s.created_at),
            s.comment_count,
            style!(extract_domain(&s.url), dimmed)
        )
        .ok();
        writeln!(out).ok();
    }

    if let Some(p) = page {
        if p > 1 {
            writeln!(out, "{}", style!(format!("[page {p}]").as_str(), dimmed)).ok();
        }
    }
}

pub fn story_detail(s: &StoryDetail, opts: &DisplayOpts) {
    match opts.format {
        OutputFormat::Json => return print_json(s),
        OutputFormat::Ids => {
            println!("{}", s.short_id);
            return;
        }
        _ => {}
    }

    let mut out = io::stdout().lock();
    let sep_len = s.title.chars().count().min(70);

    writeln!(out, "{}", style!(s.title.as_str(), bold)).ok();
    writeln!(out, "{}", style!("-".repeat(sep_len).as_str(), dimmed)).ok();

    if !s.url.is_empty() {
        writeln!(out, "{}", style!(s.url.as_str(), blue)).ok();
    }
    writeln!(out).ok();

    writeln!(
        out,
        "{} pts | {} | {} | {}",
        style!(s.score.to_string().as_str(), green),
        style!(s.submitter_user.as_str(), yellow),
        relative_time(&s.created_at),
        style!(s.tags.join(", ").as_str(), cyan)
    )
    .ok();

    if !s.description_plain.is_empty() {
        writeln!(out).ok();
        for line in s.description_plain.lines() {
            writeln!(out, "  {}", style!(line, italic)).ok();
        }
    }

    let visible: Vec<_> = s.comments.iter().filter(|c| c.is_visible()).collect();
    if visible.is_empty() {
        return;
    }

    writeln!(out).ok();
    writeln!(
        out,
        "{}",
        style!(
            format!("--- {} comments ---", s.comment_count).as_str(),
            dimmed
        )
    )
    .ok();
    writeln!(out).ok();

    for c in visible {
        print_comment(&mut out, c, opts.full);
    }
}

fn print_comment<W: Write>(out: &mut W, c: &Comment, full: bool) {
    let indent = "  ".repeat(c.depth as usize);
    let bar = if c.depth > 0 { "| " } else { "" };

    writeln!(
        out,
        "{indent}{bar}{} {} {}",
        style!(c.commenting_user.as_str(), yellow),
        comment_score_color(c.score),
        style!(relative_time(&c.created_at).as_str(), dimmed)
    )
    .ok();

    let prefix_len = indent.len() + bar.len() + 2;
    let text = html2text::from_read(c.comment.as_bytes(), WRAP_WIDTH.saturating_sub(prefix_len))
        .unwrap_or_default();

    let max = if full {
        usize::MAX
    } else {
        COMMENT_PREVIEW_LINES
    };
    let lines: Vec<_> = text.trim().lines().collect();

    for line in lines.iter().take(max) {
        writeln!(out, "{indent}{bar}  {line}").ok();
    }
    if lines.len() > max {
        writeln!(out, "{indent}{bar}  {}", style!("[...]", dimmed)).ok();
    }
    writeln!(out).ok();
}

pub fn user(u: &User, stats: Option<&UserStats>, opts: &DisplayOpts) {
    if matches!(opts.format, OutputFormat::Json) {
        if let Some(s) = stats {
            #[derive(Serialize)]
            struct Combined<'a> {
                #[serde(flatten)]
                user: &'a User,
                stats: &'a UserStats,
            }
            return print_json(&Combined { user: u, stats: s });
        }
        return print_json(u);
    }
    if matches!(opts.format, OutputFormat::Ids) {
        println!("{}", u.username);
        return;
    }

    let mut out = io::stdout().lock();

    writeln!(out, "{}", style!(u.username.as_str(), yellow, bold)).ok();
    writeln!(
        out,
        "{}",
        style!("-".repeat(u.username.len()).as_str(), dimmed)
    )
    .ok();
    writeln!(out).ok();
    writeln!(out, "joined  {}", relative_time(&u.created_at)).ok();

    if u.is_admin {
        writeln!(out, "role    {}", style!("admin", bright_red)).ok();
    } else if u.is_moderator {
        writeln!(out, "role    {}", style!("moderator", bright_blue)).ok();
    }

    if let Some(s) = stats {
        if let Some(count) = s.stories_count {
            write!(out, "stories {}", style!(count.to_string().as_str(), green)).ok();
            match &s.most_common_tag {
                Some(tag) => writeln!(out, " (top: {})", style!(tag.as_str(), cyan)).ok(),
                None => writeln!(out).ok(),
            };
        }
        if let Some(count) = s.comments_count {
            writeln!(out, "comments {count}").ok();
        }
        if let Some(karma) = s.story_karma {
            let k = karma.to_string();
            let colored = match karma {
                n if n >= 1000 => style!(k.as_str(), bright_green, bold),
                n if n >= 100 => style!(k.as_str(), green),
                n if n < 0 => style!(k.as_str(), red),
                _ => k.normal(),
            };
            writeln!(out, "karma   {colored}").ok();
        }
    }

    if let Some(inv) = &u.invited_by_user {
        writeln!(out, "inv by  {}", style!(inv.as_str(), yellow)).ok();
    }
    if let Some(gh) = &u.github_username {
        writeln!(out, "github  {gh}").ok();
    }
    if let Some(m) = &u.mastodon_username {
        writeln!(out, "masto   {m}").ok();
    }

    if !u.about.is_empty() {
        writeln!(out).ok();
        write!(
            out,
            "{}",
            html2text::from_read(u.about.as_bytes(), WRAP_WIDTH).unwrap_or_default()
        )
        .ok();
    }
}

pub fn tags(items: &[Tag], opts: &DisplayOpts, filter: Option<&str>) {
    match opts.format {
        OutputFormat::Json => {
            let filtered: Vec<_> = items
                .iter()
                .filter(|t| filter.is_none_or(|f| t.category.eq_ignore_ascii_case(f)))
                .collect();
            return print_json(&filtered);
        }
        OutputFormat::Ids => {
            for t in items {
                let cat = if t.category.is_empty() {
                    "other"
                } else {
                    &t.category
                };
                if filter.is_none_or(|f| cat.eq_ignore_ascii_case(f)) {
                    println!("{}", t.tag);
                }
            }
            return;
        }
        OutputFormat::Tsv => {
            for t in items {
                let cat = if t.category.is_empty() {
                    "other"
                } else {
                    &t.category
                };
                if filter.is_none_or(|f| cat.eq_ignore_ascii_case(f)) {
                    println!("{}\t{}\t{}", t.tag, cat, t.description);
                }
            }
            return;
        }
        OutputFormat::Pretty => {}
    }

    let mut out = io::stdout().lock();

    let by_cat: BTreeMap<&str, Vec<&Tag>> = items.iter().fold(BTreeMap::new(), |mut acc, t| {
        let cat = if t.category.is_empty() {
            "other"
        } else {
            &t.category
        };
        if filter.is_none_or(|f| cat.eq_ignore_ascii_case(f)) {
            acc.entry(cat).or_default().push(t);
        }
        acc
    });

    if by_cat.is_empty() {
        if let Some(cat) = filter {
            writeln!(out, "no tags in category '{cat}'").ok();
        }
        return;
    }

    for (cat, mut tags) in by_cat {
        writeln!(out, "{}", style!(cat.to_uppercase().as_str(), cyan, bold)).ok();
        tags.sort_unstable_by_key(|t| &t.tag);
        for t in tags {
            let m = if t.is_media { "*" } else { " " };
            writeln!(
                out,
                "  {m}{:18} {}",
                t.tag,
                style!(t.description.as_str(), dimmed)
            )
            .ok();
        }
        writeln!(out).ok();
    }
}

pub fn user_comments(items: &[UserComment], opts: &DisplayOpts, page: u32) {
    match opts.format {
        OutputFormat::Json => return print_json(items),
        OutputFormat::Tsv => {
            for c in items {
                let preview = c.comment_text.lines().next().unwrap_or("");
                println!(
                    "{}\t{}\t{}\t{}",
                    c.score, c.created_at, c.story_title, preview
                );
            }
            return;
        }
        _ => {}
    }

    if items.is_empty() {
        return println!("no comments found");
    }

    let mut out = io::stdout().lock();

    for c in items {
        writeln!(
            out,
            "{} {} on: {}",
            comment_score_color(c.score),
            style!(c.created_at.as_str(), dimmed),
            style!(c.story_title.as_str(), cyan)
        )
        .ok();

        let lines: Vec<_> = c.comment_text.lines().collect();
        for line in lines.iter().take(6) {
            writeln!(out, "  {line}").ok();
        }
        if lines.len() > 6 {
            writeln!(out, "  {}", style!("[...]", dimmed)).ok();
        }
        writeln!(out).ok();
    }

    if page > 1 {
        writeln!(out, "{}", style!(format!("[page {page}]").as_str(), dimmed)).ok();
    }
}

pub fn search_results(result: &SearchResult, opts: &DisplayOpts, page: u32) {
    match result {
        SearchResult::Stories(stories) => search_stories(stories, opts, page),
        SearchResult::Comments(comments) => search_comments(comments, opts, page),
    }
}

fn search_stories(items: &[SearchStory], opts: &DisplayOpts, page: u32) {
    match opts.format {
        OutputFormat::Json => return print_json(items),
        OutputFormat::Ids => {
            for s in items {
                println!("{}", s.short_id);
            }
            return;
        }
        OutputFormat::Tsv => {
            for s in items {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    s.short_id,
                    s.title,
                    s.score,
                    s.comment_count,
                    s.submitter_user,
                    s.tags.join(","),
                    s.url
                );
            }
            return;
        }
        OutputFormat::Pretty => {}
    }

    if items.is_empty() {
        return println!("no stories found");
    }

    let mut out = io::stdout().lock();

    for s in items {
        let score = format!("{:4}", s.score);
        let desc_marker = if s.has_description { "☰" } else { " " };
        writeln!(
            out,
            "{} {}{} {}",
            score_color(s.score, &score),
            desc_marker,
            style!(s.title.as_str(), bold),
            style!(format!("({})", s.short_id).as_str(), dimmed)
        )
        .ok();
        writeln!(
            out,
            "     {} | {} | {} | {}c | {}",
            style!(s.tags.join(",").as_str(), cyan),
            style!(s.submitter_user.as_str(), yellow),
            relative_time(&s.created_at),
            s.comment_count,
            style!(s.domain.as_str(), dimmed)
        )
        .ok();
        writeln!(out).ok();
    }

    if page > 1 {
        writeln!(out, "{}", style!(format!("[page {page}]").as_str(), dimmed)).ok();
    }
}

fn search_comments(items: &[SearchComment], opts: &DisplayOpts, page: u32) {
    match opts.format {
        OutputFormat::Json => return print_json(items),
        OutputFormat::Ids => {
            for c in items {
                println!("{}", c.story_short_id);
            }
            return;
        }
        OutputFormat::Tsv => {
            for c in items {
                let preview = c.comment_text.lines().next().unwrap_or("");
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    c.score, c.commenting_user, c.created_at, c.story_title, preview
                );
            }
            return;
        }
        OutputFormat::Pretty => {}
    }

    if items.is_empty() {
        return println!("no comments found");
    }

    let mut out = io::stdout().lock();

    for c in items {
        writeln!(
            out,
            "{} {} by {} on: {} {}",
            comment_score_color(c.score),
            style!(relative_time(&c.created_at).as_str(), dimmed),
            style!(c.commenting_user.as_str(), yellow),
            style!(c.story_title.as_str(), cyan),
            style!(format!("({})", c.story_short_id).as_str(), dimmed)
        )
        .ok();

        let lines: Vec<_> = c.comment_text.lines().collect();
        for line in lines.iter().take(6) {
            writeln!(out, "  {line}").ok();
        }
        if lines.len() > 6 {
            writeln!(out, "  {}", style!("[...]", dimmed)).ok();
        }
        writeln!(out).ok();
    }

    if page > 1 {
        writeln!(out, "{}", style!(format!("[page {page}]").as_str(), dimmed)).ok();
    }
}
