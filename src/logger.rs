use std::cmp::max;
use std::io::{stdout, Write};
use std::ops::Sub;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossterm::cursor::*;
use crossterm::style::StyledContent;
use crossterm::style::Stylize;
use crossterm::terminal::{Clear, ClearType};
use crossterm::ExecutableCommand;
use tokio::spawn;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Trait that can be logged for configuration purposes
pub trait Attempt {
    fn total(&self) -> u64;
    fn begin(&self) -> String;
    fn end(&self) -> String;
}

const MINUTE: u64 = 60;
const HOUR: u64 = MINUTE * 60;
const DAY: u64 = HOUR * 24;

/// Logger that can be either off or on
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Logger {
    is_logging: bool,
}

/// Formats table headings and rows
pub struct TableFormat {
    headings: Vec<String>,
    log: Logger,
}

impl TableFormat {
    /// Logs the table heading
    pub fn log_heading(&self) {
        let headings = self.headings.join("|");
        self.log.println(headings.as_str().bold());
    }

    fn format(&self, row: Vec<String>) -> String {
        let mut padded = vec![];
        for i in 0..row.len() {
            let len = self.headings[i].len();
            padded.push(format!("{: <1$}", row[i], len));
        }
        padded.join("|")
    }

    /// Logs the row containing same number of strings as heading
    pub fn log_row(&self, row: Vec<String>) {
        self.log.println(self.format(row).as_str().stylize());
    }
}

/// Periodically logs the time and progress of a task
#[derive(Debug, Clone)]
pub struct Timer {
    name: String,
    oneliner: bool,
    total: Arc<AtomicU64>,
    end: Arc<AtomicU64>,
    counter: Arc<AtomicU64>,
    seconds: Arc<AtomicU64>,
    last_speed: Arc<AtomicU64>,
    multiplier: u64,
    log: Logger,
}

impl Timer {
    /// Get the number of seconds elapsed
    pub fn seconds(&self) -> u64 {
        max(self.seconds.fetch_add(0, Ordering::Relaxed), 1)
    }

    /// Get the current count
    pub fn count(&self) -> u64 {
        self.counter
            .fetch_add(0, Ordering::Relaxed)
            .saturating_mul(self.multiplier)
    }

    /// Get the speed string without the multiplier
    pub fn gpu_speed(&self) -> String {
        Logger::format_num(self.count() / self.seconds() / self.multiplier)
    }

    /// Get the speed string
    pub fn speed(&self) -> String {
        Logger::format_num(self.count() / self.seconds())
    }

    /// Add to the count
    pub fn add(&self, amt: u64) {
        self.counter.fetch_add(amt, Ordering::Relaxed);
    }

    /// Store the count
    pub fn store(&self, amt: u64) {
        self.counter.store(amt, Ordering::Relaxed);
    }

    /// Tell the timer loop to end
    pub fn end(&self) {
        self.end.store(1, Ordering::Relaxed);
    }

    /// Start the timer
    pub async fn start(&self) -> JoinHandle<()> {
        self.start_at(0).await
    }

