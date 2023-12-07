use std::fmt::{Display, Formatter};
use std::str::FromStr;

use crate::logger::Attempt;
use anyhow::{bail, format_err, Result};
use bitcoin::bip32::{ChildNumber, Xpub};
use bitcoin::{Address, Network};

// FIXME: Need this to be low for now or status updates are too slow
const MAX_DERIVATIONS: usize = 10;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AddressValid {
    pub formatted: String,
    pub kind: AddressKind,
    pub derivations: Derivations,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Derivations {
    derivations: Vec<String>,
    args: Vec<String>,
}

impl Derivations {
    /// Args that are exploded in the hashes file
    pub fn args(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Number of args that are exploded inside hashcat
    pub fn hash_ratio(&self) -> f64 {
        self.derivations.len() as f64 / self.args.len() as f64
    }
}

impl Attempt for Derivations {
    fn total(&self) -> u64 {
        self.derivations.len() as u64
    }

    fn begin(&self) -> String {
        self.derivations.first().expect("exists").clone()
    }

    fn end(&self) -> String {
        self.derivations.last().expect("exists").clone()
    }
}

const ERR_MSG: &str = "\nDerivation path should be valid comma or path-separated:
  Address #0 from an unhardened path: 'm/0/0'
  Address #2 from a hardened path:    'm/44h/0h/0h/0/2'
  You can try multiple paths:         'm/0/0,m/44h/0h/0h/0/0'
  '?' attempts all paths from 0-11:   'm/0/?11'

  Master XPUB does not require a derivation path and is ~2x faster to guess
  Try to use the exact derivation path for the address you have (see https://walletsrecovery.org/)\n";

impl AddressValid {
    pub fn from_arg(address: &str, derivation: &Option<String>) -> Result<Self> {
        let kind = Self::kind(&address)?;

        if kind.is_xpub && derivation.is_some() {
            bail!("XPUBs do not require a derivation path to be specified");
        }

        let derivations = Self::derivation(&kind, derivation)?;

        Ok(Self::new(address.to_string(), kind, derivations))
    }

    pub fn new(formatted: String, kind: AddressKind, derivations: Derivations) -> Self {
        Self {
            formatted,
            kind,
            derivations,
        }
    }

    fn kind(address: &str) -> Result<AddressKind> {
        let strs: Vec<_> = address_kinds().iter().map(|k| format!("\t{}", k)).collect();
        let error = format!("You must use one of the following formats (https://en.bitcoin.it/wiki/List_of_address_prefixes)\n{}", strs.join("\n"));

        for kind in address_kinds() {
            if address.starts_with(&kind.start) {
                if kind.is_xpub {
                    match Xpub::from_str(&address) {
                        Ok(xpub) if is_master(xpub) => return Ok(kind.clone()),
                        Ok(_) => bail!(
                            "Xpub is not a master public key (use an address instead)\n{}",
                            error
                        ),
                        Err(_) => bail!("Xpub is not correctly encoded\n{}", error),
                    }
                } else {
                    match Address::from_str(&address) {
                        Ok(_) => return Ok(kind.clone()),
                        Err(_) => bail!("Address is not correctly encoded\n{}", error),
                    }
                }
            }
        }

        bail!(error);
    }

    fn derivation(kind: &AddressKind, arg: &Option<String>) -> Result<Derivations> {
        let split = match arg {
            None => kind.derivations.clone(),
            Some(arg) => {
                let args = if arg.contains(",") {
                    arg.split(",")
                } else if arg.contains("|") {
                    arg.split("|")
                } else {
                    arg.split(" ")
                };
                args.map(|s| s.to_string()).collect()
            }
        };

        let mut derivations = vec![];
        let mut args = vec![];
        for derivation in split.clone() {
            let derivation = match derivation.strip_prefix("m/") {
                None => bail!(
                    "Derivation path '{}' must start with 'm/'{}",
                    derivation,
                    ERR_MSG
                ),
                Some(str) => str,
            };

            let (derivation, arg) = Self::derivation_paths(derivation, derivations.len())?;
            derivations.extend(derivation);

            if derivations.len() <= MAX_DERIVATIONS && args.len() > 0 {
                args = Self::extend_paths(&args, &arg, ",");
            } else {
                args.extend(arg);
            }
        }

        Ok(Derivations { derivations, args })
    }

    fn derivation_paths(derivation: &str, num_args: usize) -> Result<(Vec<String>, Vec<String>)> {
        let mut derivations = vec!["m".to_string()];
        let mut args = vec!["m".to_string()];

        for path in derivation.split("/").into_iter() {
            let nodes = Self::derivation_nodes(path).map_err(|err| {
                format_err!(
                    "Bad element in derivation path '{}' {}{}",
                    derivation,
                    err,
                    ERR_MSG
                )
            })?;

            derivations = Self::extend_paths(&derivations, &nodes, "/");

            if num_args + derivations.len() > MAX_DERIVATIONS {
                args = Self::extend_paths(&args, &nodes, "/");
            } else {
                args = Self::extend_paths(&args, &vec![path.to_string()], "/");
            }
        }

        return Ok((derivations, args));
    }

    fn extend_paths(current: &Vec<String>, nodes: &Vec<String>, delim: &str) -> Vec<String> {
        let mut tmp = vec![];
        for node in nodes.clone().into_iter() {
            for out in current.clone().into_iter() {
                tmp.push(format!("{}{}{}", out, delim, node));
            }
        }
        tmp
    }

    fn derivation_nodes(path: &str) -> Result<Vec<String>> {
        let mut suffix = "".to_string();
        let mut question = "".to_string();
        let mut node = path.chars();

        if path.ends_with("h") || path.ends_with("'") {
            suffix = node.next_back().unwrap().to_string();
        }
        if path.starts_with("?") {
            question = node.next().unwrap().to_string();
        }

        return match node.as_str().parse::<u32>() {
            Ok(num) if question.is_empty() => Ok(vec![format!("{}{}", num, suffix)]),
            Ok(num) => Ok((0..=num).map(|i| format!("{}{}", i, suffix)).collect()),
            Err(_) => bail!("invalid number '{}'", node.as_str()),
        };
    }
}

pub fn address_kinds() -> Vec<AddressKind> {
    vec![
        AddressKind::new(
            "XPUB",
            "Master Extended Pubic Key",
            "xpub",
            vec!["m/0".to_string()],
            true,
        ),
        AddressKind::new(
            "P2PKH",
            "Legacy",
            "1",
            vec!["m/0/0".to_string(), "m/44'/0'/0'/0/0".to_string()],
            false,
        ),
        AddressKind::new(
            "P2SH-P2WPKH",
            "Nested Segwit",
            "3",
            vec!["m/0/0".to_string(), "m/49'/0'/0'/0/0".to_string()],
            false,
        ),
        AddressKind::new(
            "P2WPKH",
            "Native Segwit",
            "bc1",
            vec!["m/84'/0'/0'/0/0".to_string()],
            false,
        ),
    ]
}

fn is_master(xpub: Xpub) -> bool {
    return xpub.network == Network::Bitcoin
        && xpub.depth == 0
        && xpub.child_number == ChildNumber::from(0);
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AddressKind {
    pub key: String,
    pub name: String,
    start: String,
    derivations: Vec<String>,
    is_xpub: bool,
}

impl AddressKind {
    fn new(key: &str, name: &str, start: &str, derivations: Vec<String>, is_xpub: bool) -> Self {
        Self {
            key: key.to_string(),
            name: name.to_string(),
            start: start.to_string(),
            derivations,
            is_xpub,
        }
    }
}

impl Display for AddressKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:<10}", format!("{}...", self.start))?;
        write!(f, "{:<15}", self.key)?;
        write!(f, "{}", self.name)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::address::*;

    #[test]
    fn parses_addresses() {
        let kind = AddressValid::kind("1111111111111111111114oLvT2").unwrap();
        assert_eq!(kind.key, "P2PKH");

        let kind = AddressValid::kind("3AzWUwL8YYci6ZAjAfd6mzzKDAmsCWB7Nr").unwrap();
        assert_eq!(kind.key, "P2SH-P2WPKH");

        let kind = AddressValid::kind("bc1q3zn9axe5k3tptupymypjzheuxf8r9yp7zutulg").unwrap();
        assert_eq!(kind.key, "P2WPKH");

        let kind = AddressValid::kind("xpub661MyMwAqRbcG95rS28rhZiknMvbUJhPpEWgMUbWa4xjMEc12aVewXf7fey3rGD9Sef81NXqTd1vyYToRokkiU9BTz6u5UXmikfNHTV9oCT").unwrap();
        assert_eq!(kind.key, "XPUB");

        // non-master xpub
        let kind = AddressValid::kind("xpub6878MZDSpciXuNC2cRRBa6dZsgBeE8UYaFDqA1uTazMaYdR1Xq7HFHBC3FpcFHiMytkmrMVBQKi3Wx2wT9xAn8mxuMeqtJG8TPDcpyfTk2J");
        assert!(kind.is_err());
    }

    #[test]
    fn parses_derivations() {
        let kind = AddressKind::new("", "", "", vec!["m/123".to_string()], false);
        let derivation = AddressValid::derivation(&kind, &None).unwrap();
        assert_eq!(derivation.args(), vec!["m/123".to_string()]);

        let derivation = AddressValid::derivation(&kind, &Some("m/0,m/1'".to_string())).unwrap();
        assert_eq!(derivation.args(), vec!["m/0,m/1'".to_string()]);

        let derivation = AddressValid::derivation(&kind, &Some("m/0 m/1/?2".to_string())).unwrap();
        assert_eq!(derivation.args(), vec!["m/0,m/1/?2".to_string()]);
        assert_eq!(derivation.begin(), "m/0");
        assert_eq!(derivation.end(), "m/1/2");
        assert_eq!(derivation.total(), 4);
        assert_eq!(derivation.hash_ratio(), 4.0);

        assert!(AddressValid::derivation(&kind, &Some("z/?2".to_string())).is_err());

        // splits if over 10
        let derivation =
            AddressValid::derivation(&kind, &Some("m/?9'/9/?9|m/0/0".to_string())).unwrap();
        assert_eq!(derivation.begin(), "m/0'/9/0");
        assert_eq!(derivation.end(), "m/0/0");
        assert_eq!(derivation.total(), 101);
        assert_eq!(derivation.hash_ratio(), 101.0 / 11.0);
        assert_eq!(
            derivation.args,
            vec![
                "m/?9'/9/0",
                "m/?9'/9/1",
                "m/?9'/9/2",
                "m/?9'/9/3",
                "m/?9'/9/4",
                "m/?9'/9/5",
                "m/?9'/9/6",
                "m/?9'/9/7",
                "m/?9'/9/8",
                "m/?9'/9/9",
                "m/0/0",
            ]
        );
    }
}
