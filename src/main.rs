use std::{env, io};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::exit;

use anyhow::{bail, format_err, Result};
use clap::Parser;
use crossterm::style::Stylize;

use crate::address::AddressValid;
use crate::hashcat::Hashcat;
use crate::logger::{Attempt, Logger};
use crate::passphrase::Passphrase;
use crate::seed::{Finished, Seed};
use crate::tests::run_tests;

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

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Address e.g. 'bc1q490...' OR master xpub key e.g. 'xpub661MyMwAqRbc...'
    #[arg(short, long, value_name = "ADDRESS")]
    address: Option<String>,

    /// Seed words with wildcards e.g. 'cage,?,zo?,?be,?oo?,toward|st?,able...'
    #[arg(short, long, value_name = "WORD WORD...")]
    seed: Option<String>,

    /// Derivation paths with wildcards e.g. 'm/0/0,m/49h/0h/0h/?2/?10'
    #[arg(short, long, value_name = "PATH PATH...")]
    derivation: Option<String>,

    /// Dictionaries and/or mask e.g. './dict.txt' '?l?l?l?d?1'
    #[arg(short, long, value_name = "MASK|DICT", num_args = 1.., value_delimiter = ' ')]
    passphrase: Option<Vec<String>>,

    /// Choose a number of combinations for the list of seed words
    #[arg(short, long, value_name = "# WORDS")]
    combinations: Option<usize>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '1', long, value_name = "CHARS")]
    custom_charset1: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '2', long, value_name = "CHARS")]
    custom_charset2: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '3', long, value_name = "CHARS")]
    custom_charset3: Option<String>,

    /// User defined charset for use in passphrase mask attack
    #[arg(short = '4', long, value_name = "CHARS")]
    custom_charset4: Option<String>,

    /// Skips the prompt and starts immediately
    #[arg(short = 'y', long, default_value_t = false)]
    skip_prompt: bool,

    /// Runs self-tests of the application
    #[arg(short = 't', long, default_value_t = false)]
    self_test: bool,

    /// Pass options directly to hashcat (https://hashcat.net/wiki/doku.php?id=hashcat)
    #[arg(last = true, value_name = "HASHCAT OPTIONS")]
    hashcat: Vec<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let cli: Cli = Cli::parse();
    if cli.self_test {
        run_tests().await;
        exit(0);
    }

    let logger = Logger::new();
    let mut hashcat = match configure(&cli, &logger) {
        Ok(hashcat) => hashcat,
        Err(err) => return logger.println_err(&err.to_string())
    };
    let finished = match hashcat.run(&cli.hashcat, &logger).await {
        Ok(finished) => finished,
        Err(err) => return logger.println_err(&err.to_string())
    };

    match finished {
        Finished {
            seed: Some(seed),
            passphrase: Some(passphrase),
            ..
        } => {
            logger.print("Found Seed: ".dark_green().bold());
            logger.println(seed.as_str().stylize());
            if !passphrase.is_empty() {
                logger.print("Found Passphrase: ".dark_green().bold());
                logger.println(passphrase.as_str().stylize());
            }
        }
        _ => logger.println_err("Exhausted search with no results...try with different parameters"),
    }
    logger.println("".stylize());
}

pub fn configure(cli: &Cli, log: &Logger) -> Result<Hashcat> {
    log.heading("Seedcat Configuration");
    let exe = validate_exe()?;

    let seed_arg = cli.seed.clone();
    let seed = seed_arg.ok_or(format_err!("--seed is a required argument"))?;
    let seed = Seed::from_args(&seed, &cli.combinations)?;
    seed.validate_length()?;

    let address = AddressValid::from_arg(cli.address.clone(), &cli.derivation)?;

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

    let format_address = format!("{} ({}) Address: ", address.kind.key, address.kind.name);
    log.print(format_address.as_str().bold());
    log.println(format!("{}\n", address.formatted).as_str().stylize());
    log.format_attempt("Derivations", &address.derivations);
    log.format_attempt("Seeds", &seed);
    if let Some(passphrase) = &passphrase {
        log.format_attempt("Passphrases", passphrase);
    }

    // log.print_num("Valid seeds: ", seed.valid_seeds());
    if seed.valid_seeds() == 0 {
        bail!("All possible seeds have invalid checksums")
    }
    let hashcat = Hashcat::new(exe, address.clone(), seed, passphrase);

    if hashcat.total() == u64::MAX {
        bail!("Exceeding 2^64 attempts will take forever to run, try reducing combinations");
    }
    log.print_num("Total Guesses: ", hashcat.total());

    if hashcat.uses_binary_charsets()? {
        log.print(
            "Pure GPU Mode: Can run on large GPU clusters (using binary charsets)\n".dark_green(),
        );
    } else if hashcat.pure_gpu()? {
        log.print("Pure GPU Mode: Can run on large GPU clusters\n".dark_green());
    } else if !hashcat.within_max_hashes() {
        log.print("Stdin Mode: CPU-limited due to many seeds to guess\n".dark_yellow());
    } else if !hashcat.has_enough_passphrases() {
        log.print("Stdin Mode: CPU-limited due to not enough passphrases to guess\n".dark_yellow());
    }
    if address.derivations.total() > 100 {
        log.println("Note: More than 100 derivations will slow status updates".dark_yellow())
    }

    if !cli.skip_prompt {
        prompt_continue();
    }

    log.heading("Seedcat Recovery");

    Ok(hashcat)
}

fn prompt_continue() {
    print!("\nContinue with recovery [Y/n]? ");
    io::stdout().flush().unwrap();
    let mut line = String::new();
    let stdin = io::stdin();
    stdin.lock().read_line(&mut line).unwrap();
    if line.contains("n") {
        exit(0);
    }
}

fn validate_exe() -> Result<PathBuf> {
    let platform = match env::consts::FAMILY {
        "unix" => "hashcat.bin",
        "windows" => "hashcat.exe",
        p => bail!("Unable to identify '{}' family", p),
    };

    let hashcat = Path::new(HASHCAT_PATH);
    for executable in [platform, "hashcat"] {
        if hashcat.join(executable).exists() {
            return Ok(hashcat.join(executable));
        }
    }
    bail!(
        "Could not find executable '{}' make sure you are running in the directory with the 'hashcat' folder",
        hashcat.join(platform).to_str().unwrap()
    );
}
