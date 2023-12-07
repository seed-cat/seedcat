use std::cmp::max;
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::string::ToString;

use anyhow::{bail, format_err, Result};
use sha2::digest::FixedOutputReset;
use sha2::{Digest, Sha256};

use crate::combination::Combinations;
use crate::logger::Attempt;
use crate::passphrase::Passphrase;
use crate::SEPARATOR;

const NUM_WORDS: usize = 2048;
const BIP39_BYTE_OFFSET: u8 = 48;
const EXACT_VALID_MAX: u64 = 100_000;
const VALID_LENGTHS: [usize; 5] = [12, 15, 18, 21, 24];

const ERR_MSG: &str = "\nSeed takes 1 arg with comma or space-separated values:
 Unknown word:    '?' expands into all possible 2048 words
 Unknown suffix:  'zo?' expands into 'zone|zoo'
 Unknown prefix:  '?ppy' expands into 'happy|puppy|unhappy'
 Unknown both:    '?orro?' expands into 'borrow|horror|tomorrow'
 Multiple words:  'puppy|zo?' expands into 'puppy|zone|zoo'
 Anchor word:     '^able' when using --combinations this word stays in place
                   (wildcards may also be used in anchored words e.g. '^s?')

 Putting together 12 words: '?,wa?,?kin,?kul?,pass|arr?|zoo,vague,^?ug,^flight,^wolf,^demise,?,?'";

#[derive(Debug, Clone)]
pub struct Seed {
    words: Combinations<u32>,
    encoder: SeedEncoder,
    args: Combinations<String>,
}

impl Attempt for Seed {
    fn total(&self) -> u64 {
        self.words.total()
    }

    fn begin(&self) -> String {
        Self::to_words(&self.words.begin())
    }

    fn end(&self) -> String {
        Self::to_words(&self.words.end())
    }
}

impl Seed {
    #[allow(dead_code)]
    fn from_arg(arg: &str) -> Result<Self> {
        Self::from_args(arg, &None)
    }

    #[allow(dead_code)]
    fn from_combo(arg: &str, combo_arg: usize) -> Result<Self> {
        Self::from_args(arg, &Some(combo_arg))
    }

    pub fn from_args(arg: &str, combo_arg: &Option<usize>) -> Result<Seed> {
        let mut anchored = vec![];
        let mut words = vec![];
        let split = if arg.contains(SEPARATOR) {
            arg.split(SEPARATOR)
        } else {
            arg.split(" ")
        };
        for (index, word) in split.enumerate() {
            if word.starts_with("^") {
                anchored.push(index);
            }
            let word = word.replace("^", "");

            if word.contains("?") || word.contains("|") {
                let mut all = vec![];
                for word in word.split("|") {
                    let mut matching = vec![];
                    let w = word.replace("?", "");

                    for i in 0..NUM_WORDS {
                        if word.starts_with("?")
                            && word.ends_with("?")
                            && BIP39_WORDS[i].contains(&w)
                        {
                            matching.push(i as u32);
                        } else if word.starts_with("?") && BIP39_WORDS[i].ends_with(&w) {
                            matching.push(i as u32);
                        } else if word.ends_with("?") && BIP39_WORDS[i].starts_with(&w) {
                            matching.push(i as u32);
                        } else if BIP39_WORDS[i] == &w {
                            matching.push(i as u32);
                        }
                    }

                    if matching.is_empty() {
                        bail!("No matching seed words for '{}' found{}", word, ERR_MSG);
                    }
                    all.extend(matching);
                }
                words.push(all);
            } else if BIP39_WORDS.contains(&word.as_str()) {
                let num = BIP39_WORDS.iter().position(|&w| w == word).unwrap();
                words.push(vec![num as u32]);
            } else {
                bail!("Unknown seed word '{}' found{}", word, ERR_MSG);
            }
        }

        let words = match combo_arg {
            None => Combinations::new(words),
            Some(combo) => Self::validate_combinations(words, *combo, anchored)?,
        };

        Ok(Self::from_words(words))
    }

    pub fn hash_ratio(&self) -> f64 {
        let valid = max(1, self.valid_seeds()) as f64;
        self.total() as f64 / valid
    }

    pub fn with_pure_gpu(&self, is_pure_gpu: bool) -> Self {
        let mut copy = self.clone();
        copy.encoder.is_pure_gpu = is_pure_gpu;
        copy
    }

    #[allow(dead_code)]
    fn from_vecs(words: Vec<Vec<u32>>) -> Seed {
        Self::from_words(Combinations::new(words))
    }

    fn from_words(words: Combinations<u32>) -> Seed {
        let encoder = SeedEncoder::new(words.clone(), false);
        let args = Combinations::new(
            words
                .fixed_positions()
                .into_iter()
                .map(|fixed| match fixed {
                    None => vec!["?".to_string()],
                    Some(index) => vec![index.to_string()],
                })
                .collect(),
        );
        Seed {
            words,
            encoder,
            args,
        }
    }

    pub fn next_arg(&mut self) -> Option<String> {
        if let Some(arg) = self.args.next() {
            Some(arg.join(SEPARATOR))
        } else {
            None
        }
    }

    pub fn total_args(&self) -> u64 {
        self.args.total()
    }

    pub fn binary_charsets(
        &self,
        max_args: u64,
        passphrase: &Option<Passphrase>,
    ) -> Result<Option<(Seed, Passphrase)>> {
        if self.words.permutations() > 1 {
            return Ok(None);
        }

        let mut seed = self.clone();
        let mut args = vec![];
        let mut guesses = 0;
        let mut last_question = false;
        for element in seed.words.elements() {
            if element.len() == BIP39_WORDS.len() {
                guesses += 1;
                args.push(vec!["?".to_string()]);
                last_question = true;
            } else {
                let mapped = if element.len() > 1 {
                    element.into_iter().map(|i| format!("={}", i)).collect()
                } else {
                    vec![element[0].to_string()]
                };
                args.push(mapped);
                last_question = false;
            }
        }
        seed.args = Combinations::new(args);
        if !last_question {
            return Ok(None);
        }
        if seed.args.total() > max_args {
            return Ok(None);
        }

        let passphrase = passphrase.clone().unwrap_or(Passphrase::empty_mask());
        if let Some(passphrase) =
            passphrase.add_binary_charsets(guesses, self.encoder.entropy_bits)?
        {
            return Ok(Some((seed, passphrase)));
        }
        return Ok(None);
    }

