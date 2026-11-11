use crate::commands::{self, Flags};
use crate::error::{AppError, UsageError};
use crate::output::{self};
use clap::{ArgAction, ColorChoice, Parser, Subcommand};

const DESCRIPTION: &str =
    "Ranked videos and insights for agentic engineering, AI engineering, and AI research.";

#[derive(Parser, Debug)]
#[command(
    name = "hackertube",
    about = DESCRIPTION,
    color = ColorChoice::Never,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, global = true, action = ArgAction::SetTrue, help = "JSON instead of TOON")]
    json: bool,
    #[arg(long, global = true, action = ArgAction::SetTrue, help = "do not truncate text")]
    full: bool,
    #[arg(long, global = true, help = "comma-separated columns")]
    fields: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Load the HackerTube dataset and AI Engineer talks
    Ingest {
        #[arg(long, help = "dataset zip/dir, or an aietalks snapshot dir")]
        path: Option<String>,
        #[arg(long, value_parser = ["hackertube", "aietalks", "all"], default_value = "all")]
        source: String,
        /// AI Engineer talks to ingest; 0 means the full index
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long, action = ArgAction::SetTrue, help = "re-download cached sources")]
        refresh: bool,
    },
    /// Semantic plus keyword search
    Search {
        query: String,
        #[arg(long, value_parser = ["agentic-engineering", "ai-engineering", "ai-research"])]
        area: Option<String>,
        #[arg(long = "in", value_parser = ["catalog", "transcripts"], default_value = "catalog")]
        in_: String,
        #[arg(long, value_parser = ["hackertube", "aietalks"])]
        source: Option<String>,
        #[arg(long)]
        limit: Option<i64>,
    },
    /// Rank unseen videos in a focus area
    Recommend {
        #[arg(long, value_parser = ["agentic-engineering", "ai-engineering", "ai-research"])]
        area: Option<String>,
        #[arg(long, value_parser = ["hackertube", "aietalks"])]
        source: Option<String>,
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long, action = ArgAction::SetTrue)]
        include_news: bool,
    },
    /// Neighbors by embedding, else co-recommendation
    Similar {
        vid: String,
        #[arg(long)]
        limit: Option<i64>,
    },
    /// One video card
    Show { vid: String },
    /// AI Engineer markdown, else YouTube captions
    Transcript { vid: String },
    /// Chapters, claims, mentions, and quotes
    Insights {
        vid: String,
        #[arg(long, action = ArgAction::SetTrue, help = "optional pass; needs HACKERTUBE_LLM_API_KEY")]
        llm: bool,
    },
    /// Mark a video seen
    Seen { vid: String },
    /// Record a like; likes stay recommendable
    Like { vid: String },
    /// Hide a video until forget
    Dislike { vid: String },
    /// Hide a video until a date
    Snooze {
        vid: String,
        #[arg(long, help = "default 7")]
        days: Option<i64>,
        #[arg(long, help = "YYYY-MM-DD")]
        until: Option<String>,
    },
    /// Remove a seen, like, dislike, or snooze mark
    Forget { vid: String },
    /// List saved marks
    State {
        #[arg(long)]
        limit: Option<i64>,
    },
    /// Registered sources and row counts
    Sources,
    /// Focus areas and eligible counts
    Areas,
    /// Install the optional session hook
    Setup,
}

pub fn run() -> i32 {
    run_from(std::env::args_os())
}

pub fn run_from<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args: Vec<_> = args.into_iter().collect();
    match Cli::try_parse_from(args) {
        Ok(cli) => dispatch(cli),
        Err(err) => {
            use clap::error::ErrorKind;
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = err.print();
                return 0;
            }
            let detail = err.to_string();
            let flag = detail
                .split_whitespace()
                .find(|token| token.starts_with("--"))
                .map(|token| token.trim_matches(|ch| ch == '\'' || ch == '"').to_string());
            let code = if flag.is_some() {
                "unknown_flag"
            } else {
                "usage"
            };
            let extra = flag
                .map(|flag| vec![("flag".into(), flag)])
                .unwrap_or_default();
            let doc = output::error_doc(
                code,
                detail.lines().next().unwrap_or("usage error"),
                2,
                &["hackertube --help"],
                &extra,
            );
            output::emit(&doc, false, false);
            2
        }
    }
}

fn dispatch(cli: Cli) -> i32 {
    let flags = Flags {
        json: cli.json,
        full: cli.full,
        fields: cli.fields.map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect()
        }),
    };
    let result = match cli.command {
        None => commands::home(&flags),
        Some(Command::Ingest {
            path,
            source,
            limit,
            refresh,
        }) => commands::ingest_cmd(&flags, path.as_deref(), &source, limit, refresh),
        Some(Command::Search {
            query,
            area,
            in_,
            source,
            limit,
        }) => commands::search(
            &flags,
            &query,
            area.as_deref(),
            &in_,
            source.as_deref(),
            limit,
        ),
        Some(Command::Recommend {
            area,
            source,
            limit,
            include_news,
        }) => commands::recommend(
            &flags,
            area.as_deref(),
            limit,
            include_news,
            source.as_deref(),
        ),
        Some(Command::Similar { vid, limit }) => commands::similar(&flags, &vid, limit),
        Some(Command::Show { vid }) => commands::show(&flags, &vid),
        Some(Command::Transcript { vid }) => commands::transcript(&flags, &vid),
        Some(Command::Insights { vid, llm }) => commands::insights_cmd(&flags, &vid, llm),
        Some(Command::Seen { vid }) => commands::mark(&flags, "seen", &vid, None, None),
        Some(Command::Like { vid }) => commands::mark(&flags, "like", &vid, None, None),
        Some(Command::Dislike { vid }) => commands::mark(&flags, "dislike", &vid, None, None),
        Some(Command::Snooze { vid, days, until }) => {
            commands::mark(&flags, "snooze", &vid, days, until.as_deref())
        }
        Some(Command::Forget { vid }) => commands::mark(&flags, "forget", &vid, None, None),
        Some(Command::State { limit }) => commands::state_cmd(&flags, limit),
        Some(Command::Sources) => commands::sources_cmd(&flags),
        Some(Command::Areas) => commands::areas_cmd(&flags),
        Some(Command::Setup) => commands::setup(&flags),
    };
    match result {
        Ok((_, code)) => code,
        Err(err) => {
            let exit = err
                .extra
                .iter()
                .find(|(key, _)| key == "__exit")
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(1);
            let extra: Vec<(String, String)> = err
                .extra
                .into_iter()
                .filter(|(key, _)| key != "__exit")
                .collect();
            let doc =
                output::error_doc(&err.code, &err.detail, exit, &["hackertube --help"], &extra);
            output::emit(&doc, flags.json, flags.full);
            exit as i32
        }
    }
}

pub fn usage(err: UsageError) -> AppError {
    let mut app = AppError::new("usage", err.detail);
    if let Some(flag) = err.flag {
        app = app.with("flag", flag);
    }
    app
}
