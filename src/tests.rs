use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Result};
use clap::Parser;
use crossterm::style::Stylize;

use crate::hashcat::{Hashcat, HashcatRunner};
use crate::logger::Logger;
use crate::seed::Finished;
use crate::{configure, Cli};

static TEST_COUNT: AtomicUsize = AtomicUsize::new(0);

pub struct Test {
    args: String,
    expected: Finished,
    binary: bool,
}

impl Test {
    pub fn configure(prefix: &str, args: &str, log: &Logger) -> Hashcat {
        let mut args: Vec<_> = args.split(" ").into_iter().collect();
        args.insert(0, "");
        args.push("-y");
        args.push("--");

        let cli = Cli::parse_from(args).run.unwrap();
        let mut hashcat = configure(&cli, &log).unwrap();
        hashcat.set_prefix(prefix.to_string());
        hashcat
    }

    pub async fn run(&self) -> Result<()> {
        let id = TEST_COUNT.fetch_add(1, Ordering::Relaxed);
        let name = format!("hc_test{}", id);

        let log = Logger::new();
        let mut hashcat = Self::configure(&name, &self.args, &log);
        if self.expected.pure_gpu {
            hashcat.min_passphrases = 0;
        } else {
            hashcat.max_hashes = 0;
        }

        if self.binary {
            if !matches!(
                hashcat.get_mode().unwrap().runner,
                HashcatRunner::BinaryCharsets(_, _)
            ) {
                bail!("Expected binary mode for test '{}'", name);
            }
        }

        let run = hashcat.run(&log, false);
        let (_, result) = run.await.unwrap();
        if result != self.expected {
            bail!("{} Failed: {}\nExpected: {}", name, result, self.expected);
        } else {
            Ok(())
        }
    }
}

struct Tests {
    tests: Vec<Test>,
}

impl Tests {
    fn new() -> Self {
        Self { tests: vec![] }
    }

    fn test_both(&mut self, args: &str, expected: &str) {
        self.test(args, expected, false, false);
        self.test(args, expected, true, false);
    }

    fn test_stdin(&mut self, args: &str, expected: &str) {
        self.test(args, expected, false, false);
    }

    fn test_binary(&mut self, args: &str, expected: &str) {
        self.test(args, expected, true, true);
    }

    fn test(&mut self, args: &str, expected: &str, pure_gpu: bool, binary: bool) {
        let expected: Vec<_> = expected.split(" ").collect();
        self.tests.push(Test {
            args: args.to_string(),
            expected: Finished::new(expected[0], expected.get(1).unwrap_or(&""), pure_gpu),
            binary,
        })
    }
}

pub async fn run_tests() -> Result<()> {
    let mut tests = Tests::new();

    tests.test_binary("-a 1Mbe4MHF4awqg2cojz8LRJErKaKyoQjsiD -s harbor,?,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,?",
                     "harbor,acquire,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,pumpkin");

    tests.test_binary("-a 1HJVf7UhgHKhvMyKVuQMhzrvGQ9QSGUARQ -s harbor,a?,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,? -p hashcat",
                      "harbor,acquire,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,pumpkin hashcat");

    tests.test_binary("-a 1HJVf7UhgHKhvMyKVuQMhzrvGQ9QSGUARQ -s harbor,acquire,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,? -p hashca?l",
                      "harbor,acquire,clinic,index,mix,shoe,tube,awkward,food,acquire,sustain,pumpkin hashcat");

    tests.test_stdin("-a 1CFizqjfv4kGz4PbvMviXY84Z73D7PSdR1 -s zoo,survey,thought,^hill,^friend,^fatal,^fall,^amused,^pact,^ripple,^glance,^rural,hand -c 12",
                     "hand,thought,survey,hill,friend,fatal,fall,amused,pact,ripple,glance,rural");

    tests.test_stdin("-a 1Hh5BipqjUyFJXXynux6ReTdEN5vStpQvn -s ?,r?,weather,dish,swall?|zoo,water,mosquito,merry,icon,congress,blush,section",
                     "there,river,weather,dish,swallow,water,mosquito,merry,icon,congress,blush,section");

    tests.test_stdin("-a 39zQn8yBDmUHswRYUgjwwEe6y5b6wDTUTi -s skill,check,filter,camera,pond,oppose,lesson,delay,rare,prepare,oak,bring,tape,fancy,pulp,voyage,coil,spot,faculty,nominee,rough,stick,?,enter",
                     "skill,check,filter,camera,pond,oppose,lesson,delay,rare,prepare,oak,bring,tape,fancy,pulp,voyage,coil,spot,faculty,nominee,rough,stick,wide,enter");

    tests.test_stdin("-a bc1qscpdw0smafzpwe5s9kjfstq48p6vcz0n30sccs -s p?,stumble,print,mansion,occur,client,deposit,electric,dance,olive,stay,mom -d m/0/0/?99,m/84'/0'/?2'/0/?3",
                     "private,stumble,print,mansion,occur,client,deposit,electric,dance,olive,stay,mom");

    tests.test_binary("-a bc1qscpdw0smafzpwe5s9kjfstq48p6vcz0n30sccs -s private,stumble,print,mansion,occur,client,deposit,electric,dance,olive,stay,? -d m/0/0/?99|m/84'/0'/?2'/0/?3",
                     "private,stumble,print,mansion,occur,client,deposit,electric,dance,olive,stay,mom");

    tests.test_both("-a xpub661MyMwAqRbcF5snxLXxdet4WwyipbK6phjJdy5ViauCkTSjQc37zm6Gyyryq1aF8Uuj4Xub9Bh7LfQo8ZmNujZVczj1FVs1wMDWrnTym39 -s very,cart,matter,object,raise,predict,water,term,easy,play,?,earn -p hashca?2 -2 zt",
                     "very,cart,matter,object,raise,predict,water,term,easy,play,give,earn hashcat");

    tests.test_both("-a 1AeC6MA7U651BTVS5hWTGi5u9Z7tGtkE6y -s very,cart,matter,object,raise,predict,water,term,easy,play,give,earn -p ./dicts/test.txt,-,./dicts/test_cap.txt,- ./dicts/test.txt",
                    "very,cart,matter,object,raise,predict,water,term,easy,play,give,earn the-Of-and");

    tests.test_both("-a 1Gmu1iEtjmnhrB8svoFDiFjYsc4sqXuU7z -s very,cart,matter,object,raise,predict,water,term,easy,play,give,earn -p ./dicts/test.txt,-- ?d",
                    "very,cart,matter,object,raise,predict,water,term,easy,play,give,earn and--2");

    tests.test_both("-a 1Hv3dB4JyhDBwo1vDzPKKJZp4SpxaoES6L -s very,cart,matter,object,raise,predict,water,term,easy,play,give,earn -p ?d?d-- ./dicts/test.txt",
                    "very,cart,matter,object,raise,predict,water,term,easy,play,give,earn 12--the");

    tests.test_both("-a 18zpD3jMSrHAYoA1XcDLshXPJA46DocVNi -s very,cart,matter,object,raise,predict,water,term,easy,play,give,earn -p ?dmask?d",
                    "very,cart,matter,object,raise,predict,water,term,easy,play,give,earn 1mask2");

    let num = tests.tests.len();
    let mut passed = 0;
    for test in tests.tests.drain(..) {
        test.run().await?;
        passed += 1;
        let output = format!("{}/{} tests passed.", passed, num);
        println!("{}", output.as_str().dark_green());
    }
    Ok(())
}