    fn validate_combinations(
        words: Vec<Vec<u32>>,
        combo: usize,
        anchored: Vec<usize>,
    ) -> Result<Combinations<u32>> {
        let combo_str = format!("Seed word length from '--combinations' is {}", combo);
        let num = combo - anchored.len();
        if !VALID_LENGTHS.contains(&combo) {
            bail!("{} must be one of {:?}", combo_str, VALID_LENGTHS);
        }
        if words.len() < combo {
            bail!(
                "{} but only {} possible words supplied",
                combo_str,
                words.len()
            );
        }
        if num >= 21 {
            bail!(
                "Attempting {}! permutations is infeasible, try anchoring more words with '^' prefix",
                num
            );
        }
        let mut indices = vec![];
        for i in 0..words.len() {
            if anchored.contains(&i) && i >= combo {
                bail!(
                    "{} but attempting to anchor a word at location {}",
                    combo_str,
                    i + 1
                );
            }
            if !anchored.contains(&i) {
                indices.push(i);
            }
        }
        Ok(Combinations::permute(words, indices, combo))
    }

    /// Returns the complete found seed
    pub fn found(&self, found: Option<String>) -> Result<Finished> {
        if let Some(found) = found {
            let mut seed = vec![];
            let mut split = found.split(",");
            for element in &self.words.fixed_positions() {
                match *element {
                    Some(index) => seed.push(BIP39_WORDS[index as usize]),
                    None => {
                        let next = split.next();
                        seed.push(next.ok_or(format_err!("Not enough words in {}", found))?);
                    }
                }
            }
            let passphrase = split.next().unwrap_or("");
            Ok(Finished::new(
                &seed.join(SEPARATOR),
                passphrase,
                self.encoder.is_pure_gpu,
            ))
        } else {
            Ok(Finished::exhausted(self.encoder.is_pure_gpu))
        }
    }

    pub fn shard_words(&self, min: usize) -> Vec<Seed> {
        let mut shards = vec![];
        for shard_words in self.words.shard(min) {
            let mut s = self.clone();
            s.words = shard_words;
            shards.push(s);
        }
        shards
    }

    pub fn valid_seeds(&self) -> u64 {
        if self.total() < EXACT_VALID_MAX {
            return self.exact_valid_seeds();
        }
        let divisor = 2_u64.pow(self.words.len() as u32 / 3);
        self.total() / divisor
    }

    fn exact_valid_seeds(&self) -> u64 {
        let mut seed = self.clone();
        let mut num = 0;
        while let Some(_) = seed.next_valid() {
            num += 1;
        }
        num
    }

    pub fn next_valid(&mut self) -> Option<Vec<u8>> {
        while let Some(next) = self.words.next() {
            if self.encoder.valid_checksum(next) {
                return Some(self.encoder.encode_words(next));
            }
        }
        None
    }

    pub fn next_encoded(&mut self) -> Option<Vec<u8>> {
        if let Some(next) = self.words.next() {
            return Some(self.encoder.encode_words(next));
        }
        None
    }

    pub fn next(&mut self) -> Option<&Vec<u32>> {
        self.words.next()
    }

    pub fn validate_length(&self) -> Result<()> {
        if VALID_LENGTHS.contains(&self.words.len()) {
            return Ok(());
        }
        bail!(
            "Invalid number of seed words '{}' should be one of {:?}",
            self.words.len(),
            VALID_LENGTHS
        );
    }

    pub fn to_words(indices: &Vec<u32>) -> String {
        let mut words = vec![];
        for index in indices {
            words.push(BIP39_WORDS[*index as usize]);
        }
        words.join(",")
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Finished {
    pub seed: Option<String>,
    pub passphrase: Option<String>,
    pub pure_gpu: bool,
}

impl Display for Finished {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (&self.seed, &self.passphrase) {
            (Some(seed), Some(passphrase)) => write!(f, "{} {}", seed, passphrase)?,
            _ => write!(f, "Exhausted")?,
        }
        if self.pure_gpu {
            write!(f, " (Pure)")?
        } else {
            write!(f, " (Stdin)")?
        }
        Ok(())
    }
}

impl Finished {
    pub fn new(seed: &str, passphrase: &str, pure_gpu: bool) -> Finished {
        Finished {
            seed: Some(seed.to_string()),
            passphrase: Some(passphrase.to_string()),
            pure_gpu,
        }
    }

    pub fn exhausted(pure_gpu: bool) -> Finished {
        Finished {
            seed: None,
            passphrase: None,
            pure_gpu,
        }
    }
}

#[derive(Debug, Clone)]
struct SeedEncoder {
    guessed: Vec<usize>,
    entropy_bits: usize,
    checksum_bits: usize,
    // If true we are writing to the hashes file, otherwise we encode for the stdin passwords
    is_pure_gpu: bool,
    total_entropy: usize,
    hasher: Sha256,
}

impl SeedEncoder {
    pub fn new(words: Combinations<u32>, is_pure_gpu: bool) -> Self {
        let mut guessed = vec![];
        for i in 0..words.len() {
            let fixed = words.fixed_positions();
            if fixed[i].is_none() {
                guessed.push(i);
            }
        }
        let total_bits = words.len() * 11;
        let total_entropy = total_bits - total_bits % 32;
        let checksum_bits = total_bits - total_entropy;
        let entropy_bits = 11_usize.saturating_sub(checksum_bits);

        Self {
            guessed,
            entropy_bits,
            checksum_bits,
            is_pure_gpu,
            total_entropy,
            hasher: Default::default(),
        }
    }