    /// Start the timer with secs already elapsed
    pub async fn start_at(&self, secs: u64) -> JoinHandle<()> {
        let timer = self.clone();

        spawn(async move {
            let now = Instant::now().sub(Duration::from_secs(secs));
            let mut old_count = u64::MAX;
            let name = timer.name.as_str().bold();

            loop {
                sleep(Duration::from_millis(100)).await;
                let count = timer.count();
                let end = timer.end.fetch_add(0, Ordering::Relaxed);

                // Don't print if the count hasn't changed
                if count == old_count && end == 0 {
                    continue;
                }
                let total = timer.total.fetch_add(0, Ordering::Relaxed);

                if !timer.oneliner && old_count == u64::MAX {
                    timer.log.println("\n\n\n\n\n".stylize());
                }

                timer
                    .seconds
                    .store(now.elapsed().as_secs(), Ordering::Relaxed);
                let seconds = timer.seconds();
                timer
                    .last_speed
                    .store(old_count / seconds, Ordering::Relaxed);
                old_count = count;

                let mut percent = (count as f64 / total as f64) * 100.0;
                if percent > 100.0 {
                    percent = 100.0;
                }
                let count_str = Logger::format_num(count);
                let total_str = Logger::format_num(total);
                let speed = format!("Speed....: {}/sec", timer.speed());
                let gpu = format!("GPU Speed: {}/sec", timer.gpu_speed());
                let progress = format!(" {:.2}% ({}/{})", percent, count_str, total_str);
                let eta = format!("ETA......: {}", Self::format_eta(percent, seconds));
                let elapsed = format!("Elapsed..: {}", Self::format_time(seconds));
                let output = format!(
                    "\n Progress:{}\n {}\n {}\n {}\n {}",
                    progress, speed, gpu, eta, elapsed
                );

                let mut stdout = stdout();
                if timer.log.is_logging && timer.oneliner {
                    stdout.execute(MoveLeft(1000)).unwrap();
                    stdout.execute(Clear(ClearType::FromCursorDown)).unwrap();
                    stdout.write_all(name.to_string().as_bytes()).unwrap();
                    stdout.write_all(progress.to_string().as_bytes()).unwrap();
                    stdout.flush().unwrap();
                } else if timer.log.is_logging {
                    stdout.execute(MoveLeft(1000)).unwrap();
                    stdout.execute(MoveUp(6)).unwrap();
                    stdout.execute(Clear(ClearType::FromCursorDown)).unwrap();
                    stdout.write_all("\n".as_bytes()).unwrap();
                    stdout.write_all(name.to_string().as_bytes()).unwrap();
                    stdout.write_all(output.to_string().as_bytes()).unwrap();
                    stdout.flush().unwrap();
                }
                if count >= total || end != 0 {
                    timer.log.println("\n".stylize());
                    break;
                }
            }
        })
    }

    fn format_eta(percent: f64, secs: u64) -> String {
        if percent == 100.0 {
            return "N/A".to_string();
        }
        if percent.is_nan() || percent == 0.0 {
            return "Unknown".to_string();
        }
        let remaining = (secs as f64 * (100.0 / percent)) as u64;
        if remaining <= secs {
            return "Unknown".to_string();
        }
        Self::format_time(remaining - secs)
    }

    /// Formats time duration in seconds
    pub fn format_time(mut seconds: u64) -> String {
        let mut output = vec![];

        if seconds / DAY > 0 {
            output.push(format!("{} days", seconds / DAY));
            seconds %= DAY;
        }
        if seconds / HOUR > 0 || output.len() > 0 {
            output.push(format!("{} hours", seconds / HOUR));
            seconds %= HOUR;
        }
        if seconds / MINUTE > 0 || output.len() > 0 {
            output.push(format!("{} mins", seconds / MINUTE));
            seconds %= MINUTE;
        }
        output.push(format!("{} secs", seconds));
        output.join(", ")
    }
}

impl Logger {
    /// Create logger that logs
    pub fn new() -> Self {
        Self { is_logging: true }
    }

    /// Create logger that doesn't log
    pub fn off() -> Self {
        Self { is_logging: false }
    }

    /// Create a new table logger, columns will be padded to heading length
    pub fn table(&self, heading: Vec<&str>) -> TableFormat {
        TableFormat {
            headings: heading.iter().map(|s| s.to_string()).collect(),
            log: self.clone(),
        }
    }

    /// Create a new timer logger
    pub async fn time(&self, name: &str, total: u64) -> Timer {
        Timer {
            name: name.to_string(),
            oneliner: true,
            total: Arc::new(AtomicU64::new(total)),
            end: Arc::new(Default::default()),
            counter: Arc::new(Default::default()),
            seconds: Arc::new(Default::default()),
            last_speed: Arc::new(Default::default()),
            multiplier: 1,
            log: self.clone(),
        }
    }

    /// Create a new timer logger in verbose mode
    /// `multiplier` will multiply the count (not the total)
    pub async fn time_verbose(&self, name: &str, total: u64, multiplier: u64) -> Timer {
        Timer {
            name: name.to_string(),
            oneliner: false,
            total: Arc::new(AtomicU64::new(total)),
            end: Arc::new(Default::default()),
            counter: Arc::new(Default::default()),
            seconds: Arc::new(Default::default()),
            last_speed: Arc::new(Default::default()),
            multiplier,
            log: self.clone(),
        }
    }

    /// Log a heading
    pub fn heading(&self, output: &str) {
        self.print(
            format!("\n============ {} ============\n", output)
                .as_str()
                .dark_green()
                .bold(),
        )
    }

