use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{format_err, Error, Result};
use crossterm::style::Stylize;
use gzp::deflate::Gzip;
use gzp::par::compress::{ParCompress, ParCompressBuilder};
use gzp::ZWriter;
use tokio::spawn;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use crate::address::AddressValid;
use crate::logger::{Attempt, Logger, Timer};
use crate::passphrase::Passphrase;
use crate::seed::{Finished, Seed};

const DEFAULT_MAX_HASHES: u64 = 10_000_000;
const DEFAULT_MIN_PASSPHRASES: u64 = 10_000;
const HC_HASHES_FILE: &str = "_hashes.gz";
const HC_ERROR_FILE: &str = "_error.log";
const HC_OUTPUT_FILE: &str = "_output.log";
const CHANNEL_SIZE: usize = 100;
const SEED_TASKS: usize = 1000;
const STDIN_PASSPHRASE_MEM: usize = 10_000_000;
const STDIN_BUFFER_BYTES: usize = 1000;
const S_MODE_MAXIMUM: u64 = 100_000_000;

#[derive(Debug, Clone)]
pub struct HashcatExe {
    exe: PathBuf,
}

impl HashcatExe {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }

    fn cd_seedcat(&self) {
        let parent = self.exe.parent().expect("parent folder exists");
        let parent = parent.parent().expect("parent folder exists");
        env::set_current_dir(&parent).expect("can set dir");
    }

    fn cd_hashcat(&self) {
        let parent = self.exe.parent().expect("parent folder exists");
        env::set_current_dir(&parent).expect("can set dir");
    }

    fn command(&self) -> Command {
        Command::new(self.exe.clone())
    }
}

#[derive(Debug, Clone)]
pub struct HashcatMode {
    pub runner: HashcatRunner,
    pub passphrases: u64,
    pub hashes: u64,
}

impl HashcatMode {
    fn new(runner: HashcatRunner, passphrases: u64, hashes: u64) -> Self {
        Self {
            runner,
            passphrases,
            hashes,
        }
    }