    pub fn valid_checksum(&mut self, wordlist: &Vec<u32>) -> bool {
        let last_word = wordlist.last().expect("non-empty");
        let last_entropy = *last_word & (0xFFFFFFFF << self.checksum_bits);

        let mut offset: isize = 32;
        let mut index = 0;
        let mut entropy = vec![0; self.total_entropy / 32];
        for i in 0..wordlist.len() - 1 {
            offset -= 11;
            if offset < 0 {
                entropy[index] |= wordlist[i] >> -offset;
                index += 1;
                offset += 32;
            }
            entropy[index] |= wordlist[i] << offset;
        }
        offset -= 11;
        entropy[index] |= last_entropy >> -offset;

        for ent in entropy {
            self.hasher.update(&ent.to_be_bytes());
        }
        let hash = self.hasher.finalize_fixed_reset();

        let checksum_mask = 0xFFFFFFFF >> (32 - self.checksum_bits);
        let checksum = (hash.as_slice()[0] as u32) >> (8 - self.checksum_bits);

        *last_word & checksum_mask == checksum
    }

    pub fn encode_words(&self, wordlist: &Vec<u32>) -> Vec<u8> {
        if self.is_pure_gpu {
            let guessed = BTreeSet::from_iter(self.guessed.iter());
            let mut encoded = vec![];
            for i in 0..wordlist.len() {
                if guessed.contains(&i) {
                    // This is a special flag required so hashcat will return the word if found
                    encoded.push(format!("={}", wordlist[i].to_string()))
                } else {
                    encoded.push(wordlist[i].to_string())
                }
            }
            return encoded.join(",").into_bytes();
        }
        if self.guessed.is_empty() {
            return vec![];
        }

        let (last, words) = wordlist.split_last().expect("non-empty");
        let mut encoded = vec![];
        for i in &self.guessed {
            if *i < wordlist.len() - 1 {
                Self::encode_word(&mut encoded, words[*i]);
            }
        }

        let last_choice = self.guessed.last().expect("non-empty");
        if *last_choice == wordlist.len() - 1 {
            let entropy = *last >> (11 - self.entropy_bits);
            encoded.push(Self::char_offset(entropy as u8, self.entropy_bits as u8));
        }
        encoded
    }

    fn encode_word(encoded: &mut Vec<u8>, num: u32) {
        encoded.push(Self::char_offset((num >> 6) as u8, 5));
        encoded.push(Self::char_offset((num & 0x3F) as u8, 6));
    }

    fn char_offset(char: u8, bits: u8) -> u8 {
        char + BIP39_BYTE_OFFSET + bits
    }
}

#[cfg(test)]
mod tests {
    use crate::seed::*;

    #[test]
    fn uses_binary_charsets() {
        let s = Seed::from_arg("?,zoo,zoo|able,zoo,?,zoo,zoo,zoo,zoo,zoo,zoo,?").unwrap();
        let (mut s, _) = s.binary_charsets(10, &None).unwrap().unwrap();
        assert_eq!(
            s.next_arg().unwrap(),
            "?,2047,=2047,2047,?,2047,2047,2047,2047,2047,2047,?"
        );
        assert_eq!(
            s.next_arg().unwrap(),
            "?,2047,=2,2047,?,2047,2047,2047,2047,2047,2047,?"
        );
        assert_eq!(s.next_arg(), None);

        // Too many args test by setting to 0
        let s = Seed::from_arg("?,zoo,zoo|able,zoo,?,zoo,zoo,zoo,zoo,zoo,zoo,?").unwrap();
        assert!(s.binary_charsets(0, &None).unwrap().is_none());

        // Only works if the last word is '?'
        let s = Seed::from_arg("?,zoo,zoo|able,zoo,?,zoo,zoo,zoo,zoo,zoo,zoo,zoo").unwrap();
        assert!(s.binary_charsets(10, &None).unwrap().is_none());

        // Won't work with permutations
        let s = Seed::from_combo("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,?", 12).unwrap();
        assert!(s.binary_charsets(u64::MAX, &None).unwrap().is_none());
    }

    #[test]
    fn validates_combinations() {
        let s = Seed::from_combo("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo", 12);
        assert!(s.is_err());

        let s = Seed::from_combo("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo", 11);
        assert!(s.is_err());

        // 21! fails
        let s = Seed::from_combo(
            "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo",
            21,
        );
        assert!(s.is_err());

        // 20! works with anchor
        let s = Seed::from_combo(
            "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo ^zoo",
            21,
        );
        assert_eq!(
            s.unwrap().words.estimate_total(1_000_000),
            2432902008176640000
        );

        // anchor outside combo len
        let s = Seed::from_combo("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,^zoo", 12);
        assert!(s.is_err());

        // uses words outside the first anchor
        let mut s = Seed::from_combo("hand thought survey hill friend ^fatal|able ^fall ^amused ^pact ^ripple ^glance ^rural zoo zone", 12).unwrap();
        assert_eq!(s.words.total(), 5040);
        assert!(s.validate_length().is_ok());
        for _ in 0..2500 {
            s.next();
        }
        assert_eq!(
            s.next_arg(),
            Some("?,?,?,?,?,?,657,65,1269,1490,789,1516".to_string())
        );
        assert_eq!(
            Seed::to_words(s.next().unwrap()),
            "hill,hand,friend,survey,zoo,fatal,fall,amused,pact,ripple,glance,rural"
        );
        assert_eq!(
            Seed::to_words(s.next().unwrap()),
            "hill,hand,friend,survey,zoo,able,fall,amused,pact,ripple,glance,rural"
        );
    }

    #[test]
    fn estimates_valid_seeds() {
        let s = Seed::from_combo("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo", 12).unwrap();
        assert_eq!(s.words.estimate_total(1_000_000), 479001600);

        let s = Seed::from_arg("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,?,?").unwrap();
        assert_eq!(s.total(), 2048 * 2048);
        assert_eq!(s.valid_seeds(), 2048 * 2048 / 16);
        assert_eq!(s.hash_ratio(), 16.0);

        let s = Seed::from_arg("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo|zone,?,?").unwrap();
        assert_eq!(s.valid_seeds(), 524288);

        let s = Seed::from_arg("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo").unwrap();
        assert_eq!(s.valid_seeds(), 0);

        let s = Seed::from_arg("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,negative").unwrap();
        assert_eq!(s.valid_seeds(), 1);

        let s = Seed::from_arg("zoo,zoo,zoo,zoo,zoo,zoo,zoo,zoo,z?,a?,a?,able").unwrap();
        assert_eq!(s.valid_seeds(), 4687);
    }

