use std::cmp::max;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::Ordering;

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
use crate::logger::{Attempt, Logger};
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
const S_MODE_MAXIMUM: u64 = 10_000_000;

/// Helper for running hashcat
pub struct Hashcat {
    address: AddressValid,
    seed: Seed,
    passphrase: Option<Passphrase>,
    pub max_hashes: u64,
    pub min_passphrases: u64,
    exe: PathBuf,
    prefix: String,
}

impl Hashcat {
    pub fn new(
        exe: PathBuf,
        address: AddressValid,
        seed: Seed,
        passphrase: Option<Passphrase>,
    ) -> Self {
        Self {
            exe,
            address,
            seed,
            passphrase,
            max_hashes: DEFAULT_MAX_HASHES,
            prefix: "hc".to_string(),
            min_passphrases: DEFAULT_MIN_PASSPHRASES,
        }
    }

    pub fn total(&self) -> u64 {
        let mut total = self.seed.total();
        total = total.saturating_mul(self.address.derivations.total());
        if let Some(passphrase) = &self.passphrase {
            total = total.saturating_mul(passphrase.total());
        }
        total
    }

    pub fn set_prefix(&mut self, prefix: String) {
        self.prefix = prefix;
    }

    pub fn pure_gpu(&self) -> Result<bool> {
        Ok(self.uses_binary_charsets()?
            || (self.within_max_hashes() && self.has_enough_passphrases()))
    }

    /// If we have less than 10K passphrases GPU mode is much slower than Stdin
    pub fn has_enough_passphrases(&self) -> bool {
        self.total_passphrases() > self.min_passphrases
    }

    /// If we have more than 10M seeds GPU mode has too many hashes to load
    pub fn within_max_hashes(&self) -> bool {
        self.seed.valid_seeds() * self.address.derivations.total() < self.max_hashes
    }

    pub fn uses_binary_charsets(&self) -> Result<bool> {
        Ok(self
            .seed
            .binary_charsets(self.max_hashes, &self.passphrase)?
            .is_some())
    }

    pub fn total_passphrases(&self) -> u64 {
        match &self.passphrase {
            None => 0,
            Some(passphrase) => passphrase.total(),
        }
    }

    pub async fn run(&mut self, args: &Vec<String>, log: &Logger) -> Result<Finished> {
        let mut args = args.clone();
        args.push(self.hashfile());

        let mut passphrase_args = vec![];
        if let Some(passphrase) = &self.passphrase {
            passphrase_args = passphrase.build_args(&self.prefix, log).await?;
        }

        let is_pure_gpu = self.pure_gpu()?;

        if !is_pure_gpu {
            // Stdin mode
            self.seed = self.seed.with_pure_gpu(is_pure_gpu);
            let seed_rx = self.spawn_seed_senders().await;
            let rx = Self::spawn_arg_sender(&self.seed).await;
            self.write_hashes(log, rx, self.seed.total_args()).await?;

            let child = self.spawn_hashcat(&args, is_pure_gpu);
            let stdin = HashcatStdin::new(child.stdin, passphrase_args, &self.exe);
            spawn(Self::stdin_sender(self.prefix.clone(), stdin, seed_rx));

            self.run_helper(child.stderr, child.stdout, log).await
        } else if let Some((seed, passphrase)) = self
            .seed
            .binary_charsets(self.max_hashes, &self.passphrase)?
        {
            // GPU mode with binary charsets
            for arg in &passphrase.build_args(&self.prefix, log).await? {
                args.push(arg.clone());
            }
            self.passphrase = Some(passphrase);
            self.seed = seed.with_pure_gpu(is_pure_gpu);
            let rx = Self::spawn_arg_sender(&self.seed).await;
            self.write_hashes(log, rx, self.seed.total_args()).await?;

            let child = self.spawn_hashcat(&args, is_pure_gpu);
            self.run_helper(child.stderr, child.stdout, log).await
        } else {
            // GPU mode
            for arg in &passphrase_args {
                args.push(arg.clone());
            }
            self.seed = self.seed.with_pure_gpu(is_pure_gpu);
            let seed_rx = self.spawn_seed_senders().await;
            let valid_seeds = self.seed.valid_seeds();
            self.write_hashes(log, seed_rx, valid_seeds).await?;

            let child = self.spawn_hashcat(&args, is_pure_gpu);
            self.run_helper(child.stderr, child.stdout, log).await
        }
    }

