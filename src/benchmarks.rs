use std::fs::File;
use std::io;
use std::io::Write;
use std::path::PathBuf;

use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::combination::Combinations;
use crate::logger::Attempt;
use crate::permutations::Permutations;
use crate::seed::Seed;

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
            while let Some(next) = p.next() {
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
    while let Some(next) = perm.next() {
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
    let mut seed = Seed::from_args(
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
    for i in 0..9 {
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
    for (count, name) in vec![10, 1000, 10_000, 100_000].iter().zip(vec!["test", "1k", "10k", "100k"]) {
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

                let mut word: &str = line.split("\t").next().unwrap();
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