    #[test]
    fn creates_finished_result() {
        let s = Seed::from_arg("jazz,?,?,zoo").unwrap();
        assert_eq!(
            s.found(Some("ability,zone,".to_string())).unwrap(),
            Finished::new("jazz,ability,zone,zoo", "", false)
        );
        assert!(s.validate_length().is_err());

        let s = Seed::from_arg("?,zoo").unwrap().with_pure_gpu(true);
        assert_eq!(
            s.found(Some("ability,pass".to_string())).unwrap(),
            Finished::new("ability,zoo", "pass", true)
        );
    }

    #[test]
    fn generates_args() {
        let mut seed = Seed::from_arg("ability,?,zoo").unwrap();
        assert_eq!(seed.next_arg(), Some("1,?,2047".to_string()));
    }

    #[test]
    fn parses_begin_and_end() {
        let seed = Seed::from_arg("?ppy,zoo").unwrap();
        assert_eq!(seed.total(), 3);
        assert_eq!(seed.begin(), "happy,zoo");
        assert_eq!(seed.end(), "unhappy,zoo");

        let seed = Seed::from_combo(
            "paper ^warrior title|tool join assume trumpet setup angle helmet salmon save love zoo",
            12,
        )
        .unwrap();
        assert_eq!(seed.total(), 2_u64 * (1..=12).product::<u64>());
        assert_eq!(
            seed.begin(),
            "paper,warrior,title,join,assume,trumpet,setup,angle,helmet,salmon,save,love"
        );
        assert_eq!(
            seed.end(),
            "zoo,warrior,love,save,salmon,helmet,angle,setup,trumpet,assume,join,tool"
        );
    }

    #[test]
    fn parses_seeds_words() {
        let mut seed = Seed::from_arg("ability,?,zoo").unwrap();
        assert_eq!(seed.total(), 2048);
        assert_eq!(seed.next().unwrap(), &vec![1, 0, 2047]);
        assert_eq!(seed.next().unwrap(), &vec![1, 1, 2047]);

        let mut seed = Seed::from_arg("zo?").unwrap().with_pure_gpu(true);
        assert_eq!(seed.total(), 2);
        assert_eq!(Seed::to_words(seed.next().unwrap()), "zone");
        assert_eq!(Seed::to_words(seed.next().unwrap()), "zoo");

        let mut seed = Seed::from_arg("?orro?").unwrap();
        assert_eq!(seed.total(), 3);
        assert_eq!(Seed::to_words(seed.next().unwrap()), "borrow");
        assert_eq!(Seed::to_words(seed.next().unwrap()), "horror");
        assert_eq!(Seed::to_words(seed.next().unwrap()), "tomorrow");

        let mut seed = Seed::from_arg("puppy|zo?").unwrap();
        assert_eq!(seed.total(), 3);
        assert_eq!(Seed::to_words(seed.next().unwrap()), "puppy");
        assert_eq!(Seed::to_words(seed.next().unwrap()), "zone");

        assert!(Seed::from_arg("zz?").is_err());
        assert!(Seed::from_arg("zz").is_err());
    }

    #[test]
    fn iterates_over_seeds() {
        let mut seed = Seed::from_vecs(vec![vec![1, 2, 3], vec![4], vec![5, 6], vec![7, 8]]);
        assert_eq!(seed.total(), 12);
        assert_eq!(seed.next(), Some(&vec![1, 4, 5, 7]));
        assert_eq!(seed.next(), Some(&vec![1, 4, 5, 8]));
    }

    fn zeros() -> Vec<Vec<u32>> {
        let mut zero = vec![];
        for _ in 0..12 {
            zero.push(vec![0]);
        }
        zero
    }

    #[test]
    fn can_convert_words() {
        let mut test = zeros();
        let last = 0b10111011100;
        let entr = 0b00001011101;
        test[1] = vec![0, 0];
        test[5] = vec![2047, 2047];
        test[11] = vec![last, last];

        let result = Seed::from_vecs(test).next_encoded().unwrap();
        // 2 variable words + 1 entropy word
        assert_eq!(result.len(), 5);
        assert_eq!(
            result,
            vec![
                SeedEncoder::char_offset(0, 5),
                SeedEncoder::char_offset(0, 6),
                SeedEncoder::char_offset(31, 5),
                SeedEncoder::char_offset(63, 6),
                SeedEncoder::char_offset(entr, 7)
            ]
        );

        test = zeros();
        let bits = 0b11100110111;
        let bits5 = 0b11100;
        let bits6 = 0b110111;
        test[3] = vec![bits, bits];

        let result = Seed::from_vecs(test).next_encoded().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result,
            vec![
                SeedEncoder::char_offset(bits5, 5),
                SeedEncoder::char_offset(bits6, 6),
            ]
        );

        test = zeros();
        let mut seed = Seed::from_vecs(test).with_pure_gpu(true);
        let result = seed.next_encoded().unwrap();

        assert_eq!(String::from_utf8_lossy(&result), "0,0,0,0,0,0,0,0,0,0,0,0");
    }

    fn single_seed(vec: &Vec<u32>) -> Seed {
        let vecs = vec.into_iter().map(|i| vec![*i]).collect();
        Seed::from_vecs(vecs)
    }

