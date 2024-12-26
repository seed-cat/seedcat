# seedcat
The [world's fastest](docs/design.md#benchmarks) bitcoin seed phrase recovery tool.

Modern bitcoin wallets use [BIP39 seed phrases](https://en.bitcoin.it/wiki/Seed_phrase) which consist of 12 or 24 seed words and an optional passphrase.

Users who lose part of their seed phrase cannot access their bitcoin.
For instance, seed phrase backups might be incorrectly transcribed or damaged by natural disasters.
Memorized seed phrases may be partially forgotten or lost due to death.

`seedcat` helps you recover missing parts of your seed phrase:
- Guesses seed words that are missing, fragmented, or out-of-order
- Guesses passphrases using dictionaries and/or wildcards
- Leverages GPU parallelism to guess millions of seed phrases per second
- Open-source, non-custodial software that can run on your own computer

No need to trust third-party recovery services who charge up to 20% of your funds.

## Recovery instructions
`seedcat` attempts to guess seed phrases using your GPU

1. For NVIDIA GPUs install [CUDA](https://developer.nvidia.com/cuda-downloads) (other platforms see [hashcat documentation](https://hashcat.net/hashcat/))
2. Download the [latest release zip](https://github.com/seed-cat/seedcat/releases) and extract the folder 
3. Optionally you can verify the release like so:
```
> gpg --keyserver keyserver.ubuntu.com --recv-keys D249C16D6624F2C1DD0AC20B7E1F90D33230660A
> gpg --verify seedcat_*.zip.sig

gpg: Good signature from "Seed Cat <seedcat@protonmail.com>" [unknown]
Primary key fingerprint: D249 C16D 6624 F2C1 DD0A C20B 7E1F 90D3 3230 660A
```

4. Run `seedcat` on Linux or `seedcat.exe` on Windows to view the command-line options.
5. See our [recovery examples](docs/recovery.md) for detailed instructions.

If you have issues running locally or need larger GPU clusters see [renting in the cloud documentation](docs/renting.md)

## Security concerns
Since `seedcat` handles your seed phrase you should take the following security precautions:
- Disable your internet access before running with any real seed phrase information
- Sweep all bitcoin to a new wallet before enabling internet access
- For large recoveries it is safer to build your own GPU cluster than rent

Also note that:
- All code is open-source so anyone can verify there is no malicious code
- You can build from source by following the steps in [./scripts/release.sh](./scripts/release.sh)

## Performance issues
If recovery is taking too long first try to reduce the number of guesses:
- `XPUB` offers ~2x the speed and works on non-standard derivation paths and scripts
- Otherwise try to specify the exact derivation path for your address
- When seed word guessing specify letters to reduce the possible words (e.g. `so?` instead of `s?`)
- Leaving the last seed word as `?` may run faster on some systems (by allowing for pure GPU mode)
- When seed word descrambling anchor words with `^` to reduce the permutations
- For passphrase mask attacks use the most restrictive wildcards (e.g. `?l` instead of `?a`) or custom charsets
- For passphrase dictionary attacks try the most frequent words first

You may also need to upgrade your hardware:
- You should see all your GPUs print out when running
- A high-end gaming computer can handle ~100B guesses within a day
- An 8+ GPU cluster can handle ~1T guesses within a day
- You can test out your recovery speed in the [cloud](docs/renting.md) (using a dummy seed phrase)

## Contributing
All contributions are welcome, including reporting bugs or missing features through [new issues](https://github.com/seed-cat/seedcat/issues).

Check out our high-level [design docs](docs/design.md).

Developers will need to be able to compile Rust and C code.  You can setup your machine like so:

```bash
sudo apt update
sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 make git curl
curl https://sh.rustup.rs -sSf | sh

git clone git@github.com:seed-cat/seedcat.git
cd seedcat
git submodule update --init --recursive
```

For the Rust unit tests run `cargo test`

For the C unit tests (if modifying the hashcat code) run `./scripts/test_hashcat.sh test all`

For the integration tests run `cargo run -r -- test -t`

## Reach out
If you have more questions you can contact seedcat through the following:
 - [Email: seedcat@protonmail.com](mailto:seedcat@protonmail.com)
 - [Open a Github Issue](https://github.com/seed-cat/seedcat/issues/new)
