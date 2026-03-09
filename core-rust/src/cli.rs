#[derive(Debug, Clone)]
pub(crate) enum CliCommand {
    Serve,
    Backfill { limit: Option<usize> },
    BackfillConversationRecall { limit: Option<usize> },
}

pub(crate) fn parse_cli_command() -> Result<CliCommand, String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(CliCommand::Serve);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }

    match args[0].as_str() {
        "serve" => Ok(CliCommand::Serve),
        "backfill" => {
            let mut limit = None;
            let mut idx = 1usize;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--limit" => {
                        let value = args.get(idx + 1).ok_or("--limit requires a value")?;
                        limit = Some(
                            value
                                .parse::<usize>()
                                .map_err(|err| format!("invalid --limit: {}", err))?
                                .max(1),
                        );
                        idx += 2;
                    }
                    option => return Err(format!("unknown option for backfill: {}", option)),
                }
            }
            Ok(CliCommand::Backfill { limit })
        }
        "backfill-conversation-recall" => {
            let mut limit = None;
            let mut idx = 1usize;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--limit" => {
                        let value = args.get(idx + 1).ok_or("--limit requires a value")?;
                        limit = Some(
                            value
                                .parse::<usize>()
                                .map_err(|err| format!("invalid --limit: {}", err))?
                                .max(1),
                        );
                        idx += 2;
                    }
                    option => {
                        return Err(format!(
                            "unknown option for backfill-conversation-recall: {}",
                            option
                        ))
                    }
                }
            }
            Ok(CliCommand::BackfillConversationRecall { limit })
        }
        command => Err(format!("unknown command: {}", command)),
    }
}

fn print_usage() {
    println!("tsuki-core-rust");
    println!("Usage:");
    println!("  tsuki-core-rust [serve]");
    println!("  tsuki-core-rust backfill [--limit N]");
    println!("  tsuki-core-rust backfill-conversation-recall [--limit N]");
}