    #[test]
    fn can_invalidate_checksums() {
        let w3 = vec![366, 297, 2047];
        let w6 = vec![1384, 1143, 803, 1671, 789, 2046];
        let w9 = vec![979, 1121, 205, 531, 441, 187, 585, 12, 2046];
        let w12 = vec![
            1993, 2044, 7, 1991, 1948, 1948, 973, 1893, 1438, 414, 1429, 2046,
        ];
        let w15 = vec![
            1947, 789, 1517, 704, 1971, 1615, 502, 1720, 1704, 1086, 1550, 883, 1447, 929, 2046,
        ];
        let w18 = vec![
            388, 1081, 652, 1498, 1177, 1022, 302, 1762, 335, 1903, 1238, 1348, 649, 65, 1380, 769,
            742, 2046,
        ];
        let w24 = vec![
            1760, 91, 1106, 217, 415, 922, 1718, 710, 841, 232, 583, 1910, 1814, 830, 1408, 642,
            222, 1089, 928, 1936, 958, 284, 800, 2046,
        ];
        let lists = vec![w3, w6, w9, w12, w15, w18, w24];
        for list in lists {
            assert!(single_seed(&list).next_valid().is_none());
        }
    }

    #[test]
    fn can_validate_checksums() {
        let w3 = vec![779, 505, 1435];
        let w6 = vec![1384, 1143, 803, 1671, 789, 1037];
        let w9 = vec![1087, 612, 665, 659, 1526, 1322, 1703, 1695, 828];
        let w12 = vec![
            1993, 2044, 7, 1991, 1948, 1948, 973, 1893, 1438, 414, 1429, 1554,
        ];
        let w15 = vec![
            1947, 789, 1517, 704, 1971, 1615, 502, 1720, 1704, 1086, 1550, 883, 1447, 929, 1270,
        ];
        let w18 = vec![
            388, 1081, 652, 1498, 1177, 1022, 302, 1762, 335, 1903, 1238, 1348, 649, 65, 1380, 769,
            742, 1612,
        ];
        let w24 = vec![
            1760, 91, 1106, 217, 415, 922, 1718, 710, 841, 232, 583, 1910, 1814, 830, 1408, 642,
            222, 1089, 928, 1936, 958, 284, 800, 189,
        ];
        let lists = vec![w3, w6, w9, w12, w15, w18, w24];
        for list in lists {
            assert!(single_seed(&list).next_valid().is_some());
        }
    }
}

