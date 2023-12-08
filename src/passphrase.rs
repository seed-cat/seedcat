use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::PathBuf;

use anyhow::{bail, format_err, Error, Result};

use crate::combination::Combinations;
use crate::logger::{Attempt, Logger};
use crate::{HASHCAT_PATH, SEPARATOR};

const ERR_MSG: &str = "\nPassphrase takes at most 2 args with the following possibilities:
  DICT attack:            --passphrase 'prefix,./dicts/dict.txt,suffix'
  MASK attack:            --passphrase 'prefix?l?l?l?d?d?1suffix'
  DICT DICT attack:       --passphrase './dicts/dict.txt,deliminator' './dicts/dict.txt'
  DICT DICT DICT attack:  --passphrase './dict1.txt,deliminator1,./dict2.txt,deliminator2' './dict3.txt'
  DICT MASK attack:       --passphrase './dict.txt' '?l?l?l?d?1'
  MASK DICT attack:       --passphrase '?l?l?l?d?1' './dict.txt'

  DICT files should be comma-separated relative paths starting with './' or deliminators
  MASK attacks should contain a mix of wildcards and normal characters
  To escape special characters '?' ',' '/' just double them, e.g. '??' ',,' '//'\n";

const MAX_DICT: u64 = 1_000_000_000;
const HC_LEFT_DICT: &str = "_left.gz";
const HC_RIGHT_DICT: &str = "_right.gz";

#[derive(Debug, Clone)]
pub struct Passphrase {
    pub attack_mode: usize,
    left: PassphraseArg,
    right: Option<PassphraseArg>,
    charsets: UserCharsets,
}

impl Attempt for Passphrase {
    fn total(&self) -> u64 {
        let mut total = 1;
        total *= Self::attempt(&self.left).total();
        if let Some(right) = &self.right {
            total *= Self::attempt(&right).total();
        }
        total
    }

    fn begin(&self) -> String {
        if let Some(right) = &self.right {
            return Self::attempt(&self.left).begin() + &Self::attempt(&right).begin();
        }
        Self::attempt(&self.left).begin()
    }

    fn end(&self) -> String {
        if let Some(right) = &self.right {
            return Self::attempt(&self.left).end() + &Self::attempt(&right).end();
        }
        Self::attempt(&self.left).end()
    }
}

impl Passphrase {
    pub fn empty_mask() -> Self {
        Self::new(
            3,
            vec![PassphraseArg::Mask(Mask::empty())],
            UserCharsets::empty(),
        )
    }

    fn new(attack_mode: usize, args: Vec<PassphraseArg>, charsets: UserCharsets) -> Self {
        let mut args = args.into_iter();
        Self {
            attack_mode,
            left: args.next().expect("at least one arg"),
            right: args.next(),
            charsets,
        }
    }

    fn attempt(arg: &PassphraseArg) -> Box<dyn Attempt> {
        match arg {
            PassphraseArg::Dict(d) => Box::new(d.clone()),
            PassphraseArg::Mask(m) => Box::new(m.clone()),
        }
    }

    pub async fn build_args(&self, prefix: &str, log: &Logger) -> Result<Vec<String>> {
        let mut result = vec![];
        result.push("-a".to_string());
        result.push(self.attack_mode.to_string());
        let dict = prefix.to_string() + HC_LEFT_DICT;
        result.push(Self::build_arg(&self.left, dict, log).await?);

        if let Some(right) = &self.right {
            let dict = prefix.to_string() + HC_RIGHT_DICT;
            result.push(Self::build_arg(right, dict, log).await?);
        }

        for charset in self.charsets.to_wildcards() {
            result.push(format!("-{}", charset.flag));
            result.push(charset.charset.unwrap().to_string());
        }

        Ok(result)
    }

    pub fn add_binary_charsets(&self, guesses: usize, entropy_bits: usize) -> Result<Option<Self>> {
        let mut copy = self.clone();

        let wildcards = copy.charsets.add_binary_charsets(entropy_bits)?;
        // Unable to generate the 3 wildcards required
        if wildcards.len() != 3 {
            return Ok(None);
        }
        if let PassphraseArg::Dict(d) = &self.left {
            if copy.right.is_none() {
                copy.right = Some(PassphraseArg::Dict(d.clone()));
                copy.left = PassphraseArg::Mask(Mask::empty());
                copy.attack_mode = 7;
            }
        }

        if let PassphraseArg::Mask(ref mut m) = &mut copy.left {
            m.prefix_wild(&wildcards[2]);
            for _ in 1..guesses {
                m.prefix_wild(&wildcards[1]);
                m.prefix_wild(&wildcards[0]);
            }

            return Ok(Some(copy));
        }
        Ok(None)
    }

