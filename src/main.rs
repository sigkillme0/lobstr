mod api;
mod config;
mod display;
mod reader;
mod util;

use api::{CommentSort, ListOpts, SearchOpts, SearchOrder, SearchWhat, TagMode};
use chrono::{DateTime, FixedOffset, Local, TimeDelta};
use clap::{Args, Parser, Subcommand};
use display::DisplayOpts;
use std::num::{NonZeroU32, NonZeroUsize};
use std::process::ExitCode;
use util::OutputFormat;

#[derive(Parser)]
#[command(name = "lobstr", version, about = "terminal client for lobste.rs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// output format (pretty, json, tsv, ids)
    #[arg(long, short = 'F', global = true, default_value = "pretty")]
    format: OutputFormat,

    /// output as json (shorthand for --format json)
    #[arg(long, global = true)]
    json: bool,

    /// exit with code 1 if no results found (for scripting)
    #[arg(long, global = true)]
    strict: bool,
}

impl Cli {
    const fn format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.format
        }
    }
}

#[derive(Args, Clone)]
struct ListArgs {
    /// max stories to show [default: 25]
    #[arg(short, long)]
    limit: Option<NonZeroUsize>,

    /// page number [default: 1]
    #[arg(short, long)]
    page: Option<NonZeroU32>,

    /// minimum score filter
    #[arg(short = 's', long)]
    min_score: Option<i32>,

    /// only stories after this date (e.g. "2024-01-01", "1d", "1w", "1m")
    #[arg(long, value_parser = parse_date)]
    after: Option<DateTime<FixedOffset>>,

    /// only stories before this date (e.g. "2024-01-15", "1h")
    #[arg(long, value_parser = parse_date)]
    before: Option<DateTime<FixedOffset>>,
}

fn parse_date(s: &str) -> Result<DateTime<FixedOffset>, String> {
    #[allow(clippy::type_complexity)]
    const UNITS: [(char, fn(i64) -> Option<TimeDelta>); 5] = [
        ('h', TimeDelta::try_hours),
        ('d', TimeDelta::try_days),
        ('w', TimeDelta::try_weeks),
        ('m', |n| TimeDelta::try_days(n * 30)),
        ('y', |n| TimeDelta::try_days(n * 365)),
    ];

    for (suffix, to_delta) in UNITS {
        if let Some(dt) = s
            .strip_suffix(suffix)
            .and_then(|n| n.parse::<i64>().ok())
            .and_then(to_delta)
            .map(|d| (Local::now() - d).fixed_offset())
        {
            return Ok(dt);
        }
    }

    if let Ok(naive) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = naive.and_hms_opt(0, 0, 0).unwrap();
        return Ok(DateTime::from_naive_utc_and_offset(
            dt,
            *Local::now().offset(),
        ));
    }

    DateTime::parse_from_rfc3339(s)
        .map_err(|_| format!("invalid date: {s} (try: 1d, 1w, 2024-01-15)"))
}