    fn is_pure_gpu(&self) -> bool {
        match self.runner {
            HashcatRunner::StdinMaxHashes | HashcatRunner::StdinMinPassphrases => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HashcatRunner {
    PureGpu,
    BinaryCharsets(Seed, Passphrase),
    StdinMaxHashes,
    StdinMinPassphrases,
}

/// Helper for running hashcat
pub struct Hashcat {
    address: AddressValid,
    seed: Seed,
    passphrase: Option<Passphrase>,
    pub max_hashes: u64,
    pub min_passphrases: u64,
    exe: HashcatExe,
    prefix: String,
    hashcat_args: Vec<String>,
    total: u64,
}

impl Hashcat {
    pub fn new(
        exe: HashcatExe,
        address: AddressValid,
        seed: Seed,
        passphrase: Option<Passphrase>,
        hashcat_args: Vec<String>,
    ) -> Self {
        let mut total = seed.total();
        total = total.saturating_mul(address.derivations.total());
        if let Some(passphrase) = &passphrase {
            total = total.saturating_mul(passphrase.total());
        }

        Self {
            exe,
            address,
            seed,
            passphrase,
            max_hashes: DEFAULT_MAX_HASHES,
            prefix: "hc".to_string(),
            min_passphrases: DEFAULT_MIN_PASSPHRASES,
            hashcat_args,
            total,
        }
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn set_prefix(&mut self, prefix: String) {
        self.prefix = prefix;
    }

    pub fn get_mode(&self) -> Result<HashcatMode> {
        let total_derivations = self.address.derivations.args().len() as u64;
        let binary_charsets = self.seed.binary_charsets(self.max_hashes, &self.passphrase);
        if let Some((seed, passphrase)) = binary_charsets? {
            if passphrase.total() > self.min_passphrases {
                return Ok(HashcatMode::new(
                    HashcatRunner::BinaryCharsets(seed.clone(), passphrase.clone()),
                    passphrase.total(),
                    seed.total_args() * total_derivations,
                ));
            }
        }
        let derivations = self.address.derivations.args().len() as u64;
        let passphrases = match &self.passphrase {
            None => 0,
            Some(passphrase) => passphrase.total(),
        };

        let gpu_hashes = self.seed.valid_seeds() * derivations;
        let stdin_hashes = self.seed.total_args() * derivations;
        if gpu_hashes > self.max_hashes {
            let mode = HashcatMode::new(HashcatRunner::StdinMaxHashes, 0, stdin_hashes);
            return Ok(mode);
        }
        if passphrases < self.min_passphrases {
            let mode = HashcatMode::new(HashcatRunner::StdinMinPassphrases, 0, stdin_hashes);
            return Ok(mode);
        }
        let mode = HashcatMode::new(HashcatRunner::PureGpu, passphrases, gpu_hashes);
        Ok(mode)
    }

    pub async fn run(&mut self, log: &Logger) -> Result<(Timer, Finished)> {
        self.exe.cd_hashcat();
        let mut args = self.hashcat_args.clone();
        args.push(self.hashfile());

        let mut passphrase_args = vec![];
        if let Some(passphrase) = &self.passphrase {
            passphrase_args = passphrase.build_args(&self.prefix, log).await?;
        }

        let mode = self.get_mode()?;
        let is_pure_gpu = mode.is_pure_gpu();

        match mode.clone().runner {
            HashcatRunner::PureGpu => {
                for arg in &passphrase_args {
                    args.push(arg.clone());
                }
                self.seed = self.seed.with_pure_gpu(is_pure_gpu);
                let seed_rx = self.spawn_seed_senders().await;
                self.write_hashes(log, seed_rx, mode.hashes).await?;

                let child = self.spawn_hashcat(&args, mode);
                self.run_helper(child.stderr, child.stdout, log).await
            }
            HashcatRunner::BinaryCharsets(seed, passphrase) => {
                for arg in &passphrase.build_args(&self.prefix, log).await? {
                    args.push(arg.clone());
                }
                self.passphrase = Some(passphrase);
                self.seed = seed.with_pure_gpu(is_pure_gpu);
                let rx = Self::spawn_arg_sender(&self.seed).await;
                self.write_hashes(log, rx, mode.hashes).await?;

                let child = self.spawn_hashcat(&args, mode);
                self.run_helper(child.stderr, child.stdout, log).await
            }
            HashcatRunner::StdinMaxHashes | HashcatRunner::StdinMinPassphrases => {
                self.seed = self.seed.with_pure_gpu(is_pure_gpu);
                let seed_rx = self.spawn_seed_senders().await;
                let rx = Self::spawn_arg_sender(&self.seed).await;
                self.write_hashes(log, rx, mode.hashes).await?;

                let child = self.spawn_hashcat(&args, mode);
                let stdin = HashcatStdin::new(child.stdin, passphrase_args, &self.exe);
                spawn(Self::stdin_sender(self.prefix.clone(), stdin, seed_rx));

                self.run_helper(child.stderr, child.stdout, log).await
            }
        }
    }

    async fn run_helper(
        &self,
        stderr: Option<ChildStderr>,
        stdout: Option<ChildStdout>,
        log: &Logger,
    ) -> Result<(Timer, Finished)> {
        // multiplier is how many derivations and seeds are performed per hash
        let mut multiplier = self.seed.hash_ratio();
        multiplier *= self.address.derivations.hash_ratio();
        spawn(Self::run_stderr(stderr, self.file(HC_ERROR_FILE)?));
        let timer = log
            .time_verbose("Recovery Guesses", self.total(), multiplier as u64)
            .await;
        let result = self.run_stdout(stdout, log, &timer).await?;
        let found = self.seed.found(result)?;
        self.exe.cd_seedcat();
        Ok((timer, found))
    }

    fn hashfile(&self) -> String {
        format!("{}{}", self.prefix, HC_HASHES_FILE)
    }

    async fn write_hashes(
        &self,
        log: &Logger,
        mut receiver: Receiver<Vec<u8>>,
        total: u64,
    ) -> Result<()> {
        let timer = log.time("Writing Hashes", total).await;
        let timer_handle = timer.start().await;
        let hashfile = self.hashfile();
        let path = Path::new(&hashfile);
        let file = File::create(path).unwrap();
        let writer = BufWriter::new(file);
        let address = &self.address;

        let mut parz: ParCompress<Gzip> = ParCompressBuilder::new().from_writer(writer);
        let kind = address.kind.key.as_bytes();
        let separator = ":".as_bytes();
        let address = address.formatted.as_bytes();
        let newline = "\n".as_bytes();

        while let Some(seed) = receiver.recv().await {
            for derivation in self.address.derivations.args() {
                parz.write_all(kind).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(derivation.as_bytes()).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(&seed).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(address).map_err(Error::msg)?;
                parz.write_all(newline).map_err(Error::msg)?;
                timer.add(1);
            }
        }
        parz.finish().map_err(Error::msg)?;
        timer.end();
        timer_handle.await.map_err(Error::msg)
    }

    fn spawn_hashcat(&self, args: &Vec<String>, mode: HashcatMode) -> Child {
        let mut cmd = self.exe.command();
        cmd.arg("-m");
        cmd.arg("28510");
        cmd.arg("-w");
        cmd.arg("4");
        cmd.arg("--status");
        cmd.arg("--self-test-disable");
        cmd.arg("--status-timer");
        cmd.arg("1");

        // -S mode is faster if we have <10M passphrases
        if mode.is_pure_gpu() && mode.passphrases < S_MODE_MAXIMUM {
            let attack_mode = self
                .passphrase
                .clone()
                .map(|p| p.attack_mode)
                .unwrap_or_default();
            if attack_mode != 6 && attack_mode != 7 {
                cmd.arg("-S");
            }
        }
        for arg in args {
            cmd.arg(arg);
        }
        // println!("Running {:?}", cmd);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Could not start hashcat process")
    }

    async fn stdin_sender(prefix: String, mut stdin: HashcatStdin, mut rx: Receiver<Vec<u8>>) {
        if stdin.passphrase_args.is_empty() {
            while let Some(seed) = rx.recv().await {
                stdin.stdin_send(seed);
            }
        } else {
            let mut pass_buffer = vec![];
            while let Some(seed) = rx.recv().await {
                let mut pass_rx = Self::spawn_passphrases(&prefix, &stdin, &mut pass_buffer).await;
                for pass in &pass_buffer {
                    let mut input = seed.clone();
                    input.extend_from_slice(pass);
                    stdin.stdin_send(input);
                }
                while let Some(pass) = pass_rx.recv().await {
                    let mut input = seed.clone();
                    input.extend(pass);
                    stdin.stdin_send(input);
                }
            }
        }
        stdin.flush();
    }

    async fn spawn_passphrases(
        prefix: &str,
        stdin: &HashcatStdin,
        buffer: &mut Vec<Vec<u8>>,
    ) -> Receiver<Vec<u8>> {
        let (tx, mut rx) = channel(CHANNEL_SIZE);

        // all passphrases fit in memory
        let buffer_len = buffer.len();
        if buffer_len > 0 && buffer_len < STDIN_PASSPHRASE_MEM {
            return rx;
        }

        // spawn hashcat to stdout to generate passphrases
        let exe = stdin.exe.clone();
        let passphrase_args = stdin.passphrase_args.clone();
        let mut cmd = exe.command();
        cmd.arg("--stdout");
        cmd.arg("--session");
        cmd.arg(format!("{}stdout", prefix));

        for arg in passphrase_args {
            cmd.arg(arg);
        }
        spawn(async move {
            let child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Could not start hashcat process");
            let out = child.stdout.expect("Pipes stdout");

            let reader = BufReader::new(out);
            let mut num = 0;
            for read in reader.lines() {
                num += 1;
                if num > buffer_len {
                    if tx.send(read.unwrap().into_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        });

        // initialize buffer
        if buffer.len() == 0 {
            while let Some(rx) = rx.recv().await {
                if buffer.len() == STDIN_PASSPHRASE_MEM {
                    break;
                }
                buffer.push(rx);
            }
        }

        rx
    }

    fn file(&self, name: &str) -> Result<BufWriter<File>> {
        let name = self.prefix.clone() + name;
        let path = Path::new(&name);
        let file =
            File::create(path).map_err(|_| format_err!("Unable to create file '{}'", name))?;
        Ok(BufWriter::new(file))
    }

    async fn spawn_arg_sender(seed: &Seed) -> Receiver<Vec<u8>> {
        let (tx, rx) = channel(CHANNEL_SIZE);
        let mut seed = seed.clone();
        spawn(async move {
            while let Some(arg) = seed.next_arg() {
                tx.send(arg.into_bytes())
                    .await
                    .expect("Arg receiver stoppped");
            }
        });
        rx
    }

    async fn spawn_seed_senders(&self) -> Receiver<Vec<u8>> {
        let (tx, rx) = channel(CHANNEL_SIZE);
        for shard in self.seed.shard_words(SEED_TASKS) {
            spawn(Self::seed_sender(shard, tx.clone()));
        }
        rx
    }

    async fn seed_sender(mut seed: Seed, sender: Sender<Vec<u8>>) {
        while let Some(next) = seed.next_valid() {
            if sender.send(next).await.is_err() {
                // receiver thread was killed
                break;
            }
        }
    }

    async fn run_stdout(
        &self,
        out: Option<ChildStdout>,
        log: &Logger,
        timer: &Timer,
    ) -> Result<Option<String>> {
        let mut handle = None;

        let address = self.address.formatted.clone();
        let mut file = self.file(HC_OUTPUT_FILE)?;
        let out = out.expect("Pipes stdout");
        let address = format!("{}:", address);
        let reader = BufReader::new(out);
        log.println("Waiting for GPU initialization please be patient...".bold());
        for read in reader.lines() {
            let line = read.map_err(Error::from)?;
            if line.contains("* Device") && !line.contains("WARNING") && !line.contains("skipped") {
                log.println(line.as_str().stylize());
            } else if line.starts_with("Time.Started.....: ") && handle.is_none() {
                let num = line.split(" (").nth(1).unwrap();
                let num = num.split(" sec").nth(0).unwrap();
                let num = num.parse::<u64>().expect("is num");
                handle = Some(timer.start_at(num).await);
            } else if line.starts_with("Progress.........: ") {
                let num = line.split(": ").nth(1).unwrap();
                let num = num.split("/").nth(0).unwrap();
                let total = num.parse::<u64>().expect("is num");
                timer.store(total);
            } else if line.contains(&address) {
                timer.end();
                if let Some(handle) = handle {
                    handle.await.expect("Logging finishes");
                }
                return Ok(line.split(":").nth(1).map(ToString::to_string));
            }
            writeln!(file, "{}", line).map_err(Error::from)?;
            file.flush().map_err(Error::from)?;
        }
        timer.end();
        if let Some(handle) = handle {
            handle.await.expect("Logging finishes");
        }
        Ok(None)
    }

    async fn run_stderr(err: Option<ChildStderr>, mut file: BufWriter<File>) -> Result<()> {
        let err = err.expect("Piped stderr");
        let reader = BufReader::new(err);
        for read in reader.lines() {
            let line = read.map_err(Error::from)?;
            writeln!(file, "{}", line).map_err(Error::from)?;
            file.flush().map_err(Error::from)?;
        }
        Ok(())
    }
}

struct HashcatStdin {
    stdin: ChildStdin,
    stdin_buffer: Vec<u8>,
    passphrase_args: Vec<String>,
    exe: HashcatExe,
}

impl HashcatStdin {
    pub fn new(stdin: Option<ChildStdin>, passphrase_args: Vec<String>, exe: &HashcatExe) -> Self {
        Self {
            stdin: stdin.expect("Stdin piped"),
            stdin_buffer: vec![],
            passphrase_args,
            exe: exe.clone(),
        }
    }

    fn stdin_send(&mut self, pass: Vec<u8>) {
        self.stdin_buffer.extend(pass);
        self.stdin_buffer.push(10); // terminate password
        if self.stdin_buffer.len() > STDIN_BUFFER_BYTES {
            // might close when we find a match
            let _ = self.stdin.write_all(&self.stdin_buffer);
            self.stdin_buffer.clear();
        }
    }

    pub fn flush(&mut self) {
        // might close early due to success
        let _ = self.stdin.write_all(&self.stdin_buffer);
        let _ = self.stdin.flush();
    }
}

#[cfg(test)]
mod tests {
    use crate::hashcat::*;

    fn hashcat(passphrase: &str, seed: &str) -> Hashcat {
        let passphrase = Passphrase::from_arg(&vec![passphrase.to_string()], &vec![]).unwrap();
        let seed = Seed::from_args(seed, &None).unwrap();
        let derivation = Some("m/0/0".to_string());
        let address =
            AddressValid::from_arg("1B2hrNm7JGW6Wenf8oMvjWB3DPT9H9vAJ9", &derivation).unwrap();
        Hashcat::new(
            HashcatExe::new(PathBuf::new()),
            address,
            seed.clone(),
            Some(passphrase.clone()),
            vec![],
        )
    }

    #[test]
    fn determines_whether_to_run_pure_gpu() {
        let hc = hashcat("", "zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,?,?");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::BinaryCharsets(_, _)));
        assert_eq!(mode.hashes, 1);
        assert_eq!(mode.passphrases, 2048 * 2_u64.pow(7));

        let hc = hashcat("", "zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,z?,?");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::StdinMinPassphrases));
        assert_eq!(mode.hashes, 1);
        assert_eq!(mode.passphrases, 0);

        let hc = hashcat("?d?d?d?d", "zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,z?,?");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::BinaryCharsets(_, _)));
        assert_eq!(mode.hashes, 4); // number of z words
        assert_eq!(mode.passphrases, 10_000 * 2_u64.pow(7));

        let hc = hashcat("?d?d?d?d", "?,?,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::PureGpu));
        assert_eq!(mode.hashes, (2048 * 2048) / 16); // valid seeds estimate
        assert_eq!(mode.passphrases, 10_000);

        let hc = hashcat("?d?d", "?,?,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::StdinMinPassphrases));
        assert_eq!(mode.hashes, 1);
        assert_eq!(mode.passphrases, 0);

        let hc = hashcat("?d?d?d?d", "?,?,?,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo");
        let mode = hc.get_mode().unwrap();
        assert!(matches!(mode.runner, HashcatRunner::StdinMaxHashes));
        assert_eq!(mode.hashes, 1);
        assert_eq!(mode.passphrases, 0);
        assert_eq!(hc.total(), 10_000 * 2048 * 2048 * 2048);
    }
}