    async fn build_arg(arg: &PassphraseArg, dictname: String, log: &Logger) -> Result<String> {
        Ok(match arg {
            PassphraseArg::Mask(m) => m.arg.clone(),
            PassphraseArg::Dict(d) => {
                let mut dict = d.clone();
                dict.combinations.write_zip(&dictname, log).await?;
                dictname
            }
        })
    }

    pub fn from_arg(args: &Vec<String>, charsets: &Vec<Option<String>>) -> Result<Passphrase> {
        let charsets = UserCharsets::new(charsets.clone())?;
        let mut parsed = vec![];
        for arg in args {
            parsed.push(Self::validate_arg(arg, &charsets)?);
        }

        let passphrase = match parsed[..] {
            [PassphraseArg::Mask(_)] => Passphrase::new(3, parsed, charsets),
            [PassphraseArg::Dict(_)] => Passphrase::new(0, parsed, charsets),
            [PassphraseArg::Dict(_), PassphraseArg::Dict(_)] => {
                Passphrase::new(1, parsed, charsets)
            }
            [PassphraseArg::Dict(_), PassphraseArg::Mask(_)] => {
                Passphrase::new(6, parsed, charsets)
            }
            [PassphraseArg::Mask(_), PassphraseArg::Dict(_)] => {
                Passphrase::new(7, parsed, charsets)
            }
            _ => bail!("Invalid passphrase args {:?}{}", args, ERR_MSG),
        };

        Ok(passphrase)
    }

    fn validate_arg(arg: &str, charsets: &UserCharsets) -> Result<PassphraseArg> {
        if arg.replace("??", "").contains("?") {
            Ok(PassphraseArg::Mask(Self::mask(arg, &charsets)?))
        } else {
            Ok(PassphraseArg::Dict(Self::dict(arg)?))
        }
    }

    fn dict(arg: &str) -> Result<Dictionary> {
        let mut combinations: Vec<Vec<String>> = vec![];
        for arg in arg.split(SEPARATOR) {
            if arg.starts_with("./") && !arg.starts_with(".//") {
                let path = PathBuf::from_iter(arg.split("/").into_iter());
                let err = format_err!("Failed to read file '{:?}'{}", path, ERR_MSG);
                let file = File::open(path).map_err(|_| err)?;
                let str = io::read_to_string(file).map_err(Error::msg)?;
                let bytes = str.lines().map(String::from).collect();
                combinations.push(bytes);
            } else if arg == "" {
                combinations.push(vec![",".to_string()]);
            } else {
                let replaced = arg.replace("??", "?").replace("//", "/");
                combinations.push(vec![replaced.to_string()]);
            }
        }
        Ok(Dictionary::new(combinations, arg)?)
    }

    fn mask(arg: &str, charsets: &UserCharsets) -> Result<Mask> {
        let arg = arg.replace("//", "/").replace(",,", ",");
        let mut example_start = vec![];
        let mut example_end = vec![];
        let wildcards = wildcards(charsets)?;
        let mut question = false;
        let mut combinations = 1_u64;
        for c in arg.chars() {
            if question {
                let wildcard = wildcards.get(&c).ok_or(Self::wildcard_err(c, &wildcards))?;
                example_start.push(wildcard.example_start.clone());
                example_end.push(wildcard.example_end.clone());
                combinations = combinations.saturating_mul(wildcard.length);
                question = false;
            } else if c == '?' {
                question = true;
            } else {
                example_start.push(c.to_string());
                example_end.push(c.to_string());
            }
        }
        if question {
            bail!("Mask '{}' ends in a ? use ?? to escape", arg);
        }
        Ok(Mask {
            arg,
            total: combinations,
            example_start: example_start.join(""),
            example_end: example_end.join(""),
        })
    }

    fn wildcard_err(unknown: char, wildcards: &BTreeMap<char, Wildcard>) -> Error {
        let mut valid = vec![];
        for (c, wildcard) in wildcards {
            valid.push(format!("  ?{} - {}", c, wildcard.display));
        }

        format_err!(
            "Wildcard '?{}' is unknown, valid wildcards are:\n{}",
            unknown,
            valid.join("\n")
        )
    }
}