impl From<&ListArgs> for ListOpts {
    fn from(a: &ListArgs) -> Self {
        let default_limit = config::CONFIG
            .default_limit
            .unwrap_or(config::DEFAULT_LIMIT);
        Self {
            limit: a.limit.map_or(default_limit, NonZeroUsize::get),
            page: a.page.map_or(1, NonZeroU32::get),
            min_score: a.min_score,
            after: a.after,
            before: a.before,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// hottest stories
    Hot {
        #[command(flatten)]
        args: ListArgs,
    },
    /// newest stories
    New {
        #[command(flatten)]
        args: ListArgs,
    },
    /// stories by tag (comma-separated for multiple; AND by default, --any for OR)
    Tag {
        /// tag name(s), comma-separated
        name: String,
        /// match ANY of the tags (OR) instead of ALL (AND)
        #[arg(long)]
        any: bool,
        /// exclude stories with these tags (comma-separated)
        #[arg(long)]
        exclude: Option<String>,
        #[command(flatten)]
        args: ListArgs,
    },
    /// active discussions
    Active {
        #[command(flatten)]
        args: ListArgs,
    },
    /// search stories or comments
    Search {
        /// search query (supports multiple terms, "quoted phrases")
        query: String,
        /// search stories or comments
        #[arg(short, long, default_value = "stories")]
        what: SearchWhat,
        /// order by: relevance, newest, score
        #[arg(short, long, default_value = "relevance")]
        order: SearchOrder,
        /// max results to show [default: 25]
        #[arg(short, long)]
        limit: Option<NonZeroUsize>,
        /// page number [default: 1]
        #[arg(short, long)]
        page: Option<NonZeroU32>,
    },
    /// view story with comments
    View {
        /// story id (e.g. ngwloq)
        id: String,
        /// show full comments without truncation
        #[arg(short, long)]
        full: bool,
        /// sort comments: score, newest, oldest, default
        #[arg(long, default_value = "default")]
        sort: CommentSort,
        /// show related stories (same tags)
        #[arg(short, long)]
        related: bool,
        /// number of related stories to show
        #[arg(long, default_value = "5")]
        related_limit: NonZeroUsize,
    },
    /// user info and activity
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// list available tags
    Tags {
        /// filter by category
        #[arg(short, long)]
        category: Option<String>,
    },
    /// read article content in terminal
    Open {
        /// story id (e.g. ngwloq)
        id: String,
        /// show full content without truncation
        #[arg(short, long)]
        full: bool,
        /// width for text wrapping (default: 80)
        #[arg(short, long, default_value = "80")]
        width: usize,
        /// dump raw extracted text (no formatting)
        #[arg(long)]
        raw: bool,
    },
}

#[derive(Subcommand)]
enum UserAction {
    /// show user profile with stats
    Info { name: String },
    /// show user's submitted stories
    Stories {
        name: String,
        #[command(flatten)]
        args: ListArgs,
    },
    /// show user's comments
    Comments {
        name: String,
        #[command(flatten)]
        args: ListArgs,
    },
}

enum RunResult {
    Ok,
    Empty,
    Err(api::Error),
}

impl<T> From<api::Result<Vec<T>>> for RunResult {
    fn from(r: api::Result<Vec<T>>) -> Self {
        match r {
            Ok(v) if v.is_empty() => Self::Empty,
            Ok(_) => Self::Ok,
            Err(e) => Self::Err(e),
        }
    }
}

fn fetch_and_show<T, F>(items: api::Result<Vec<T>>, show: F) -> RunResult
where
    F: FnOnce(&[T]),
{
    if let Ok(ref v) = items {
        show(v);
    }
    items.into()
}

#[allow(clippy::too_many_lines, clippy::future_not_send)]
async fn run(cli: &Cli) -> RunResult {
    let format = cli.format();
    let opts = DisplayOpts { format };

    match &cli.cmd {
        Cmd::Hot { args } => {
            let p = args.page.map_or(1, NonZeroU32::get);
            fetch_and_show(api::hottest(&args.into()).await, |v| {
                display::stories(v, &opts, p);
            })
        }
        Cmd::New { args } => {
            let p = args.page.map_or(1, NonZeroU32::get);
            fetch_and_show(api::newest(&args.into()).await, |v| {
                display::stories(v, &opts, p);
            })
        }
        Cmd::Active { args } => {
            let p = args.page.map_or(1, NonZeroU32::get);
            fetch_and_show(api::active(&args.into()).await, |v| {
                display::stories(v, &opts, p);
            })
        }
        Cmd::Tag {
            name,
            any,
            exclude,
            args,
        } => {
            let mode = if *any { TagMode::Any } else { TagMode::All };
            let p = args.page.map_or(1, NonZeroU32::get);
            fetch_and_show(
                api::by_tag(name, &args.into(), mode, exclude.as_deref()).await,
                |v| {
                    display::stories(v, &opts, p);
                },
            )
        }
        Cmd::Search {
            query,
            what,
            order,
            limit,
            page,
        } => {
            let default_limit = config::CONFIG
                .default_limit
                .unwrap_or(config::DEFAULT_LIMIT);
            let search_opts = SearchOpts {
                query: query.clone(),
                what: *what,
                order: *order,
                page: page.map_or(1, NonZeroU32::get),
                limit: limit.map_or(default_limit, NonZeroUsize::get),
            };
            match api::search(&search_opts).await {
                Ok(result) if result.is_empty() => RunResult::Empty,
                Ok(result) => {
                    display::search_results(&result, &opts, page.map_or(1, NonZeroU32::get));
                    RunResult::Ok
                }
                Err(e) => RunResult::Err(e),
            }
        }
        Cmd::View {
            id,
            full,
            sort,
            related,
            related_limit,
        } => match api::story(id).await {
            Ok(mut story) => {
                api::sort_comments(&mut story.comments, *sort);
                display::story_detail(&story, &opts, *full);
                if *related {
                    if let Ok(rel) = api::related(&story, related_limit.get()).await {
                        if !rel.is_empty() {
                            display::related_stories(&rel, &opts);
                        }
                    }
                }
                RunResult::Ok
            }
            Err(e) => RunResult::Err(e),
        },
        Cmd::User { action } => match action {
            UserAction::Info { name } => {
                let (user_result, stats_result) =
                    tokio::join!(api::user(name), api::user_stats(name));
                match user_result {
                    Ok(user) => {
                        display::user(&user, stats_result.ok().as_ref(), &opts);
                        RunResult::Ok
                    }
                    Err(e) => RunResult::Err(e),
                }
            }
            UserAction::Stories { name, args } => {
                let p = args.page.map_or(1, NonZeroU32::get);
                fetch_and_show(api::user_stories(name, &args.into()).await, |v| {
                    display::stories(v, &opts, p);
                })
            }
            UserAction::Comments { name, args } => {
                let p = args.page.map_or(1, NonZeroU32::get);
                fetch_and_show(api::user_comments(name, &args.into()).await, |v| {
                    display::user_comments(v, &opts, p);
                })
            }
        },
        Cmd::Tags { category } => fetch_and_show(api::tags().await, |v| {
            display::tags(v, &opts, category.as_deref());
        }),
        Cmd::Open {
            id,
            full,
            width,
            raw,
        } => match api::story(id).await {
            Ok(story) => {
                let read_opts = reader::ReadOpts {
                    width: *width,
                    full: *full,
                    raw: *raw,
                    format,
                };
                match reader::read_article(&story, &read_opts).await {
                    Ok(()) => RunResult::Ok,
                    Err(e) => RunResult::Err(api::Error::ParseHtml(format!(
                        "failed to read article: {e}"
                    ))),
                }
            }
            Err(e) => RunResult::Err(e),
        },
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // Reset SIGPIPE to default so broken pipes kill the process cleanly
    // instead of causing write errors throughout the display code
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    match run(&cli).await {
        RunResult::Empty if cli.strict => ExitCode::FAILURE,
        RunResult::Ok | RunResult::Empty => ExitCode::SUCCESS,
        RunResult::Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
