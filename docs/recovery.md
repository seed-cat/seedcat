# Seed Recovery
Any recovery requires a [bitcoin address](https://en.bitcoin.it/wiki/Invoice_address) and some seed words from your wallet.

For example if you memorized your seed words but can't remember the first 3 words then you could use the `?` wildcard:
```bash
seedcat --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" \
 --seed "? ? ? ethics vapor struggle ramp dune join nothing wait length"
```

Before starting recovery `seedcat` displays the configuration preview:

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
- If you are unsure which derivation path your address is from check [your wallet documentation](https://walletsrecovery.org/)
- For custom derivation paths see the [derivations section](#derivations)

`Seeds` shows how many different combinations of seed words `seedcat` will attempt
- We are using `?` to guess all `2048` possible seed words starting at `abandon` and ending with `zoo`
- Since we are guessing 3 words with 2 derivations the `Total Guesses` is `2048 * 2048 * 2048 * 2`

`?` wildcards can be used with letters to constrain the words guessed
- For instance, the word `donkey` will be guessed with `do?` or `?key` or `?onk?`
- You can also separate different guesses with `|` such as `do?|da?`

With today's hardware if you are completely missing more than 4 seed words then recovery is impossible.
If you know some information about the missing seed words (such as the first letter) then recovery should be possible.

Let's try again with constraints on the second word:
```bash
seedcat --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" \
 --seed "? do?|da? ? ethics vapor struggle ramp dune join nothing wait length"
```

Use constraints to make seed recovery faster:

```
Seeds: 96.5M
 Begin: abandon,doctor,abandon,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
 End:   zoo,day,zoo,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length

Total Guesses: 193M

Continue with recovery [Y/n]?
```

The number of guesses is reduced from `17.2B` to `193M` making our recovery **~90x** faster!

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

# Permuting Seeds
If you are unsure about the order of the words you can try different permutations of words.
- Use the `--combinations N` to guess every permutation with a seed phrase length of `N`
- You can pass in more than `N` words and those words will be included in the permutations
- The `^` symbol will anchor a word at its current position within the phrase

For instance, perhaps you are only sure that the first 3 words of the seed phrase are in correct order:

```bash
seedcat --address "1AtD3g5AmR4fMsCRa1haNGmvCTVWq7YfzD" \
 --combinations 12 --seed "^toy ^donkey ^chaos zoo vapor struggle zone nothing join ethics ramp wait length dune"
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

Using `^` anchors greatly reduces the number of guesses that `seedcat` needs to make.

# Passphrase Recovery
Bitcoin passphrases (sometimes misleadingly called the 25th word) are arbitrary strings of text that are added to your seed words.

Wallets often prompt users to back up their seed words, but users may be tempted to memorize their passphrases leading to possible loss.

The `--passphrase` option allows you to specify how to attack the passphrase
- **Mask attacks** allow you to use [hashcat wildcards](https://hashcat.net/wiki/doku.php?id=mask_attack) such as `?d` for digits and `?l` for lowercase letters
- **Dictionary attacks** allow you to specify newline-separated text files containing words to try
- You can use the `--passphrase` option twice to combine attacks
- Guessing both seed words and passphrases is possible but multiplies the number of guesses

## Mask attacks
If you need to guess a passphrase `"secret"` followed by 3 digits using `--passphrase` argument:

```bash
seedcat --address "1Aa7DosYfoYJwZDmMPPTqtH7dXUehYbyMu" \
 --seed "toy donkey chaos ethics vapor struggle ramp dune join nothing wait length" \
 --passphrase "secret?d?d?d"
```

Just as with seed guessing we get a preview of what passphrases will be guessed:
```
Passphrases: 1.00K
 Begin: secret000
 End:   secret999
```

If the recovery is successful then the passphrase will be output alongside  the seed:
```
Found Seed: toy,donkey,chaos,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
Found Passphrase: secret123
```

## Dictionary attacks
Dictionary attacks require you have a text file in the `seedcat` folder.  We provide english dictionaries of various lengths (sorted by word frequency) in the `seedcat/dicts` folder you can use.
- Specify a dictionary file using the relative path starting with `./` and separated by `/`
- We use this format regardless of your platform so that commands are portable
- To separate multiple dictionaries or add text delimiters use `,`

If you want to guess 1 lowercase word and 1 uppercase word separated by `"-"` using the `--passphrase` argument:
```bash
seedcat --address "1CahNjsc2Lw46q1WgvmbQYkLon4NvHhcYw" \
 --seed "toy donkey chaos ethics vapor struggle ramp dune join nothing wait length" \
 --passphrase "./dicts/1k.txt,-,./dicts/1k_upper.txt"
```

Since both files contain `1000` words the number of guesses will be `1000 * 1000`:
````
Passphrases: 1.00M
Begin: the-THE
End:   entry-ENTRY
````

The result:
```
Found Seed: toy,donkey,chaos,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
Found Passphrase: best-PRACTICE
```

Note that a single dictionary attack is limited to 1 billion guesses.

## Combining attacks
You may wish to combine attacks to try a dictionary of words followed by wildcards or to combine 2 dictionary attacks.

For example if you want to guess 3 letters followed by `" "` and an unknown word:
```bash
seedcat --address "1CUFN2jAH3FVcBUU1r4qadHnhvo7Ywsi1v" \
 --seed "toy donkey chaos ethics vapor struggle ramp dune join nothing wait length" \
 --passphrase "?u?u?u " --passphrase "./dicts/1k_cap.txt"
```

The preview reveals we are guessing `A-Z` followed by a word from the dictionary:
```
Passphrases: 17.6M
 Begin: AAA the
 End:   ZZZ entry
```

The result:
```
Found Seed: toy,donkey,chaos,ethics,vapor,struggle,ramp,dune,join,nothing,wait,length
Found Passphrase: ABC Books
```

# Derivations
Derivations are chosen by default based on your address, however some wallets use non-standard derivation paths.
- Every derivation path increases the number of guesses so try to use only 1 if possible
- Or for the fastest speed use `XPUB` which doesn't use derivations at all
- The [mnemonic code converter](https://iancoleman.io/bip39/) provides an useful demo of standard address derivations

You can pass in a custom derivation path using the `--derivation` option.
- The `?` before a number will try every derivation up to that depth
- To specify a hardened path use `h` or `'` after the number
- You can try multiple derivations separated by `space` or `,`

For example, suppose you are unsure whether your wallet uses BIP32 or BIP44 and you think your address is one of the first 5 paths:
```bash
seedcat --address "1NgqeNE2EfBthz4enLb7vs1bapDEQbbivT" \
 --seed "toy donkey chaos ethics vapor struggle ramp dune join nothing wait ?" \
 --derivation "m/0/?4 m/44h/0h/0h/0/?4"
```

This will attempt all 10 derivations `m/0/0`, `m/0/1`, ..., `m/44h/0h/0h/0/3`, `m/44h/0h/0h/0/4` which increases the number of guesses:
```
Derivations: 10
 Begin: m/0/0
 End:   m/44h/0h/0h/0/4
 
Total Guesses: 20.5K
```

Since we are guessing one word with 10 derivations the `Total Guesses` is `10 * 2048`