#[derive(Clone, Debug)]
pub struct UserCharsets {
    charsets: BTreeMap<usize, Wildcard>,
}

impl UserCharsets {
    pub fn empty() -> Self {
        UserCharsets::new(vec![]).unwrap()
    }

    pub fn new(args: Vec<Option<String>>) -> Result<Self> {
        let mut charsets = BTreeMap::new();
        for i in 0..args.len() {
            if let Some(str) = &args[i] {
                let num = i + 1;
                charsets.insert(num, Wildcard::new_custom(num, str)?);
            }
        }

        Ok(Self { charsets })
    }

    pub fn add_binary_charsets(&mut self, entropy_bits: usize) -> Result<Vec<Wildcard>> {
        let mut bin = vec![entropy_bits, 6, 5];
        let mut totals = vec![2_u64.pow(entropy_bits as u32), 2_u64.pow(6), 2_u64.pow(5)];
        let mut wildcards = vec![];
        for i in 1..=4 {
            if self.charsets.contains_key(&i) {
                continue;
            }
            if let Some(bin) = bin.pop() {
                let total = totals.pop().unwrap();
                let wildcard = Wildcard::new_binary(i, bin, total)?;
                self.charsets.insert(i, wildcard.clone());
                wildcards.push(wildcard);
            }
        }
        Ok(wildcards)
    }

    pub fn to_wildcards(&self) -> Vec<Wildcard> {
        self.charsets.iter().map(|(_, v)| v.clone()).collect()
    }
}

#[derive(Debug, Clone)]
enum PassphraseArg {
    Dict(Dictionary),
    Mask(Mask),
}

#[derive(Debug, Clone)]
pub struct Dictionary {
    combinations: Combinations<String>,
}

impl Attempt for Dictionary {
    fn total(&self) -> u64 {
        self.combinations.total()
    }

    fn begin(&self) -> String {
        self.combinations.begin().join("")
    }

    fn end(&self) -> String {
        self.combinations.end().join("")
    }
}

