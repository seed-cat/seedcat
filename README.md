# seedcat
The [world's fastest](docs/design.md) bitcoin seed word and passphrase recovery tool.

As bitcoin users switch from [insecure brain wallets](https://fc16.ifca.ai/preproceedings/36_Vasek.pdf) to modern wallets that implement [seed phrases](https://en.bitcoin.it/wiki/Seed_phrase) the need for a better recovery tool emerged.

If you lost your seed words or passphrase this tool is your best chance at recovery without trusting third-party services who charge up to 20% of your funds.

## Setup

- For NVIDIA GPUs install [CUDA](https://developer.nvidia.com/cuda-downloads) (other platforms [see hashcat docs](https://hashcat.net/hashcat/))
- If you need more powerful hardware see the [renting GPUs instructions](#renting-gpus)
- Download the [latest release]() and unzip
- Run `seedcat` on Linux or `seedcat.exe` on Windows

Optionally you can verify the release like so:
```
gpg --keyserver keyserver.ubuntu.com --recv-keys D249C16D6624F2C1DD0AC20B7E1F90D33230660A
gpg --verify seedcat_0.0.1.zip.gpg
```

You should get the result:
```
gpg: Good signature from "Seed Cat <seedcat@protonmail.com>" [unknown]
gpg: WARNING: This key is not certified with a trusted signature!
gpg:          There is no indication that the signature belongs to the owner.
Primary key fingerprint: D249 C16D 6624 F2C1 DD0A  C20B 7E1F 90D3 3230 660A
```

## Seed Recovery
Any recovery requires a [bitcoin address](https://en.bitcoin.it/wiki/Invoice_address) and some seed word information from your wallet.

For example if you memorized your seed words but can't remember the first 3 words then you could use the `?` wildcard:
```bash
seedcat --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" --seed "? ? ? ethics vapor struggle ramp dune join nothing wait length"
```

Before starting recovery `seedcat` displays the configuration:

```
============ Seedcat Configuration ============
P2PKH (Legacy) Address: 1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD

Derivations: 2
 Begin: m/0/0
 End:   m/44'/0'/0'/0/0

Seeds: 8.59B
 Begin: abandon,abandon,abandon,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
 End:   zoo,zoo,zoo,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length

Total Guesses: 17.2B
```

`Address` can be either `Master XPUB`, `P2PKH`, `P2SH-P2WPKH`, or `P2WPKH`
- The address is determined based on whether it starts with `xpub661MyMwAqRbc`, `1`, `3`, or `bc1` respectively
- We recommend using `XPUB` which offers ~2x the speed and works on non-standard derivation paths and scripts
- Standard derivation paths are chosen that assume you provided your first wallet address (a path ending in `/0`)
- For custom derivation paths see the [derivations section](#derivations)

`Seeds` shows how many different combinations of seed words `seedcat` will attempt
- We are using `?` to guess all `2048` possible seed words starting at `abandon` and ending with `zoo`
- Since we are guessing 3 words with 2 derivations the `Total Guesses` is `2048 * 2048 * 2048 * 2`

`?` wildcards can be used with letters to constrain the words guessed
- For instance, the word `donkey` will be guessed with `do?` or `?key` or `?onk?`
- You can also separate different guesses with `|` such as `do?|da?`

Let's try again with constraints on the second word:
```bash
seedcat --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" --seed "? do?|da? ? ethics vapor struggle ramp dune join nothing wait length"
```
Note the number of guesses is reduced from `17.2B` to `193M` making our recovery **~90x** faster!

```
Seeds: 96.5M
 Begin: abandon,doctor,abandon,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
 End:   zoo,day,zoo,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length

Total Guesses: 193M

Continue with recovery [Y/n]?
```

Once you choose to continue you will see updates from the recovery status:
```
============ Seedcat Recovery ============
Writing Hashes 100.00% (1/1)

Waiting for GPU initialization please be patient...
* Device #1: NVIDIA GeForce RTX 3090, 22976/24237 MB, 82MCU

Recovery Guesses
 Progress: 27.85% (53.7M/193M)
 Speed....: 5.97M/sec
 GPU Speed: 187K/sec
 ETA......: 23 secs
 Elapsed..: 9 secs

Found Seed: toy,donkey,chaos,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
```

We were able to guess `donkey` as the second word alongside `toy` and `chaos`...success!

## Permuting Seeds
If you are unsure about the order of the words you can try different permutations of words.
- Use the `--combinations N` to guess every permutation with a seed phrase length of `N`
- You can pass in more than `N` words and those words will be included in the permutations
- The `^` symbol will anchor a word at its current position within the phrase

For instance, perhaps you are only sure that the first 3 words of the seed phrase are in correct order:

```bash
seedcat --combinations 12 --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" \
 --seed "^toy ^donkey ^chaos zoo vapor struggle zone nothing join ethics ramp wait length dune"
```
Note that we are passing in 14 words instead of 12 and all the words get permuted (except for the first 3 anchored words):
```
Seeds: 20.0M
 Begin: toy,donkey,chaos,zoo,vapor,struggle,zone,nothing,join,ethics,ramp,wait
 End:   toy,donkey,chaos,dune,length,wait,ramp,ethics,join,nothing,zone,struggle
```
Our result excludes the unused words `zoo` and `zone` while descrambling the rest of the phrase:
```
Found Seed: toy,donkey,chaos,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
```

Note you may use the `?` wildcard with any of the permuted or anchored words.

Using `^` anchors can greatly reduce the number of guesses that `seedcat` needs to make.

## Derivations
Derivations are chosen by default based on your address, however some wallets use non-standard derivation paths.
- If you are unsure which derivation path your address is from check [your wallet documentation](https://walletsrecovery.org/)
- Every derivation path increases the number of guesses so try to use only 1 if possible
- Or for the fastest speed use `XPUB` which doesn't use derivations at all
- The [mnemonic code converter](https://iancoleman.io/bip39/) provides an useful demo of standard address derivations

You can pass in a custom derivation path using the `--derivation` option.
- The `?` before a number will try every derivation up to that depth
- To specify a hardened path use `h` or `'` after the number
- You can try multiple derivations separated by `space` or `,`

For example, suppose you are unsure whether your wallet uses BIP32 or BIP44 and you think your address is one of the first 5 paths:
```bash
seedcat --derivation "m/0/?4 m/44h/0h/0h/0/?4" --address "1NgqeNE2EfBthz4enLb7vs1bapDEQbbivT" \
 --seed "toy donkey chaos ethics vapor struggle ramp dune join nothing wait ?"
```

This will attempt all 10 derivations `m/0/0`, `m/0/1`, ..., `m/44h/0h/0h/0/3`, `m/44h/0h/0h/0/4` which increases the number of guesses:
```
Derivations: 10
 Begin: m/0/0
 End:   m/44h/0h/0h/0/4
 
Total Guesses: 20.5K
```

Since we are guessing one word with 10 derivations the `Total Guesses` is `10 * 2048`

## Passphrase Recovery
Bitcoin passphrases (sometimes misleadingly called the 25th word) are arbitrary strings of text that are added to your seed words.  Wallets often prompt users to back up their seed words, but users may be tempted to memorize their passphrases leading to possible loss.



## Renting GPUs
If you have a large number of guesses to make and you don't have powerful hardware then renting GPUs may be your best option.



[Vast.ai](https://vast.ai/)