    async fn run_helper(
        &self,
        stderr: Option<ChildStderr>,
        stdout: Option<ChildStdout>,
        log: &Logger,
    ) -> Result<Finished> {
        spawn(Self::run_stderr(stderr, self.file(HC_ERROR_FILE)?));
        let result = self.run_stdout(stdout, log).await?;
        let found = self.seed.found(result)?;
        Ok(found)
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
        let timer_handle = timer.start(log.clone()).await;
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
            for derivation in &self.address.derivations {
                parz.write_all(kind).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(derivation.as_bytes()).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(&seed).map_err(Error::msg)?;
                parz.write_all(separator).map_err(Error::msg)?;
                parz.write_all(address).map_err(Error::msg)?;
                parz.write_all(newline).map_err(Error::msg)?;
            }
            timer.counter.fetch_add(1, Ordering::Relaxed);
        }
        parz.finish().map_err(Error::msg)?;
        timer.counter.store(u64::MAX, Ordering::Relaxed);
        timer_handle.await.map_err(Error::msg)
    }

    fn spawn_hashcat(&self, args: &Vec<String>, is_pure_gpu: bool) -> Child {
        let mut cmd = Command::new(self.exe.as_os_str());
        cmd.arg("-m");
        cmd.arg("28510");
        cmd.arg("-w");
        cmd.arg("4");
        cmd.arg("--status");
        cmd.arg("--self-test-disable");
        cmd.arg("--status-timer");
        cmd.arg("1");

        // -S mode is faster if we have <10M passphrases
        if is_pure_gpu && self.total_passphrases() < S_MODE_MAXIMUM {
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
        let mut cmd = Command::new(exe.as_os_str());
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

    async fn run_stdout(&self, out: Option<ChildStdout>, log: &Logger) -> Result<Option<String>> {
        let seed_ratio = self.seed.total() / max(1, self.seed.valid_seeds());
        let timer = log
            .time_verbose("Recovering Bitcoin", self.total(), seed_ratio)
            .await;
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
                handle = Some(timer.start_at(log.clone(), num).await);
            } else if line.starts_with("Progress.........: ") {
                let num = line.split(": ").nth(1).unwrap();
                let num = num.split("/").nth(0).unwrap();
                let total = num.parse::<u64>().expect("is num");
                timer.counter.store(total, Ordering::Relaxed);
            } else if line.contains(&address) {
                timer.counter.store(u64::MAX, Ordering::Relaxed);
                if let Some(handle) = handle {
                    handle.await.expect("Logging finishes");
                }
                return Ok(line.split(":").nth(1).map(ToString::to_string));
            }
            writeln!(file, "{}", line).map_err(Error::from)?;
            file.flush().map_err(Error::from)?;
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
    exe: PathBuf,
}

impl HashcatStdin {
    pub fn new(stdin: Option<ChildStdin>, passphrase_args: Vec<String>, exe: &PathBuf) -> Self {
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
        self.stdin
            .write_all(&self.stdin_buffer)
            .expect("Stdin closed");
        self.stdin.flush().expect("Stdin closed");
    }
}

#[cfg(test)]
mod tests {
    use crate::address::address_kinds;
    use crate::hashcat::*;

    #[test]
    fn creates_total() {
        let derivations = vec!["1".to_string(), "2".to_string()];
        let passphrase = Passphrase::from_arg(&vec!["?l".to_string()], &vec![]).unwrap();
        let seed = Seed::from_args("?", &None).unwrap();
        let address = AddressValid::new("".to_string(), address_kinds()[0].clone(), derivations);
        let hc = Hashcat::new(PathBuf::new(), address, seed, Some(passphrase));

        assert_eq!(hc.total(), 2 * 2048 * 26);
    }

    #[test]
    fn determines_whether_to_run_pure_gpu() {
        let derivations = vec!["1".to_string(), "2".to_string()];
        let passphrase = Passphrase::from_arg(&vec!["?l".to_string()], &vec![]).unwrap();
        let seed = Seed::from_args("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,?,?", &None).unwrap();
        let address = AddressValid::new("".to_string(), address_kinds()[0].clone(), derivations);
        let mut hc = Hashcat::new(PathBuf::new(), address, seed, Some(passphrase));

        // 524288 = 2 derivations * 2048^2 seeds / 16 checksums
        hc.max_hashes = 524288;
        assert_eq!(hc.within_max_hashes(), false);

        hc.max_hashes = 524289;
        assert_eq!(hc.within_max_hashes(), true);
    }
}