impl Dictionary {
    pub fn new(vecs: Vec<Vec<String>>, arg: &str) -> Result<Self> {
        let combinations = Combinations::new(vecs);
        if combinations.total() > MAX_DICT {
            bail!(
                "Dictionaries '{}' exceed 1B combinations\n  Try splitting into 2 args or reducing size",
                arg
            );
        }
        Ok(Self { combinations })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Mask {
    pub arg: String,
    total: u64,
    example_start: String,
    example_end: String,
}

impl Attempt for Mask {
    fn total(&self) -> u64 {
        self.total
    }

    fn begin(&self) -> String {
        self.example_start.clone()
    }

    fn end(&self) -> String {
        self.example_end.clone()
    }
}

impl Mask {
    pub fn empty() -> Self {
        Self::new("", 1, "", "")
    }

    fn prefix_wild(&mut self, wildcard: &Wildcard) {
        self.total = self.total.saturating_mul(wildcard.length);
        self.arg = format!("?{}{}", wildcard.flag, self.arg);
    }

    fn new(arg: &str, total: u64, start: &str, end: &str) -> Self {
        Self {
            arg: arg.to_string(),
            total,
            example_start: start.to_string(),
            example_end: end.to_string(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Wildcard {
    flag: char,
    display: String,
    length: u64,
    example_start: String,
    example_end: String,
    charset: Option<String>,
}

impl Wildcard {
    fn new(flag: char, display: &str, length: u64) -> Self {
        Self {
            flag,
            display: display.to_string(),
            length,
            example_start: display.chars().next().unwrap().to_string(),
            example_end: display.chars().last().unwrap().to_string(),
            charset: None,
        }
    }

    fn new_chars(flag: char, display: &str, length: u64, start: &str, end: &str) -> Self {
        Self {
            flag,
            display: display.to_string(),
            length,
            example_start: start.to_string(),
            example_end: end.to_string(),
            charset: None,
        }
    }

    fn new_binary(num: usize, bin: usize, total: u64) -> Result<Self> {
        let root1 = PathBuf::new()
            .join(HASHCAT_PATH)
            .join("charsets")
            .join("bin");
        let root2 = PathBuf::new().join("charsets").join("bin");

        for root in [root1.clone(), root2] {
            let path = root.join(format!("{}bit.hcchr", bin));
            if path.exists() {
                let path = path.to_str().expect("Is valid path");
                return Ok(Self {
                    flag: num.to_string().chars().next().unwrap(),
                    display: path.to_string(),
                    length: total,
                    example_start: "".to_string(),
                    example_end: "".to_string(),
                    charset: Some(path.to_string()),
                });
            }
        }
        bail!("Could not find file '{:?}' make sure you are running in the directory with the '{}' folder", root1, HASHCAT_PATH);
    }

    fn new_custom(num: usize, display: &String) -> Result<Self> {
        if display.len() == 0 {
            bail!(
                "Custom charset {} is empty, pass in characters like so: -{} 'qwerty'",
                num,
                num
            );
        }
        Ok(Self {
            flag: num.to_string().chars().next().unwrap(),
            display: format!("Custom charset '{}'", display),
            length: display.len() as u64,
            example_start: display.chars().next().unwrap().to_string(),
            example_end: display.chars().last().unwrap().to_string(),
            charset: Some(display.to_string()),
        })
    }
}

fn wildcards(charsets: &UserCharsets) -> Result<BTreeMap<char, Wildcard>> {
    let mut wildcards = vec![
        Wildcard::new('l', "abcdefghijklmnopqrstuvwxyz", 26),
        Wildcard::new('u', "ABCDEFGHIJKLMNOPQRSTUVWXYZ", 26),
        Wildcard::new('d', "0123456789", 10),
        Wildcard::new('h', "0123456789abcdef", 16),
        Wildcard::new('H', "0123456789ABCDEF", 16),
        Wildcard::new_chars(
            's',
            "«space»!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
            33,
            " ",
            "~",
        ),
        Wildcard::new_chars('a', "?l?u?d?s", 95, "a", "~"),
        Wildcard::new_chars('b', "0x00 - 0xFF", 256, "0x00", "0xFF"),
        Wildcard::new_chars('?', "Escapes '?' character", 1, "?", "?"),
    ];
    wildcards.extend(charsets.to_wildcards());

    let mut map = BTreeMap::new();
    for wildcard in wildcards {
        map.insert(wildcard.flag, wildcard);
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use std::fs::remove_file;

    use crate::passphrase::*;

    #[tokio::test]
    async fn passphrase_generates_args() {
        let pp = Passphrase::from_arg(&vec!["?2".to_string()], &vec![None, Some("a".to_string())]);
        let args = pp.unwrap().build_args("hc", &Logger::off()).await;
        assert_eq!(args.unwrap(), vec!["-a", "3", "?2", "-2", "a"]);
    }

    fn bitfile(num: usize) -> String {
        let root = PathBuf::from_iter(vec!["hashcat", "charsets", "bin"].iter());
        let bit = root.join(format!("{}bit.hcchr", num));
        bit.into_os_string().into_string().unwrap()
    }

    #[tokio::test]
    async fn passphrase_can_add_binary_charsets() {
        let bit2 = bitfile(2);
        let bit3 = bitfile(3);
        let bit5 = bitfile(5);
        let bit6 = bitfile(6);

        let pp = Passphrase::from_arg(
            &vec!["test?d".to_string()],
            &vec![None, Some("a".to_string())],
        )
        .unwrap();
        let pp_with_bin = pp.add_binary_charsets(3, 2).unwrap().unwrap();
        assert_args(
            pp_with_bin.build_args("", &Logger::off()).await,
            &format!(
                "-a 3 ?1?3?1?3?4test?d -1 {} -2 a -3 {} -4 {}",
                bit5, bit6, bit2
            ),
        );
        assert_eq!(pp_with_bin.total(), 10 * 2048 * 2048 * 2_u64.pow(2));

        let pp = Passphrase::from_arg(&vec!["test".to_string()], &vec![]).unwrap();
        let pp_with_bin = pp.add_binary_charsets(3, 3).unwrap().unwrap();
        assert_args(
            pp_with_bin.build_args("hc", &Logger::off()).await,
            &format!(
                "-a 7 ?1?2?1?2?3 hc_right.gz -1 {} -2 {} -3 {}",
                bit5, bit6, bit3
            ),
        );
        assert_eq!(pp_with_bin.total(), 2048 * 2048 * 2_u64.pow(3));
        remove_file("hc_right.gz").unwrap();
    }

    fn assert_args(args: Result<Vec<String>>, expected: &str) {
        let expected: Vec<_> = expected.split(" ").collect();
        assert_eq!(args.unwrap(), expected);
    }

    #[test]
    fn can_add_binary_charsets() {
        let mut charsets = UserCharsets::new(vec![None, Some("a".to_string())]).unwrap();
        let wildcards = charsets.add_binary_charsets(2).unwrap();
        // Skips charset 2 because that's being used
        assert_eq!(
            wildcards,
            vec![
                Wildcard::new_binary(1, 5, 2_u64.pow(5)).unwrap(),
                Wildcard::new_binary(3, 6, 2_u64.pow(6)).unwrap(),
                Wildcard::new_binary(4, 2, 2_u64.pow(2)).unwrap()
            ]
        );
        let wildcards2 = charsets.add_binary_charsets(2).unwrap();
        // Empty vec because there is no more charset space
        assert_eq!(wildcards2, vec![]);

        // The binary file 10 doesn't exist
        assert!(UserCharsets::empty().add_binary_charsets(10).is_err());
    }

    #[test]
    fn validates_passphrase() {
        let pp = Passphrase::from_arg(&vec!["?2".to_string()], &vec![None, Some("a".to_string())]);
        assert_eq!(pp.unwrap().attack_mode, 3);

        let pp = Passphrase::from_arg(&vec!["asdf".to_string()], &vec![]);
        assert_eq!(pp.unwrap().attack_mode, 0);

        let pp = Passphrase::from_arg(&vec!["asdf".to_string(), "asdf".to_string()], &vec![]);
        assert_eq!(pp.unwrap().attack_mode, 1);

        let pp = Passphrase::from_arg(&vec!["?l".to_string(), "asdf".to_string()], &vec![]);
        assert_eq!(pp.unwrap().attack_mode, 7);

        let pp = Passphrase::from_arg(&vec!["asdf".to_string(), "?l".to_string()], &vec![]);
        assert_eq!(pp.unwrap().attack_mode, 6);

        let pp = Passphrase::from_arg(&vec!["?l".to_string(), "?l".to_string()], &vec![]);
        assert!(pp.is_err());
    }

    #[test]
    fn validates_dicts() {
        let dict = Passphrase::dict("a,./dicts/10k.txt,,./dicts/10k_upper.txt,b").unwrap();
        assert_eq!(dict.total(), 10_000 * 10_000);
        assert_eq!(dict.begin(), "athe,THEb".to_string());
        assert_eq!(dict.end(), "apoison,POISONb".to_string());

        let dict =
            Passphrase::dict("./dicts/1k.txt,.//,./dicts/1k_cap.txt,??,./dicts/1k_upper.txt")
                .unwrap();
        assert_eq!(dict.total(), 1000 * 1000 * 1000);
        assert_eq!(dict.begin(), "the./The?THE".to_string());
        assert_eq!(dict.end(), "entry./Entry?ENTRY".to_string());

        assert!(Passphrase::dict("./dicts/asdf.txt").is_err());
        assert!(Passphrase::dict("./dicts/100k.txt,./dicts/100k_cap.txt").is_err());
    }

    fn charsets(chars: Vec<&str>) -> UserCharsets {
        let mut charsets = vec![];
        for char in chars {
            if char.is_empty() {
                charsets.push(None);
            } else {
                charsets.push(Some(char.to_string()));
            }
        }
        UserCharsets::new(charsets).unwrap()
    }

    #[test]
    fn validates_masks() {
        let mask = Passphrase::mask("a?l//?d ??", &charsets(vec![])).unwrap();
        assert_eq!(mask, Mask::new("a?l/?d ??", 260, "aa/0 ?", "az/9 ?"));

        let mask = Passphrase::mask("?b,,?1", &charsets(vec!["qwerty"])).unwrap();
        assert_eq!(mask, Mask::new("?b,?1", 256 * 6, "0x00,q", "0xFF,y"));

        let mask = Passphrase::mask("?H ?2", &charsets(vec!["", "ab"])).unwrap();
        assert_eq!(mask, Mask::new("?H ?2", 16 * 2, "0 a", "F b"));

        assert!(Passphrase::mask("?H ?2", &charsets(vec!["ab"])).is_err());
        assert!(Passphrase::mask("?l?", &charsets(vec![])).is_err());
    }
}
