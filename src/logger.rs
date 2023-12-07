use std::cmp::max;
use std::io::{stdout, Write};
use std::ops::Sub;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor::*;
use crossterm::style::StyledContent;
use crossterm::style::Stylize;
use crossterm::terminal::{Clear, ClearType};
use crossterm::ExecutableCommand;
use tokio::spawn;
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub trait Attempt {
    fn total(&self) -> u64;
    fn begin(&self) -> String;
    fn end(&self) -> String;
}

const MINUTE: u64 = 60;
const HOUR: u64 = MINUTE * 60;
const DAY: u64 = HOUR * 24;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Logger {
    is_logging: bool,
}

pub struct TableFormat {
    headings: Vec<String>,
    log: Logger,
}

impl TableFormat {
    pub fn log_heading(&self) {
        let headings = self.headings.join("\t|");
        self.log.println(headings.as_str().bold());
    }

    pub fn format(&self, row: Vec<String>) -> String {
        let mut padded = vec![];
        for i in 0..row.len() {
            let len = self.headings[i].len();
            padded.push(format!("{: <1$}", row[i], len));
        }
        padded.join("\t|")
    }

    pub fn log_row(&self, row: Vec<String>) {
        self.log.println(self.format(row).as_str().stylize());
    }
}

#[derive(Debug, Clone)]
pub struct Timer {
    name: String,
    oneliner: bool,
    total: Arc<AtomicU64>,
    end: Arc<AtomicU64>,
    counter: Arc<AtomicU64>,
    seconds: Arc<AtomicU64>,
    multiplier: u64,
    log: Logger,
}

impl Timer {
    pub fn seconds(&self) -> u64 {
        max(self.seconds.fetch_add(0, Ordering::Relaxed), 1)
    }

    pub fn count(&self) -> u64 {
        self.counter
            .fetch_add(0, Ordering::Relaxed)
            .saturating_mul(self.multiplier)
    }

    pub fn gpu_speed(&self) -> String {
        Logger::format_num(self.count() / self.seconds() / self.multiplier)
    }

    pub fn speed(&self) -> String {
        Logger::format_num(self.count() / self.seconds())
    }

    pub fn multiplier(&self) -> u64 {
        self.multiplier
    }

    pub fn add(&self, amt: u64) {
        self.counter.fetch_add(amt, Ordering::Relaxed);
    }

    pub fn store(&self, amt: u64) {
        self.counter.store(amt, Ordering::Relaxed);
    }

    pub fn end(&self) {
        self.end.store(1, Ordering::Relaxed);
    }

    pub async fn start(&self) -> JoinHandle<()> {
        self.start_at(0).await
    }

