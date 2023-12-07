use crossterm::style::Stylize;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::combination::Combinations;
use crate::logger::{Attempt, Logger, Timer};
use crate::permutations::Permutations;
use crate::seed::{Finished, Seed};
use crate::tests::Test;
use crate::{log_finished, BenchOption};

static BENCH_COUNT: AtomicUsize = AtomicUsize::new(0);

struct Benchmark {
    name: String,
    args: String,
    timer: Option<Timer>,
    wall_time: u64,
    derivations: String,
    is_fast: bool,
}

impl Benchmark {
    fn new(name: &str, args: &str) -> Self {
        Self::with_derivations(name, "m/0/0", args)
    }

    fn with_derivations(name: &str, derivations: &str, args: &str) -> Self {
        Self {
            name: name.to_string(),
            args: args.to_string(),
            derivations: derivations.to_string(),
            timer: None,
            wall_time: 0,
            is_fast: false,
        }
    }

    fn with_fast(mut self) -> Self {
        self.is_fast = true;
        self
    }
}

pub async fn run_benchmarks(option: BenchOption) {
    let mut benchmarks = vec![];
    // dad moral begin apology cheap vast clerk limb shaft salt citizen awesome
    // aim twin nest escape combine lady grant ocean olympic post silent exist burger amateur physical muscle blossom series because dress cradle zone kick dove
    benchmarks.push(Benchmark::new("Master XPUB (mask attack)", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?l?d?d?d?d?d?d -a xpub661MyMwAqRbcEZjJh7cPj6aGJ9NpRDUfpNz65bLKQQKR6dznUoszbxGyF7JUeCCNdYyboeD9EnRGgz8UfZW2hMzMBXA7SLumhtMU8VWy65L").with_fast());
    benchmarks.push(Benchmark::with_derivations("1000 derivations (mask attack)", "m/0/?9h/?9/?9", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?l?d?d?d -a 18FkAx3zZNwmm6iTCcpHFxrrbs5sgKC6Wf"));
    benchmarks.push(Benchmark::with_derivations("100 derivations (mask attack)", "m/0/5h/?9/?9", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?l?d?d?d?d -a 1EtMfpkU1PyGnTYraoV2RrZEMgEijbRxLg").with_fast());
    benchmarks.push(Benchmark::with_derivations("10 derivations (mask attack)", "m/0/5h/5/?9", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?l?d?d?d?d?d -a 18dAXjq3NG5uVBxe1cpcwrxfvJxeDWy9oQ").with_fast());
    benchmarks.push(Benchmark::new("1 derivations (mask attack)", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?l?d?d?d?d?d?d -a 1A8nieZBBmXbwb4kvVpXBRdEpCaekiRhHH").with_fast());
    benchmarks.push(Benchmark::new("Missing first words of 12", "-s ?,?,begin,apology,cheap,v?,clerk,limb,shaft,salt,citizen,awesome -a 13PciouesvLmVAvmNxW4RhZyDkCGuqpwRY"));
    benchmarks.push(Benchmark::new("Missing first words of 24", "-s ?,?,nest,escape,combine,lady,grant,ocean,olympic,post,s?,exist,burger,amateur,physical,muscle,blossom,series,because,dress,cradle,zone,kick,dove -a 18qfTDrgRZa3ASKy6erJUCWLARaiFNyLty"));
    benchmarks.push(Benchmark::new("Permute 12 of 12 words", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,awesome,citizen -c 12 -a 13PciouesvLmVAvmNxW4RhZyDkCGuqpwRY"));
    benchmarks.push(Benchmark::new("Permute 12 of 24 words", "-s ^ai?,^twin,^nest,^escape,^combine,^lady,^grant,^ocean,^olympic,^post,^silent,^exist,burger,amateur,physical,muscle,blossom,series,because,dress,cradle,zone,dove,kick -c 24 -a 18qfTDrgRZa3ASKy6erJUCWLARaiFNyLty"));
    benchmarks.push(Benchmark::new("Missing last words of 12", "-s dad,moral,begin,apology,cheap,v?,clerk,limb,shaft,salt,?,? -a 13PciouesvLmVAvmNxW4RhZyDkCGuqpwRY").with_fast());
    benchmarks.push(Benchmark::new("Missing last words of 24", "-s aim,twin,nest,escape,combine,lady,grant,ocean,olympic,post,s?,exist,burger,amateur,physical,muscle,blossom,series,because,dress,cradle,zone,?,? -a 18qfTDrgRZa3ASKy6erJUCWLARaiFNyLty"));
    benchmarks.push(Benchmark::new("Passphrase dict attack", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ./dicts/10k.txt,~,./dicts/1k_upper.txt -a 17whoxEdasBPiEWKU1kjreNBaGBDzp2woS"));
    benchmarks.push(Benchmark::new("Passphrase dict+dict attack", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ./dicts/10k.txt,~ ./dicts/1k_upper.txt -a 17whoxEdasBPiEWKU1kjreNBaGBDzp2woS"));
    benchmarks.push(Benchmark::new("Passphrase dict+mask attack", "-s dad,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ./dicts/10k.txt ~?d?d?d -a 1CnKNvDUaEQ6ybR6GN56wBsPYKnFd3ZRDa"));
    benchmarks.push(Benchmark::new("Small passphrase + seed", "-s ?,?,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?d?d -a 1DrJAfW6TY6X3q6SBmZHAUddfodzEuz6Mg").with_fast());
    benchmarks.push(Benchmark::new("Large passphrase + seed", "-s ?,moral,begin,apology,cheap,vast,clerk,limb,shaft,salt,citizen,awesome -p ?d?d?d?d?d -a 1FRm26FwcVtnRe2q8fHdd9c11UEEH5EYUo"));

    let log = Logger::new();
    for benchmark in &mut benchmarks {
        if option.pass {
            let out = format!("\n\n\n\n\nRunning passing benchmark '{}'", benchmark.name);
            log.println(out.as_str().bold().dark_cyan());
            let finished = run_benchmark(benchmark, &log, false, false).await;
            assert!(finished.seed.is_some());
        }

        if option.slow || option.exhaust {
            let out = format!(
                "\n\n\n\n\nRunning exhausting benchmark '{}'",
                benchmark.name
            );
            log.println(out.as_str().bold().dark_cyan());
            let finished = run_benchmark(benchmark, &log, true, option.slow).await;
            assert!(finished.seed.is_none());
        }
    }

    let table = log.table(vec![
        "Benchmark Name              ",
        "Guesses  ",
        "Speed   ",
        "GPU Speed",
        "Time     ",
        "Wall Time",
    ]);
    table.log_heading();
    for benchmark in benchmarks {
        if let Some(timer) = benchmark.timer {
            let guesses = Logger::format_num(timer.count());
            let recovery_time = Timer::format_time(timer.seconds());
            let wall_time = Timer::format_time(benchmark.wall_time);
            table.log_row(vec![
                benchmark.name,
                guesses,
                timer.speed() + "/sec",
                timer.gpu_speed() + "/sec",
                recovery_time,
                wall_time,
            ]);
        }
    }
}

async fn run_benchmark(
    benchmark: &mut Benchmark,
    log: &Logger,
    exhaust: bool,
    slow: bool,
) -> Finished {
    let id = BENCH_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut derivation = benchmark.derivations.clone();
    let mut args = benchmark.args.clone();
    if slow && benchmark.is_fast {
        args = args.replace("?d ", "?d?d ");
        args = args.replace("v?", "?");
    }
    if exhaust {
        derivation = derivation.replace("m/0", "m/1");
        args = args.replace("awesome", "flower");
    }
    let name = format!("hc_bench{}", id);
    let args = format!("-d {} {}", derivation, args);
    let mut hashcat = Test::configure(&name, &args, &log);

    let now = Instant::now();
    let (timer, finished) = hashcat.run(&log).await.unwrap();
    benchmark.timer = Some(timer);
    benchmark.wall_time = now.elapsed().as_secs();
    log_finished(&finished, &log);
    finished
}

#[allow(dead_code)]
pub async fn benchmark_permutations() {
    let vec = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
    let choose = 10;
    let mut perm = Permutations::new(vec.clone(), choose);

    let mut set = JoinSet::new();
    let time = Instant::now();
    for mut p in perm.shard(100) {
        set.spawn(async move {
            let mut count = 0;
            while let Some(_) = p.next() {
                count += 1;
            }
            count
        });
    }
    let mut count = 0;
    while let Some(c) = set.join_next().await {
        count += c.unwrap();
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());

    let mut count = 0;
    while let Some(_) = perm.next() {
        count += 1;
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());
}

#[allow(dead_code)]
pub async fn benchmark_combinations1() {
    let path = PathBuf::from("dicts");
    let file1 = io::read_to_string(File::open(path.join("10k.txt")).unwrap()).unwrap();
    let file2 = io::read_to_string(File::open(path.join("100k.txt")).unwrap()).unwrap();
    let lines1: Vec<_> = file1.lines().map(|str| str.to_string()).collect();
    let lines2: Vec<_> = file2.lines().map(|str| str.to_string()).collect();
    let mut combinations = Combinations::new(vec![lines1, lines2]);
    while let Some(_) = combinations.next() {}
    // let log = Logger::new();
    // combinations.write_zip("/tmp/test.gz", &log).await.unwrap();
}

// ~1B permutations in ~3635ms
#[allow(dead_code)]
pub async fn benchmark_combinations2() {
    let mut list = vec![];
    let mut index = vec![];
    for i in 0..13 {
        list.push(vec![0]);
        index.push(i);
    }
    let mut combinations = Combinations::permute(list, index, 10);
    println!("Permutations: {}", combinations.permutations());
    println!("Estimated: {}", combinations.total());
    println!("Exact    : {}", combinations.estimate_total(u64::MAX));

    let mut set = JoinSet::new();
    let time = Instant::now();
    for mut p in combinations.shard(100) {
        set.spawn(async move {
            let mut count = 0;
            while let Some(_) = p.next() {
                count += 1;
            }
            count
        });
    }
    let mut count = 0;
    while let Some(c) = set.join_next().await {
        count += c.unwrap();
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());

    let time = Instant::now();
    let mut count = 0;
    while let Some(_) = combinations.next() {
        count += 1;
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());
}

// 800M
#[allow(dead_code)]
pub async fn benchmark_seed() {
    let seed = Seed::from_args(
        "music,eternal,upper,myth,slight,divide,voyage,afford,q?,e?,e?,e?,e?,abandon,zoo",
        &None,
    )
    .unwrap();
    println!("Total: {}", seed.total());

    let mut set = JoinSet::new();
    let time = Instant::now();
    for mut s in seed.shard_words(100) {
        set.spawn(async move {
            let mut count = 0;
            while let Some(_) = s.next_valid() {
                count += 1;
            }
            count
        });
    }
    let mut count = 0;
    while let Some(c) = set.join_next().await {
        count += c.unwrap();
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());
}

// 1B combinations in ~450ms
#[allow(dead_code)]
pub async fn benchmark_combinations3() {
    let mut list = vec![];
    for _ in 0..9 {
        list.push(vec![0; 10]);
    }
    let mut combinations = Combinations::permute(list, vec![], 9);
    println!("Permutations: {}", combinations.permutations());
    println!("Estimated: {}", combinations.total());
    println!("Exact    : {}", combinations.estimate_total(u64::MAX));

    let mut set = JoinSet::new();
    let time = Instant::now();
    for mut p in combinations.shard(100) {
        set.spawn(async move {
            let mut count = 0;
            while let Some(_) = p.next() {
                count += 1;
            }
            count
        });
    }
    let mut count = 0;
    while let Some(c) = set.join_next().await {
        count += c.unwrap();
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());

    let time = Instant::now();
    let mut count = 0;
    while let Some(_) = combinations.next() {
        count += 1;
    }
    println!("ITERATIONS: {}", count);
    println!("ELAPSED: {:?}", time.elapsed().as_millis());
}

// Generate dicts of popular words from https://norvig.com/ngrams/
#[allow(dead_code)]
pub fn dicts() {
    let root = PathBuf::from("dicts");
    for (count, name) in vec![10, 1000, 10_000, 100_000]
        .iter()
        .zip(vec!["test", "1k", "10k", "100k"])
    {
        for kind in vec!["", "_upper", "_cap"] {
            let path = root.join("norvig.com_ngrams_count_1w.txt");
            let raw = File::open(path).expect("File exists");
            let filename = format!("{}{}.txt", name, kind);
            let mut file = File::create(root.join(filename)).unwrap();
            let mut written = 0;

            for line in io::read_to_string(raw).unwrap().lines() {
                if written == *count {
                    continue;
                }

                let word: &str = line.split("\t").next().unwrap();
                if kind == "_upper" {
                    writeln!(file, "{}", word.to_uppercase()).unwrap();
                } else if kind == "_cap" {
                    let mut c = word.chars();
                    let upper: String = c.next().unwrap().to_uppercase().chain(c).collect();
                    writeln!(file, "{}", upper).unwrap();
                } else {
                    writeln!(file, "{}", word).unwrap();
                }

                written += 1;
            }
        }
    }
}