pub const BIP39_WORDS: &'static [&str; 2048] = &[
    "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract", "absurd",
    "abuse", "access", "accident", "account", "accuse", "achieve", "acid", "acoustic", "acquire",
    "across", "act", "action", "actor", "actress", "actual", "adapt", "add", "addict", "address",
    "adjust", "admit", "adult", "advance", "advice", "aerobic", "affair", "afford", "afraid",
    "again", "age", "agent", "agree", "ahead", "aim", "air", "airport", "aisle", "alarm", "album",
    "alcohol", "alert", "alien", "all", "alley", "allow", "almost", "alone", "alpha", "already",
    "also", "alter", "always", "amateur", "amazing", "among", "amount", "amused", "analyst",
    "anchor", "ancient", "anger", "angle", "angry", "animal", "ankle", "announce", "annual",
    "another", "answer", "antenna", "antique", "anxiety", "any", "apart", "apology", "appear",
    "apple", "approve", "april", "arch", "arctic", "area", "arena", "argue", "arm", "armed",
    "armor", "army", "around", "arrange", "arrest", "arrive", "arrow", "art", "artefact", "artist",
    "artwork", "ask", "aspect", "assault", "asset", "assist", "assume", "asthma", "athlete",
    "atom", "attack", "attend", "attitude", "attract", "auction", "audit", "august", "aunt",
    "author", "auto", "autumn", "average", "avocado", "avoid", "awake", "aware", "away", "awesome",
    "awful", "awkward", "axis", "baby", "bachelor", "bacon", "badge", "bag", "balance", "balcony",
    "ball", "bamboo", "banana", "banner", "bar", "barely", "bargain", "barrel", "base", "basic",
    "basket", "battle", "beach", "bean", "beauty", "because", "become", "beef", "before", "begin",
    "behave", "behind", "believe", "below", "belt", "bench", "benefit", "best", "betray", "better",
    "between", "beyond", "bicycle", "bid", "bike", "bind", "biology", "bird", "birth", "bitter",
    "black", "blade", "blame", "blanket", "blast", "bleak", "bless", "blind", "blood", "blossom",
    "blouse", "blue", "blur", "blush", "board", "boat", "body", "boil", "bomb", "bone", "bonus",
    "book", "boost", "border", "boring", "borrow", "boss", "bottom", "bounce", "box", "boy",
    "bracket", "brain", "brand", "brass", "brave", "bread", "breeze", "brick", "bridge", "brief",
    "bright", "bring", "brisk", "broccoli", "broken", "bronze", "broom", "brother", "brown",
    "brush", "bubble", "buddy", "budget", "buffalo", "build", "bulb", "bulk", "bullet", "bundle",
    "bunker", "burden", "burger", "burst", "bus", "business", "busy", "butter", "buyer", "buzz",
    "cabbage", "cabin", "cable", "cactus", "cage", "cake", "call", "calm", "camera", "camp", "can",
    "canal", "cancel", "candy", "cannon", "canoe", "canvas", "canyon", "capable", "capital",
    "captain", "car", "carbon", "card", "cargo", "carpet", "carry", "cart", "case", "cash",
    "casino", "castle", "casual", "cat", "catalog", "catch", "category", "cattle", "caught",
    "cause", "caution", "cave", "ceiling", "celery", "cement", "census", "century", "cereal",
    "certain", "chair", "chalk", "champion", "change", "chaos", "chapter", "charge", "chase",
    "chat", "cheap", "check", "cheese", "chef", "cherry", "chest", "chicken", "chief", "child",
    "chimney", "choice", "choose", "chronic", "chuckle", "chunk", "churn", "cigar", "cinnamon",
    "circle", "citizen", "city", "civil", "claim", "clap", "clarify", "claw", "clay", "clean",
    "clerk", "clever", "click", "client", "cliff", "climb", "clinic", "clip", "clock", "clog",
    "close", "cloth", "cloud", "clown", "club", "clump", "cluster", "clutch", "coach", "coast",
    "coconut", "code", "coffee", "coil", "coin", "collect", "color", "column", "combine", "come",
    "comfort", "comic", "common", "company", "concert", "conduct", "confirm", "congress",
    "connect", "consider", "control", "convince", "cook", "cool", "copper", "copy", "coral",
    "core", "corn", "correct", "cost", "cotton", "couch", "country", "couple", "course", "cousin",
    "cover", "coyote", "crack", "cradle", "craft", "cram", "crane", "crash", "crater", "crawl",
    "crazy", "cream", "credit", "creek", "crew", "cricket", "crime", "crisp", "critic", "crop",
    "cross", "crouch", "crowd", "crucial", "cruel", "cruise", "crumble", "crunch", "crush", "cry",
    "crystal", "cube", "culture", "cup", "cupboard", "curious", "current", "curtain", "curve",
    "cushion", "custom", "cute", "cycle", "dad", "damage", "damp", "dance", "danger", "daring",
    "dash", "daughter", "dawn", "day", "deal", "debate", "debris", "decade", "december", "decide",
    "decline", "decorate", "decrease", "deer", "defense", "define", "defy", "degree", "delay",
    "deliver", "demand", "demise", "denial", "dentist", "deny", "depart", "depend", "deposit",
    "depth", "deputy", "derive", "describe", "desert", "design", "desk", "despair", "destroy",
    "detail", "detect", "develop", "device", "devote", "diagram", "dial", "diamond", "diary",
    "dice", "diesel", "diet", "differ", "digital", "dignity", "dilemma", "dinner", "dinosaur",
    "direct", "dirt", "disagree", "discover", "disease", "dish", "dismiss", "disorder", "display",
    "distance", "divert", "divide", "divorce", "dizzy", "doctor", "document", "dog", "doll",
    "dolphin", "domain", "donate", "donkey", "donor", "door", "dose", "double", "dove", "draft",
    "dragon", "drama", "drastic", "draw", "dream", "dress", "drift", "drill", "drink", "drip",
    "drive", "drop", "drum", "dry", "duck", "dumb", "dune", "during", "dust", "dutch", "duty",
    "dwarf", "dynamic", "eager", "eagle", "early", "earn", "earth", "easily", "east", "easy",
    "echo", "ecology", "economy", "edge", "edit", "educate", "effort", "egg", "eight", "either",
    "elbow", "elder", "electric", "elegant", "element", "elephant", "elevator", "elite", "else",
    "embark", "embody", "embrace", "emerge", "emotion", "employ", "empower", "empty", "enable",
    "enact", "end", "endless", "endorse", "enemy", "energy", "enforce", "engage", "engine",
    "enhance", "enjoy", "enlist", "enough", "enrich", "enroll", "ensure", "enter", "entire",
    "entry", "envelope", "episode", "equal", "equip", "era", "erase", "erode", "erosion", "error",
    "erupt", "escape", "essay", "essence", "estate", "eternal", "ethics", "evidence", "evil",
    "evoke", "evolve", "exact", "example", "excess", "exchange", "excite", "exclude", "excuse",
    "execute", "exercise", "exhaust", "exhibit", "exile", "exist", "exit", "exotic", "expand",
    "expect", "expire", "explain", "expose", "express", "extend", "extra", "eye", "eyebrow",
    "fabric", "face", "faculty", "fade", "faint", "faith", "fall", "false", "fame", "family",
    "famous", "fan", "fancy", "fantasy", "farm", "fashion", "fat", "fatal", "father", "fatigue",
    "fault", "favorite", "feature", "february", "federal", "fee", "feed", "feel", "female",
    "fence", "festival", "fetch", "fever", "few", "fiber", "fiction", "field", "figure", "file",
    "film", "filter", "final", "find", "fine", "finger", "finish", "fire", "firm", "first",
    "fiscal", "fish", "fit", "fitness", "fix", "flag", "flame", "flash", "flat", "flavor", "flee",
    "flight", "flip", "float", "flock", "floor", "flower", "fluid", "flush", "fly", "foam",
    "focus", "fog", "foil", "fold", "follow", "food", "foot", "force", "forest", "forget", "fork",
    "fortune", "forum", "forward", "fossil", "foster", "found", "fox", "fragile", "frame",
    "frequent", "fresh", "friend", "fringe", "frog", "front", "frost", "frown", "frozen", "fruit",
    "fuel", "fun", "funny", "furnace", "fury", "future", "gadget", "gain", "galaxy", "gallery",
    "game", "gap", "garage", "garbage", "garden", "garlic", "garment", "gas", "gasp", "gate",
    "gather", "gauge", "gaze", "general", "genius", "genre", "gentle", "genuine", "gesture",
    "ghost", "giant", "gift", "giggle", "ginger", "giraffe", "girl", "give", "glad", "glance",
    "glare", "glass", "glide", "glimpse", "globe", "gloom", "glory", "glove", "glow", "glue",
    "goat", "goddess", "gold", "good", "goose", "gorilla", "gospel", "gossip", "govern", "gown",
    "grab", "grace", "grain", "grant", "grape", "grass", "gravity", "great", "green", "grid",
    "grief", "grit", "grocery", "group", "grow", "grunt", "guard", "guess", "guide", "guilt",
    "guitar", "gun", "gym", "habit", "hair", "half", "hammer", "hamster", "hand", "happy",
    "harbor", "hard", "harsh", "harvest", "hat", "have", "hawk", "hazard", "head", "health",
    "heart", "heavy", "hedgehog", "height", "hello", "helmet", "help", "hen", "hero", "hidden",
    "high", "hill", "hint", "hip", "hire", "history", "hobby", "hockey", "hold", "hole", "holiday",
    "hollow", "home", "honey", "hood", "hope", "horn", "horror", "horse", "hospital", "host",
    "hotel", "hour", "hover", "hub", "huge", "human", "humble", "humor", "hundred", "hungry",
    "hunt", "hurdle", "hurry", "hurt", "husband", "hybrid", "ice", "icon", "idea", "identify",
    "idle", "ignore", "ill", "illegal", "illness", "image", "imitate", "immense", "immune",
    "impact", "impose", "improve", "impulse", "inch", "include", "income", "increase", "index",
    "indicate", "indoor", "industry", "infant", "inflict", "inform", "inhale", "inherit",
    "initial", "inject", "injury", "inmate", "inner", "innocent", "input", "inquiry", "insane",
    "insect", "inside", "inspire", "install", "intact", "interest", "into", "invest", "invite",
    "involve", "iron", "island", "isolate", "issue", "item", "ivory", "jacket", "jaguar", "jar",
    "jazz", "jealous", "jeans", "jelly", "jewel", "job", "join", "joke", "journey", "joy", "judge",
    "juice", "jump", "jungle", "junior", "junk", "just", "kangaroo", "keen", "keep", "ketchup",
    "key", "kick", "kid", "kidney", "kind", "kingdom", "kiss", "kit", "kitchen", "kite", "kitten",
    "kiwi", "knee", "knife", "knock", "know", "lab", "label", "labor", "ladder", "lady", "lake",
    "lamp", "language", "laptop", "large", "later", "latin", "laugh", "laundry", "lava", "law",
    "lawn", "lawsuit", "layer", "lazy", "leader", "leaf", "learn", "leave", "lecture", "left",
    "leg", "legal", "legend", "leisure", "lemon", "lend", "length", "lens", "leopard", "lesson",
    "letter", "level", "liar", "liberty", "library", "license", "life", "lift", "light", "like",
    "limb", "limit", "link", "lion", "liquid", "list", "little", "live", "lizard", "load", "loan",
    "lobster", "local", "lock", "logic", "lonely", "long", "loop", "lottery", "loud", "lounge",
    "love", "loyal", "lucky", "luggage", "lumber", "lunar", "lunch", "luxury", "lyrics", "machine",
    "mad", "magic", "magnet", "maid", "mail", "main", "major", "make", "mammal", "man", "manage",
    "mandate", "mango", "mansion", "manual", "maple", "marble", "march", "margin", "marine",
    "market", "marriage", "mask", "mass", "master", "match", "material", "math", "matrix",
    "matter", "maximum", "maze", "meadow", "mean", "measure", "meat", "mechanic", "medal", "media",
    "melody", "melt", "member", "memory", "mention", "menu", "mercy", "merge", "merit", "merry",
    "mesh", "message", "metal", "method", "middle", "midnight", "milk", "million", "mimic", "mind",
    "minimum", "minor", "minute", "miracle", "mirror", "misery", "miss", "mistake", "mix", "mixed",
    "mixture", "mobile", "model", "modify", "mom", "moment", "monitor", "monkey", "monster",
    "month", "moon", "moral", "more", "morning", "mosquito", "mother", "motion", "motor",
    "mountain", "mouse", "move", "movie", "much", "muffin", "mule", "multiply", "muscle", "museum",
    "mushroom", "music", "must", "mutual", "myself", "mystery", "myth", "naive", "name", "napkin",
    "narrow", "nasty", "nation", "nature", "near", "neck", "need", "negative", "neglect",
    "neither", "nephew", "nerve", "nest", "net", "network", "neutral", "never", "news", "next",
    "nice", "night", "noble", "noise", "nominee", "noodle", "normal", "north", "nose", "notable",
    "note", "nothing", "notice", "novel", "now", "nuclear", "number", "nurse", "nut", "oak",
    "obey", "object", "oblige", "obscure", "observe", "obtain", "obvious", "occur", "ocean",
    "october", "odor", "off", "offer", "office", "often", "oil", "okay", "old", "olive", "olympic",
    "omit", "once", "one", "onion", "online", "only", "open", "opera", "opinion", "oppose",
    "option", "orange", "orbit", "orchard", "order", "ordinary", "organ", "orient", "original",
    "orphan", "ostrich", "other", "outdoor", "outer", "output", "outside", "oval", "oven", "over",
    "own", "owner", "oxygen", "oyster", "ozone", "pact", "paddle", "page", "pair", "palace",
    "palm", "panda", "panel", "panic", "panther", "paper", "parade", "parent", "park", "parrot",
    "party", "pass", "patch", "path", "patient", "patrol", "pattern", "pause", "pave", "payment",
    "peace", "peanut", "pear", "peasant", "pelican", "pen", "penalty", "pencil", "people",
    "pepper", "perfect", "permit", "person", "pet", "phone", "photo", "phrase", "physical",
    "piano", "picnic", "picture", "piece", "pig", "pigeon", "pill", "pilot", "pink", "pioneer",
    "pipe", "pistol", "pitch", "pizza", "place", "planet", "plastic", "plate", "play", "please",
    "pledge", "pluck", "plug", "plunge", "poem", "poet", "point", "polar", "pole", "police",
    "pond", "pony", "pool", "popular", "portion", "position", "possible", "post", "potato",
    "pottery", "poverty", "powder", "power", "practice", "praise", "predict", "prefer", "prepare",
    "present", "pretty", "prevent", "price", "pride", "primary", "print", "priority", "prison",
    "private", "prize", "problem", "process", "produce", "profit", "program", "project", "promote",
    "proof", "property", "prosper", "protect", "proud", "provide", "public", "pudding", "pull",
    "pulp", "pulse", "pumpkin", "punch", "pupil", "puppy", "purchase", "purity", "purpose",
    "purse", "push", "put", "puzzle", "pyramid", "quality", "quantum", "quarter", "question",
    "quick", "quit", "quiz", "quote", "rabbit", "raccoon", "race", "rack", "radar", "radio",
    "rail", "rain", "raise", "rally", "ramp", "ranch", "random", "range", "rapid", "rare", "rate",
    "rather", "raven", "raw", "razor", "ready", "real", "reason", "rebel", "rebuild", "recall",
    "receive", "recipe", "record", "recycle", "reduce", "reflect", "reform", "refuse", "region",
    "regret", "regular", "reject", "relax", "release", "relief", "rely", "remain", "remember",
    "remind", "remove", "render", "renew", "rent", "reopen", "repair", "repeat", "replace",
    "report", "require", "rescue", "resemble", "resist", "resource", "response", "result",
    "retire", "retreat", "return", "reunion", "reveal", "review", "reward", "rhythm", "rib",
    "ribbon", "rice", "rich", "ride", "ridge", "rifle", "right", "rigid", "ring", "riot", "ripple",
    "risk", "ritual", "rival", "river", "road", "roast", "robot", "robust", "rocket", "romance",
    "roof", "rookie", "room", "rose", "rotate", "rough", "round", "route", "royal", "rubber",
    "rude", "rug", "rule", "run", "runway", "rural", "sad", "saddle", "sadness", "safe", "sail",
    "salad", "salmon", "salon", "salt", "salute", "same", "sample", "sand", "satisfy", "satoshi",
    "sauce", "sausage", "save", "say", "scale", "scan", "scare", "scatter", "scene", "scheme",
    "school", "science", "scissors", "scorpion", "scout", "scrap", "screen", "script", "scrub",
    "sea", "search", "season", "seat", "second", "secret", "section", "security", "seed", "seek",
    "segment", "select", "sell", "seminar", "senior", "sense", "sentence", "series", "service",
    "session", "settle", "setup", "seven", "shadow", "shaft", "shallow", "share", "shed", "shell",
    "sheriff", "shield", "shift", "shine", "ship", "shiver", "shock", "shoe", "shoot", "shop",
    "short", "shoulder", "shove", "shrimp", "shrug", "shuffle", "shy", "sibling", "sick", "side",
    "siege", "sight", "sign", "silent", "silk", "silly", "silver", "similar", "simple", "since",
    "sing", "siren", "sister", "situate", "six", "size", "skate", "sketch", "ski", "skill", "skin",
    "skirt", "skull", "slab", "slam", "sleep", "slender", "slice", "slide", "slight", "slim",
    "slogan", "slot", "slow", "slush", "small", "smart", "smile", "smoke", "smooth", "snack",
    "snake", "snap", "sniff", "snow", "soap", "soccer", "social", "sock", "soda", "soft", "solar",
    "soldier", "solid", "solution", "solve", "someone", "song", "soon", "sorry", "sort", "soul",
    "sound", "soup", "source", "south", "space", "spare", "spatial", "spawn", "speak", "special",
    "speed", "spell", "spend", "sphere", "spice", "spider", "spike", "spin", "spirit", "split",
    "spoil", "sponsor", "spoon", "sport", "spot", "spray", "spread", "spring", "spy", "square",
    "squeeze", "squirrel", "stable", "stadium", "staff", "stage", "stairs", "stamp", "stand",
    "start", "state", "stay", "steak", "steel", "stem", "step", "stereo", "stick", "still",
    "sting", "stock", "stomach", "stone", "stool", "story", "stove", "strategy", "street",
    "strike", "strong", "struggle", "student", "stuff", "stumble", "style", "subject", "submit",
    "subway", "success", "such", "sudden", "suffer", "sugar", "suggest", "suit", "summer", "sun",
    "sunny", "sunset", "super", "supply", "supreme", "sure", "surface", "surge", "surprise",
    "surround", "survey", "suspect", "sustain", "swallow", "swamp", "swap", "swarm", "swear",
    "sweet", "swift", "swim", "swing", "switch", "sword", "symbol", "symptom", "syrup", "system",
    "table", "tackle", "tag", "tail", "talent", "talk", "tank", "tape", "target", "task", "taste",
    "tattoo", "taxi", "teach", "team", "tell", "ten", "tenant", "tennis", "tent", "term", "test",
    "text", "thank", "that", "theme", "then", "theory", "there", "they", "thing", "this",
    "thought", "three", "thrive", "throw", "thumb", "thunder", "ticket", "tide", "tiger", "tilt",
    "timber", "time", "tiny", "tip", "tired", "tissue", "title", "toast", "tobacco", "today",
    "toddler", "toe", "together", "toilet", "token", "tomato", "tomorrow", "tone", "tongue",
    "tonight", "tool", "tooth", "top", "topic", "topple", "torch", "tornado", "tortoise", "toss",
    "total", "tourist", "toward", "tower", "town", "toy", "track", "trade", "traffic", "tragic",
    "train", "transfer", "trap", "trash", "travel", "tray", "treat", "tree", "trend", "trial",
    "tribe", "trick", "trigger", "trim", "trip", "trophy", "trouble", "truck", "true", "truly",
    "trumpet", "trust", "truth", "try", "tube", "tuition", "tumble", "tuna", "tunnel", "turkey",
    "turn", "turtle", "twelve", "twenty", "twice", "twin", "twist", "two", "type", "typical",
    "ugly", "umbrella", "unable", "unaware", "uncle", "uncover", "under", "undo", "unfair",
    "unfold", "unhappy", "uniform", "unique", "unit", "universe", "unknown", "unlock", "until",
    "unusual", "unveil", "update", "upgrade", "uphold", "upon", "upper", "upset", "urban", "urge",
    "usage", "use", "used", "useful", "useless", "usual", "utility", "vacant", "vacuum", "vague",
    "valid", "valley", "valve", "van", "vanish", "vapor", "various", "vast", "vault", "vehicle",
    "velvet", "vendor", "venture", "venue", "verb", "verify", "version", "very", "vessel",
    "veteran", "viable", "vibrant", "vicious", "victory", "video", "view", "village", "vintage",
    "violin", "virtual", "virus", "visa", "visit", "visual", "vital", "vivid", "vocal", "voice",
    "void", "volcano", "volume", "vote", "voyage", "wage", "wagon", "wait", "walk", "wall",
    "walnut", "want", "warfare", "warm", "warrior", "wash", "wasp", "waste", "water", "wave",
    "way", "wealth", "weapon", "wear", "weasel", "weather", "web", "wedding", "weekend", "weird",
    "welcome", "west", "wet", "whale", "what", "wheat", "wheel", "when", "where", "whip",
    "whisper", "wide", "width", "wife", "wild", "will", "win", "window", "wine", "wing", "wink",
    "winner", "winter", "wire", "wisdom", "wise", "wish", "witness", "wolf", "woman", "wonder",
    "wood", "wool", "word", "work", "world", "worry", "worth", "wrap", "wreck", "wrestle", "wrist",
    "write", "wrong", "yard", "year", "yellow", "you", "young", "youth", "zebra", "zero", "zone",
    "zoo",
];
