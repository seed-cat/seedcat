use std::io::{BufRead, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::exit;
use std::str::FromStr;
use std::time::Duration;
use std::{env, io};

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use crossterm::style::Stylize;

use crate::address::AddressValid;
use crate::benchmarks::run_benchmarks;
use crate::hashcat::{Hashcat, HashcatExe, HashcatRunner};
use crate::logger::Logger;
use crate::passphrase::Passphrase;
use crate::seed::{Finished, Seed};

mod address;
mod benchmarks;
mod combination;
mod hashcat;
mod logger;
mod passphrase;
mod permutations;
mod seed;
mod tests;

const HASHCAT_PATH: &str = "hashcat";
const SEPARATOR: &str = ",";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, arg_required_else_help = true, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<CliCommand>,

    #[command(flatten)]
    pub run: Option<CliRun>,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Runs benchmarks and tests of the application
    Test(BenchOption),
}

#[derive(Args, Debug)]
#[group(required = true, multiple = true)]
pub struct BenchOption {
    /// Runs all checks for a release (equivalent to -t -b -p -d)
    #[arg(short = 'r', long, default_value_t = false)]
    release: bool,

    /// Runs integration tests
    #[arg(short = 't', long, default_value_t = false)]
    test: bool,

    /// Checks whether benchmarks are passing
    #[arg(short = 'p', long, default_value_t = false)]
    pass: bool,

    /// Runs benchmarks until exhaustion
    #[arg(short = 'b', long, default_value_t = false)]
    bench: bool,

    /// Diffs the output against benchmarks_<suffix>.txt file
    #[arg(short = 'd', long, value_name = "suffix")]
    diff: Option<String>,
}

#[derive(Args, Debug)]
pub struct CliRun {
    /// Address e.g. 'bc1q490...' OR master xpub key e.g. 'xpub661MyMwAqRbc...'
    #[arg(short, long, value_name = "address")]
    address: String,

    /// Seed words with wildcards e.g. 'cage,?,zo?,?be,?oo?,toward|st?,able...'
    #[arg(short, long, value_name = "word word...")]
    seed: String,

    /// Derivation paths with wildcards e.g. 'm/0/0,m/49h/0h/0h/?2/?10'
    #[arg(short, long, value_name = "path path...")]
    derivation: Option<String>,

    /// Dictionaries and/or mask e.g. './dict.txt' '?l?l?l?d?1'
    #[arg(short, long, value_name = "MASK|DICT", num_args = 1.., value_delimiter = ' ')]
    passphrase: Option<Vec<String>>,

    /// Guess all permutations of a # of seed words
    #[arg(short, long, value_name = "# words")]
    combinations: Option<usize>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '1', long, value_name = "chars")]
    custom_charset1: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '2', long, value_name = "chars")]
    custom_charset2: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '3', long, value_name = "chars")]
    custom_charset3: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '4', long, value_name = "chars")]
    custom_charset4: Option<String>,

    /// Skips the prompt and starts immediately
    #[arg(short = 'y', long, default_value_t = false)]
    skip_prompt: bool,

    /// Pass options directly to hashcat (https://hashcat.net/wiki/doku.php?id=hashcat)
    #[arg(last = true, value_name = "hashcat options")]
    hashcat: Vec<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let log = Logger::new();

    let cli: Cli = Cli::parse();
    if let Some(CliCommand::Test(option)) = cli.cmd {
        if let Err(err) = run_benchmarks(option).await {
            log.println_err(&err.to_string());
            exit(1);
        }
        exit(0);
    }

    if let Some(run) = cli.run {
        let mut hashcat = match configure(&run, &log) {
            Ok(hashcat) => hashcat,
            Err(err) => return log.println_err(&err.to_string()),
        };
        let (_, finished) = match hashcat.run(&log, false).await {
            Ok(finished) => finished,
            Err(err) => return log.println_err(&err.to_string()),
        };
        log_finished(&finished, &log);
    }
}

