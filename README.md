# seedcat
The [world's fastest](docs/design.md#benchmarks) bitcoin seed word and passphrase recovery tool.

If you lost your [seed phrase](https://en.bitcoin.it/wiki/Seed_phrase) information this tool can recover access to your bitcoin.

No need to trust third-party services who charge up to 20% of your funds.

`seedcat` is free and open-source software you can run on your own machine.

## Setup

- For NVIDIA GPUs install [CUDA](https://developer.nvidia.com/cuda-downloads) (other platforms [see hashcat docs](https://hashcat.net/hashcat/))
- If you need more powerful hardware see the [renting GPU instructions](docs/renting.md)
- Download the [latest release zip](https://github.com/seed-cat/seedcat/releases) and extract the folder

Optionally you can verify the release like so:
```
gpg --keyserver keyserver.ubuntu.com --recv-keys D249C16D6624F2C1DD0AC20B7E1F90D33230660A
gpg --verify seedcat_0.0.1.zip.sig
```

You should get the result:
```
gpg: Good signature from "Seed Cat <seedcat@protonmail.com>" [unknown]
gpg: WARNING: This key is not certified with a trusted signature!
gpg:          There is no indication that the signature belongs to the owner.
Primary key fingerprint: D249 C16D 6624 F2C1 DD0A  C20B 7E1F 90D3 3230 660A
```

## Usage
Run `seedcat` on Linux or `seedcat.exe` on Windows to see the command-line options.

See our [recovery documentation](docs/recovery.md) for detailed examples.

## Security
Since `seedcat` outputs your seed phrase anyone with access to your machine could steal your bitcoin.
- Disable your internet access before running with any secret information
- Do not enable internet access until you have swept your funds to a new wallet
- If renting in the cloud make sure you trust the datacenter host
- For large amounts of bitcoin consider buying GPUs instead of renting (you can resell them afterwards)

## Performance
If recovery is taking too long:
- First try to reduce the `Total Guesses` by adding constraints and previewing the configuration
- Try to find the `XPUB` for your wallet for faster recovery without derivations
- Lastly upgrade your GPU or consider [renting multi-GPU clusters](docs/renting.md) in the cloud

When running `seedcat` will display whether it is in `Pure GPU Mode` or `Stdin Mode`
- The pure GPU mode scales better on multi-GPU clusters
- Stdin mode is required if you are guessing too many seeds or too few passphrases
- For more information on modes see the [design docs](docs/design.md#seedcat-frontend)

To ensure your GPU is running correctly you may wish to compare your performance to our reference benchmarks:
```bash
seedcat test --bench --diff "<3090|8x4090>"
```

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