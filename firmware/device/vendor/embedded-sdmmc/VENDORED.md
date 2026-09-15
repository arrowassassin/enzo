# Vendored `embedded-sdmmc`

This directory is a vendored copy of [`embedded-sdmmc`](https://crates.io/crates/embedded-sdmmc)
**0.10.0** (upstream repository: <https://github.com/rust-embedded-community/embedded-sdmmc-rs>),
carried as version `0.10.0-quire.1`. The upstream `LICENSE-APACHE` and
`LICENSE-MIT` files are kept unchanged; the crate remains `MIT OR Apache-2.0`.

Quire's device workspace (`firmware/device`) uses it as a path dependency
(`embedded-sdmmc = { path = "vendor/embedded-sdmmc", default-features = false }`)
and lists it under `workspace.exclude`, because the vendored crate is its own
workspace root so that its host-only tests and dev-dependencies stay out of the
`riscv32imc` workspace.

## Why

Upstream 0.10 can *read* VFAT long file names but cannot create them:
`open_long_name_file_in_dir` with a create mode returned `NotFound`, there was
no rename or move, and `delete_entry_in_dir` only took 8.3 names and left the
file's clusters allocated. Quire stores books under their real names, so the
fork adds full long-file-name support following the Microsoft *FAT: General
Overview of On-Disk Format* specification (LFN entries and the "Generating
Short Names" algorithm). The crate stays `no_std` and allocation free: all
buffers are bounded and on the stack, long names are at most 255 UTF-16 code
units (20 LFN entries).

## Changes from upstream 0.10.0

### New `VolumeManager` API

| Method | Purpose |
| --- | --- |
| `open_long_name_file_in_dir(dir, name, mode)` | Now **creates** the file when `mode` allows it and the name does not exist. Lookup is case-insensitive against long *and* short names. |
| `make_long_name_dir_in_dir(dir, name)` | Create a directory with a long name (`.` and `..` written). |
| `delete_long_name_entry_in_dir(dir, name)` | Delete a file or empty directory: short entry, all LFN entries and the cluster chain. |
| `rename_long_name_in_dir(dir, from, to)` | Rename within a directory, keeping cluster, size, attributes and timestamps. Case-only renames allowed. |
| `move_long_name(dir_from, from, dir_to, to)` | Move (and rename) between directories on one volume; a moved directory gets its `..` updated. Refuses to move a directory into itself/a descendant (`Unsupported`). |
| `find_long_name_entry_in_dir(dir, name) -> DirEntry` | Case-insensitive lookup by long or short name. |
| `open_long_name_dir_in_dir(parent, name) -> RawDirectory` | Open a sub-directory by long name (`.`/`..` accepted). |

`Directory` gained matching wrappers: `open_long_name_dir`,
`find_long_name_entry`, `make_long_name_dir_in_dir`,
`delete_long_name_entry_in_dir`, `rename_long_name_in_dir`.

New public type `LongName` (re-exported at the crate root, module
`filesystem::longname`): validation, `basis()` short-name generation,
`chunk()`, `needs_lfn_entries()`, `eq_ignore_case()`.

### Short name generation

* Basis name per the specification: upper-cased, spaces stripped, leading
  periods stripped, characters not allowed in 8.3 names (including all
  non-ASCII) replaced by `_`, base cut at the first period, extension taken
  after the last period (3 chars max).
* A numeric tail `~n` is added when the conversion was lossy, the name does not
  fit 8.3, or the plain basis name is already in use. The directory is scanned
  once and the lowest unused `n` (1..=1023) is chosen; beyond that a hashed
  tail (`XX1234~n`) is used.
* Names that are already valid 8.3 names (case-insensitively) get no LFN
  entries and are stored upper-case; 8.3-shaped names with mixed case inside
  the base or the extension (`MyBook.txt`) get LFN entries to preserve case.
* Trailing spaces and periods are stripped from long names (Windows
  behaviour); `"`, `*`, `/`, `:`, `<`, `>`, `?`, `\`, `|` and control
  characters are rejected.

### On-disk behaviour

* LFN entries hold 13 UCS-2 characters each, padded with a `0x0000` terminator
  and `0xFFFF`, carry the short-name checksum and sequence numbers with the
  `0x40` last-entry flag, and are written before the short entry in reverse
  order.
* A run of free entries (deleted `0xE5` entries are re-used) long enough for
  LFN + short entry is found in one pass; if the directory is full a zeroed
  cluster is appended (FAT32 root included; the fixed FAT16 root returns
  `NotEnoughSpace`).
* Renames and moves write the new entries before deleting the old ones, so an
  interruption leaves a duplicate rather than a lost file.

### Fixes to upstream code

* `delete_entry_in_dir` (short names) now also removes the entry's LFN entries
  and frees its cluster chain (upstream left orphan LFN entries and leaked the
  clusters).
* `truncate_cluster_chain` did not count the last freed cluster in the FSInfo
  free-cluster count (off by one per truncation, flagged by `fsck.fat`).
* The upstream `find_directory_entry_by_lfn` (case-sensitive, LFN only) and
  `delete_directory_entry` helpers were replaced by the new implementations.

### Files

* `src/filesystem/longname.rs` — `LongName`, `ShortNameBasis`, validation,
  chunking, checksums, case folding (pure, unit tested).
* `src/fat/longname.rs` — `FatVolume` directory cursor (`DirPos`), free-run
  search with growth, LFN-aware find/write/delete, cluster-chain freeing,
  `.`/`..` handling.
* `src/volume_mgr.rs` — the `VolumeManager` methods above.
* `tests/long_names.rs` — integration tests building FAT32 images with
  `mkfs.fat` (512-byte clusters) and verifying with `fsck.fat -n` plus raw
  directory scans; also exercises the FAT16 image in `tests/disk.img.gz`.

Run the tests on the host with:

```
cargo test --manifest-path firmware/device/vendor/embedded-sdmmc/Cargo.toml --target x86_64-unknown-linux-gnu
```

(`mkfs.fat` and `fsck.fat` from dosfstools must be installed.)