pub fn log_finished(finished: &Finished, log: &Logger) {
    match finished {
        Finished {
            seed: Some(seed),
            passphrase: Some(passphrase),
            ..
        } => {
            log.print("Found Seed: ".dark_green().bold());
            log.println(seed.as_str().stylize());
            if !passphrase.is_empty() {
                log.print("Found Passphrase: ".dark_green().bold());
                log.println(passphrase.as_str().stylize());
            }
        }
        _ => log.println_err("Exhausted search with no results...try with different parameters"),
    }
    log.println("".stylize());
}

pub fn configure(cli: &CliRun, log: &Logger) -> Result<Hashcat> {
    let exe = validate_exe()?;

    let seed_arg = cli.seed.clone();
    let seed = Seed::from_args(&seed_arg, &cli.combinations)?;
    seed.validate_length()?;

    let address = AddressValid::from_arg(&cli.address, &cli.derivation)?;

    let passphrase = match &cli.passphrase {
        None => None,
        Some(args) => {
            let charsets = vec![
                cli.custom_charset1.clone(),
                cli.custom_charset2.clone(),
                cli.custom_charset3.clone(),
                cli.custom_charset4.clone(),
            ];
            Some(Passphrase::from_arg(args, &charsets)?)
        }
    };

    log.heading("Seedcat Configuration");
    let format_address = format!("{} ({}) Address: ", address.kind.key, address.kind.name);
    log.print(format_address.as_str().bold());
    log.println(format!("{}\n", address.formatted).as_str().stylize());
    log.format_attempt("Derivations", &address.derivations);
    log.format_attempt("Seeds", &seed);
    if let Some(passphrase) = &passphrase {
        log.format_attempt("Passphrases", passphrase);
    }

    if seed.valid_seeds() == 0 {
        bail!("All possible seeds have invalid checksums")
    }
    let args = cli.hashcat.clone();
    let hashcat = Hashcat::new(exe, address.clone(), seed, passphrase, args);

    if hashcat.total() == u64::MAX {
        bail!("Exceeding 2^64 attempts will take forever to run, try reducing combinations");
    }
    log.print_num("Total Guesses: ", hashcat.total());

    let mode = hashcat.get_mode()?;
    match mode.runner {
        HashcatRunner::PureGpu => {
            log.print(" Pure GPU Mode: Can run on large GPU clusters\n".stylize())
        }
        HashcatRunner::BinaryCharsets(_, _) => log.print(
            " Pure GPU Mode: Can run on large GPU clusters (using binary charsets)\n".stylize(),
        ),
        HashcatRunner::StdinMaxHashes => log.print(
            " Stdin Mode: CPU-limited due to a large number of seeds to guess\n".dark_yellow(),
        ),
        HashcatRunner::StdinMinPassphrases => log.print(
            " Stdin Mode: CPU-limited due to not enough passphrases to guess\n".dark_yellow(),
        ),
    }
    if has_internet() {
        log.println(
            " Warning: For better security turn off your internet connection".dark_yellow(),
        );
    }

    if !cli.skip_prompt {
        prompt_continue(log);
    }

    log.heading("Seedcat Recovery");

    Ok(hashcat)
}

fn has_internet() -> bool {
    // See if we can connect to Google
    let socket = SocketAddr::from_str("209.85.233.101:80").expect("Valid socket");
    TcpStream::connect_timeout(&socket, Duration::from_millis(100)).is_ok()
}

fn prompt_continue(log: &Logger) {
    log.print("\nContinue with recovery [Y/n]? ".stylize());
    io::stdout().flush().unwrap();
    let mut line = String::new();
    let stdin = io::stdin();
    stdin.lock().read_line(&mut line).unwrap();
    if line.contains("n") {
        exit(0);
    }
}

fn validate_exe() -> Result<HashcatExe> {
    let platform = match env::consts::FAMILY {
        "unix" => "hashcat.bin",
        "windows" => "hashcat.exe",
        p => bail!("Unable to identify '{}' family", p),
    };

    let hashcat = Path::new(HASHCAT_PATH);
    for executable in [platform, "hashcat"] {
        if hashcat.join(executable).exists() {
            if let Ok(exe) = std::fs::canonicalize(hashcat.join(executable)) {
                return Ok(HashcatExe::new(exe));
            }
        }
    }
    bail!(
        "Could not find executable '{}'...make sure you are running in 'seedcat' directory",
        hashcat.join(platform).to_str().unwrap(),
    );
}
