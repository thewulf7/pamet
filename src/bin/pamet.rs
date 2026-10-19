//! `pamet` CLI (contracts/cli-contract.md). T009, T016, T017, T027, T034.

use clap::{Parser, Subcommand};
use pamet::{Decision, Interaction, Store};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "pamet")]
struct Cli {
    #[arg(long, default_value = "./pamet.db")]
    db: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Remember {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "cli")]
        source_id: String,
        #[arg(long)]
        explicit_remember: bool,
    },
    Recall {
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 5)]
        budget: usize,
    },
    Outcome {
        #[arg(long)]
        event_id: i64,
        #[arg(long)]
        item_id: String,
        #[arg(long, action = clap::ArgAction::Set, value_parser = clap::value_parser!(bool))]
        used: bool,
    },
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    Forget {
        #[arg(long)]
        item_id: String,
    },
    ExportLog {
        #[arg(long)]
        since: Option<String>,
    },
    Consolidate {},
    Eval {
        #[arg(long, default_value = "tests/fixtures/eval_probes_v1.json")]
        fixtures: String,
        #[arg(long)]
        with_hindsight: bool,
        #[arg(long)]
        report_out: Option<String>,
    },
    /// Migrate every event from `--from` into `--to`, replaying through the
    /// exact same event-sourced rebuild machinery `pamet` uses to verify a
    /// store against its own log (Constitution Principle I) — not a bespoke
    /// copy tool. `--to` may be a fresh or existing store of either backend.
    Migrate {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Resolve {
        #[arg(long)]
        review_id: i64,
        #[arg(long)]
        decision: String,
    },
}

fn parse_decision(s: &str) -> Option<Decision> {
    match s {
        "store" => Some(Decision::Store),
        "merge" => Some(Decision::Merge),
        "supersede" => Some(Decision::Supersede),
        "skip" => Some(Decision::Skip),
        _ => None,
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // `migrate` opens its own two stores (--from/--to) instead of the
    // default `--db` store, and must not fail just because `--db`'s file
    // doesn't exist.
    if let Command::Migrate { from, to } = &cli.command {
        return match run_migrate(from, to) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        };
    }

    let store = match Store::open(&cli.db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("storage error: {e}");
            return ExitCode::from(1);
        }
    };

    let result = run(&store, cli.command);
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn run_migrate(from: &str, to: &str) -> Result<ExitCode, pamet::Error> {
    let source = Store::open(from)?;
    let target = Store::open(to)?;
    let n = pamet::services::rebuild::rebuild_into(&source, &target)?;
    println!("migrated {n} events from {from} to {to}");
    Ok(ExitCode::SUCCESS)
}

fn run(store: &Store, command: Command) -> Result<ExitCode, pamet::Error> {
    match command {
        Command::Remember {
            text,
            source_id,
            explicit_remember,
        } => {
            let interaction = Interaction {
                source_id,
                text,
                explicit_remember,
            };
            let decisions = pamet::remember(store, &interaction)?;
            for d in decisions {
                println!(
                    "{} rule={} item={}",
                    d.decision.as_str(),
                    d.rule_name,
                    d.resulting_item_id.unwrap_or_else(|| "-".to_string())
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Recall { query, budget } => {
            let result = pamet::recall(store, &query, Some(budget))?;
            if result.items.is_empty() {
                println!("no relevant memory found");
            } else {
                for item in &result.items {
                    println!("{} [{}] {}", item.id, item.item_type.as_str(), item.content);
                }
            }
            eprintln!("event_id={}", result.event_id);
            Ok(ExitCode::SUCCESS)
        }
        Command::Outcome {
            event_id,
            item_id,
            used,
        } => {
            pamet::report_outcome(store, event_id, &item_id, used)?;
            println!("ok");
            Ok(ExitCode::SUCCESS)
        }
        Command::Review { action } => match action {
            ReviewAction::List { status } => {
                let rows =
                    pamet::services::review::list(store, status.as_deref().or(Some("pending")))?;
                for r in rows {
                    println!(
                        "{} wd={} reason={} status={}",
                        r.id, r.write_decision_id, r.reason, r.status
                    );
                }
                Ok(ExitCode::SUCCESS)
            }
            ReviewAction::Resolve {
                review_id,
                decision,
            } => {
                let decision = parse_decision(&decision).ok_or_else(|| {
                    pamet::Error::Validation(format!("invalid decision '{decision}'"))
                })?;
                pamet::resolve_review(store, review_id, decision)?;
                println!("ok");
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Forget { item_id } => {
            pamet::tombstone(store, &item_id)?;
            println!("ok");
            Ok(ExitCode::SUCCESS)
        }
        Command::ExportLog { since } => {
            let events = pamet::export_log(store, since.as_deref())?;
            for e in events {
                println!("{}", serde_json::to_string(&e).unwrap_or_default());
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Consolidate {} => {
            let summary = pamet::consolidate(store)?;
            println!(
                "consolidation_run={} items={} clusters={} contradictions={} decayed={}",
                summary.run_id,
                summary.items_processed,
                summary.clusters_formed,
                summary.contradictions_flagged,
                summary.items_decayed
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Eval {
            fixtures,
            with_hindsight,
            report_out,
        } => {
            let report = pamet::services::eval_harness::run_and_report(
                store,
                std::path::Path::new(&fixtures),
                with_hindsight,
            )?;
            let out_path = report_out.unwrap_or_else(|| {
                format!(
                    "./pamet-eval-report-{}.json",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                )
            });
            pamet::services::eval_harness::write_report_file(
                &report,
                std::path::Path::new(&out_path),
            )?;
            println!("report written to {out_path}");
            if pamet::services::eval_harness::pamet_report_all_passed(&report) {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Migrate { .. } => {
            unreachable!("migrate is handled directly in main() before run()")
        }
    }
}
