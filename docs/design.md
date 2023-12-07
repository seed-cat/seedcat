# seedcat design
What makes `seedcat` the world's fastest seed word and passphrase recovery tool?

`seedcat` consists of two components:
- A backend in C that leverages the GPU-optimized algorithms from being a [hashcat](https://hashcat.net/wiki/) module
- A frontend CLI in Rust that simplifies the recovery for users and generates valid seeds in parallel

## hashcat backend
The most expensive part of generating private keys from seed phrases is 2048 iterations of the PBKDF2-HMAC-SHA512 algorithm.  Luckily we can easily run this algorithm in parallel on GPUs and hashcat already has the fastest implementation.

If the user has the master XPUB we already have 128-bits of entropy we can check our hash against.  Otherwise we need to perform some ECC operations that are also optimized in hashcat.

The most complicated aspect of the module is the guessing of seed words because we need to take advantage of the [BIP-39 checksum](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki#generating-the-mnemonic).  With 12 seed words we can filter for 1/16 the seed phrases and with 24 seed words we can filter for 1/256 the seed phrases, translating to a 10-100x improvement in speed.

Unfortunately GPUs do not perform well with branching code that would be required to filter seeds so we perform filtering on the CPU and send it to the hashcat module running on the GPU through stdin or the hashes file.

## seedcat frontend
The frontend determines the fastest way we can run the recovery.  There are 3 modes that `seedcat` can run in:
- **Pure GPU** - if performing a passphrase attack with <10M valid seeds we pregenerate all valid seeds and put them in the hashes file
- **Binary charsets** - if the last word is `?` then we can pass in the seed entropy directly and no seed filtering is required since we can quickly generate the checksum on the GPU
- **Stdin Mode** - if we are generating many seeds and the last word is constrained then we filter seeds on parallel in the frontend and send them to the GPU module via stdin in.  In this mode we end up CPU-bound if running on a large GPU cluster so we try to avoid it when possible.

Generating and filtering valid seeds also needs to be multithreaded and fast so we wrote highly-optimized Rust code that allows us to parallelize the work.  In order to split the work across threads we perform some tricks such as using lexicographic seed word permutations that allow us to split a large permutation in O(1) time.

The rest of the frontend is dedicated to providing a more user-friendly UX.  For instance we validate user inputs, provide total counts, and examples so a user can understand what is actually being guessed.

## benchmarks
Of course we cannot claim to be the world's fastest recovery tool without some comparisons.

As far as we could find there were only two GPU-optimized versions of seed phrase recovery ever written: [btcrecover](https://github.com/gurnec/btcrecover) and John Cantrell's [one-off implementation](https://medium.com/@johncantrell97/how-i-checked-over-1-trillion-mnemonics-in-30-hours-to-win-a-bitcoin-635fe051a752).  Since both implementations have fallen out of maintenance, we already can provide a better user experience that is easier to get working.

In fact we had trouble getting either implementation running with CUDA so we had to rely on their self-reported benchmarks and ran comparisons on similar hardware (A2000 for btcrecover and 2080 TI for Cantrell).

| Test          | Implementation | Relative Speed |
|---------------|----------------|----------------|
| 12 seed words | btcrecover     | 1.0x           |
| 12 seed words | johncantrell97 | 4.3x           |
| 12 seed words | **seedcat**    | **9.7x**       |
| 24 seed words | btcrecover     | 1.0x           |
| 24 seed words | **seedcat**    | **103.9x**     |
| Passphrase    | btcrecover     | 1.0x           |
| Passphrase    | **seedcat**    | **71.3x**      |

We can see that `seedcat` offers around a **10-100x** improvement in speed.   The improvement against btcrecover is likely because it isn't filtering invalid checksums in a fast way and its passphrase attack isn't GPU-optimized.

Against the johncantrell97 implementation the speed-up is smaller and occurs due to slightly better optimized algorithms.  Note that if we attack the master XPUB instead of the address we could gain an additional 2x speed-up.  His implementation also was written as a one-off to win a contest so doesn't support passphrases or any other kinds of attack.

In comparisons against CPU-only recovery tools we generally see a **>50x** improvement in speed which easily gets much higher if running on powerful GPU clusters.  On a 8x 4090 RTX cluster we measured a **447x** improvement over a CPU-based implementation.