# quire-dict

Builds Quire's built-in dictionary, `crates/quire-ui/data/en.qdict`, from the WordNet
3.1 database. The blob is compiled into the firmware (feature `builtin-dict` of
`quire-ui`, on by default) and read in place by `quire_ui::dict::builtin`, which also
documents the `.qdict` format. The build is deterministic: the same WordNet files and
options always produce byte-identical output, so `git diff` on the blob is meaningful.

## Getting WordNet

The reader ships WordNet 3.1 as packaged by NLTK (`wordnet31.zip`; the `dict` folder of
a Princeton release works the same). Its licence is `LICENSE-wordnet.txt` at the
repository root.

```sh
curl -LO https://raw.githubusercontent.com/nltk/nltk_data/gh-pages/packages/corpora/wordnet31.zip
unzip wordnet31.zip          # → wordnet31/{index,data}.{noun,verb,adj,adv} …
```

## Building

```sh
cargo run -p quire-dict --release -- build --wordnet wordnet31 --out crates/quire-ui/data/en.qdict
cargo run -p quire-dict --release -- info crates/quire-ui/data/en.qdict
```

`build` takes a few seconds and prints the headword, gloss and vocabulary counts and the
size of every section. Options (defaults in `--help`):

| option | default | meaning |
|---|---|---|
| `--senses-per-pos N` | 1 | senses kept per part of speech (WordNet lists the commonest first) |
| `--max-senses N` | 4 | senses kept per headword across all parts of speech |
| `--gloss-chars N` | 120 | longest gloss; longer ones are cut at a word and end in `…` |
| `--segments N` | 1 | `;`-separated gloss segments kept (quoted examples are always dropped) |

With the defaults the blob is 1.55 MB for 83 253 single-word lemmas; two senses per part
of speech would be 1.81 MB, over the 1.6 MB flash budget. Only single-word lemmas are
kept (no `_`-joined phrases), and glosses are ASCII plus `…`.

`info FILE` parses a `.qdict` header and prints its counts and section layout.

## What ships

* `crates/quire-ui/data/en.qdict` — the blob, ≤ 1.6 MB.
* `crates/quire-ui/src/dict/builtin.rs` — the reader: a lookup binary-searches the
  uncompressed group index, inflates one 4 KB block at a time into a single reused
  buffer, and returns the entry as text. Peak heap per lookup is about 16 KB (the
  deflate decoder state is most of it).
* `quire-ui`'s tests (`cargo test -p quire-ui dict`) round-trip synthetic dictionaries
  through this writer and that reader, and check the shipped blob.