    /// Print stylized text
    pub fn print(&self, output: StyledContent<&str>) {
        let mut stdout = stdout();
        if self.is_logging {
            stdout.write_all(output.to_string().as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
    }

    /// Print error text
    pub fn println_err(&self, output: &str) {
        let mut split = output.split("\n");
        self.print("\nError: ".dark_red().bold());
        while let Some(line) = split.next() {
            self.println(line.stylize());
        }
    }

    /// Println stylized text
    pub fn println(&self, output: StyledContent<&str>) {
        let mut stdout = stdout();
        if self.is_logging {
            stdout.write_all(output.to_string().as_bytes()).unwrap();
            stdout.write_all("\n".to_string().as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
    }

    /// Log an Attempt (begin, end, total)
    pub fn format_attempt(&self, name: &str, attempt: &impl Attempt) {
        let total = format!("{}: ", name);
        self.print_num(&total, attempt.total());
        self.println(
            format!(" Begin: {}\n End:   {}\n", attempt.begin(), attempt.end())
                .as_str()
                .stylize(),
        );
    }

    /// Log a number
    pub fn print_num(&self, prefix: &str, thousands: u64) {
        self.print(prefix.bold());
        if thousands == u64::MAX {
            self.println("Exceeds 2^64".dark_red().bold());
        } else {
            let output = Logger::format_num(thousands);
            self.println(output.as_str().bold());
        }
    }

    /// Format a number
    pub fn format_num(num: u64) -> String {
        let mut thousands = num as f64;
        let mut denomination = "";
        let denominations = vec!["", "K", "M", "B", "T"];
        for i in 0..denominations.len() {
            denomination = denominations[i];
            if i == denominations.len() - 1 || thousands < 1000.0 {
                break;
            }
            thousands /= 1000.0;
        }
        if denomination.is_empty() || thousands >= 100.0 {
            format!("{:.0}{}", thousands, denomination)
        } else if thousands >= 10.0 {
            format!("{:.1}{}", thousands, denomination)
        } else {
            format!("{:.2}{}", thousands, denomination)
        }
    }

    /// Parse a formatted number
    pub fn parse_num(str: &str) -> Result<u64> {
        let denominations = vec!["", "K", "M", "B", "T"];
        let mut thousands = 1_000_000_000_000_f64;
        for denom in denominations.iter().rev() {
            if str.contains(denom) {
                break;
            }
            thousands /= 1000.0;
        }
        let digits = str.chars().filter(|c| c.is_ascii_digit() || *c == '.');
        match digits.collect::<String>().parse::<f64>() {
            Ok(num) => Ok((num * thousands) as u64),
            Err(_) => bail!("Unable to parse num from '{}'", str),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::logger::*;

    #[test]
    fn parses_numbers() {
        assert_eq!(Logger::parse_num(" 1.2M ").unwrap(), 1_200_000);
        assert_eq!(Logger::parse_num(" 2.34K/sec ").unwrap(), 2340);
        assert_eq!(Logger::parse_num(" 123 ").unwrap(), 123);
    }

    #[test]
    fn formats_tables() {
        let logger = Logger::new();
        let table = logger.table(vec!["a---", "b", "c--"]);
        let formatted = table.format(vec!["1".to_string(), "2".to_string(), "3".to_string()]);
        assert_eq!(formatted, "1   |2|3  ");
    }

    #[tokio::test]
    async fn timer_starts_and_ends() {
        let timer = Logger::off().time("", 100).await;
        let handle = timer.start().await;
        timer.add(99);
        timer.add(1);
        handle.await.unwrap();
        assert_eq!(timer.count(), 100);

        let timer = Logger::off().time_verbose("", 100, 10).await;
        let handle = timer.start().await;
        timer.add(50);
        timer.end();
        handle.await.unwrap();
        assert_eq!(timer.count(), 500);
    }

    #[test]
    fn formats_nums() {
        assert_eq!(Logger::format_num(123), "123");
        assert_eq!(Logger::format_num(1230), "1.23K");
        assert_eq!(Logger::format_num(12300), "12.3K");
        assert_eq!(Logger::format_num(123000), "123K");
        assert_eq!(Logger::format_num(56_700_000), "56.7M");
        assert_eq!(Logger::format_num(56_700_000_000), "56.7B");
        assert_eq!(Logger::format_num(56_700_000_000_000), "56.7T");
    }

    #[test]
    fn formats_eta() {
        assert_eq!(Timer::format_eta(50.0, 60), "1 mins, 0 secs");
        assert_eq!(
            Timer::format_eta(0.00001, 1),
            "115 days, 17 hours, 46 mins, 39 secs"
        );
    }
}