    pub async fn start_at(&self, secs: u64) -> JoinHandle<()> {
        let timer = self.clone();

        spawn(async move {
            let now = Instant::now().sub(Duration::from_secs(secs));
            let mut old_count = u64::MAX;
            let name = timer.name.as_str().bold();

            loop {
                sleep(Duration::from_millis(200)).await;
                let count = timer.count();
                let end = timer.end.fetch_add(0, Ordering::Relaxed);

                // Don't print if the count hasn't changed
                if count == old_count && end == 0 {
                    continue;
                }
                if end != 0 {
                    timer.total.store(count, Ordering::Relaxed);
                }
                let total = timer.total.fetch_add(0, Ordering::Relaxed);

                if !timer.oneliner && old_count == u64::MAX {
                    timer.log.println("\n\n\n\n\n".stylize());
                }

                old_count = count;
                timer
                    .seconds
                    .store(now.elapsed().as_secs(), Ordering::Relaxed);
                let seconds = timer.seconds();

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
                if count == total {
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

    pub fn format_time(mut remaining: u64) -> String {
        let mut output = vec![];

        if remaining / DAY > 0 {
            output.push(format!("{} days", remaining / DAY));
            remaining %= DAY;
        }
        if remaining / HOUR > 0 || output.len() > 0 {
            output.push(format!("{} hours", remaining / HOUR));
            remaining %= HOUR;
        }
        if remaining / MINUTE > 0 || output.len() > 0 {
            output.push(format!("{} mins", remaining / MINUTE));
            remaining %= MINUTE;
        }
        output.push(format!("{} secs", remaining));
        output.join(", ")
    }
}

impl Logger {
    pub fn new() -> Self {
        Self { is_logging: true }
    }

    pub fn off() -> Self {
        Self { is_logging: false }
    }

    pub fn table(&self, heading: Vec<&str>) -> TableFormat {
        TableFormat {
            headings: heading.iter().map(|s| s.to_string()).collect(),
            log: self.clone(),
        }
    }

    pub async fn time(&self, name: &str, total: u64) -> Timer {
        Timer {
            name: name.to_string(),
            oneliner: true,
            total: Arc::new(AtomicU64::new(total)),
            end: Arc::new(Default::default()),
            counter: Arc::new(Default::default()),
            seconds: Arc::new(Default::default()),
            multiplier: 1,
            log: self.clone(),
        }
    }

    pub async fn time_verbose(&self, name: &str, total: u64, multiplier: u64) -> Timer {
        Timer {
            name: name.to_string(),
            oneliner: false,
            total: Arc::new(AtomicU64::new(total)),
            end: Arc::new(Default::default()),
            counter: Arc::new(Default::default()),
            seconds: Arc::new(Default::default()),
            multiplier,
            log: self.clone(),
        }
    }

    pub fn heading(&self, output: &str) {
        self.print(
            format!("\n============ {} ============\n", output)
                .as_str()
                .dark_green()
                .bold(),
        )
    }

    pub fn print(&self, output: StyledContent<&str>) {
        let mut stdout = stdout();
        if self.is_logging {
            stdout.write_all(output.to_string().as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
    }

    pub fn println_err(&self, output: &str) {
        let mut split = output.split("\n");
        self.print("\nError: ".dark_red().bold());
        while let Some(line) = split.next() {
            self.println(line.stylize());
        }
    }

    pub fn println(&self, output: StyledContent<&str>) {
        let mut stdout = stdout();
        if self.is_logging {
            stdout.write_all(output.to_string().as_bytes()).unwrap();
            stdout.write_all("\n".to_string().as_bytes()).unwrap();
            stdout.flush().unwrap();
        }
    }

    pub fn format_attempt(&self, name: &str, attempt: &impl Attempt) {
        let total = format!("{}: ", name);
        self.print_num(&total, attempt.total());
        self.println(
            format!(" Begin: {}\n End:   {}\n", attempt.begin(), attempt.end())
                .as_str()
                .stylize(),
        );
    }

    pub fn print_num(&self, prefix: &str, thousands: u64) {
        self.print(prefix.bold());
        if thousands == u64::MAX {
            self.println("Exceeds 2^64".dark_red().bold());
        } else {
            let output = Logger::format_num(thousands);
            self.println(output.as_str().bold());
        }
    }

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
}

#[cfg(test)]
mod tests {
    use crate::logger::*;

    #[test]
    fn formats_tables() {
        let logger = Logger::new();
        let table = logger.table(vec!["a---", "b", "c--"]);
        let formatted = table.format(vec!["1".to_string(), "2".to_string(), "3".to_string()]);
        assert_eq!(formatted, "1   \t|2\t|3  ");
    }

    #[tokio::test]
    async fn timer_starts_and_ends() {
        let timer = Logger::off().time("", 100).await;
        let handle = timer.start().await;
        timer.add(100);
        handle.await.unwrap();
        assert_eq!(timer.total.fetch_add(0, Ordering::Relaxed), 100);
        assert_eq!(timer.counter.fetch_add(0, Ordering::Relaxed), 100);

        let timer = Logger::off().time("", 100).await;
        let handle = timer.start().await;
        timer.add(50);
        timer.end();
        handle.await.unwrap();
        assert_eq!(timer.total.fetch_add(0, Ordering::Relaxed), 50);
        assert_eq!(timer.counter.fetch_add(0, Ordering::Relaxed), 50);
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
