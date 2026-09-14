//! Long file name creation, deletion, renaming and moving.
//!
//! FAT32 images are built with `mkfs.fat` (512-byte clusters, so directories
//! grow after every 16 entries) and verified with `fsck.fat -n` afterwards.
//! Directory contents are also checked by scanning the raw on-disk entries,
//! independently of the crate.

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::process::Command;

use embedded_sdmmc::{
    Error, LfnBuffer, LongName, Mode, RawDirectory, RawVolume, ShortFileName, VolumeIdx,
    VolumeManager,
};

mod utils;

/// Assert that a volume manager call failed with the given error.
macro_rules! assert_err {
    ($e:expr, $p:pat) => {
        match $e {
            Err($p) => {}
            other => panic!(
                "expected {}, got {:?}",
                stringify!($p),
                other.map(|_| ())
            ),
        }
    };
}

type Vm = VolumeManager<utils::RamDisk<Vec<u8>>, utils::TestTimeSource, 8, 8, 1>;

// ****************************************************************************
//
// Image helpers
//
// ****************************************************************************

fn tool(name: &str) -> PathBuf {
    for dir in [
        "/usr/sbin",
        "/sbin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/opt/homebrew/sbin",
    ] {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

fn scratch_path(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("lfn-{}-{}.img", label, std::process::id()))
}

/// Build a fresh FAT32 image (with a fake MBR) using `mkfs.fat`.
fn fat32_image(label: &str) -> Vec<u8> {
    let path = scratch_path(label);
    let _ = std::fs::remove_file(&path);
    let output = Command::new(tool("mkfs.fat"))
        .args(["-C", "-F", "32", "-s", "1", "-S", "512", "-n", "QUIRE", "--mbr=y"])
        .arg(&path)
        .arg("40000")
        .output()
        .expect("mkfs.fat must be installed (dosfstools)");
    assert!(
        output.status.success(),
        "mkfs.fat failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let image = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    image
}

/// Check an image with `fsck.fat -n`, which must find nothing to fix.
fn fsck(image: &[u8], label: &str) {
    let path = scratch_path(&format!("{label}-fsck"));
    std::fs::write(&path, image).unwrap();
    let output = Command::new(tool("fsck.fat"))
        .args(["-n", "-V", "-v"])
        .arg(&path)
        .output()
        .expect("fsck.fat must be installed (dosfstools)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "fsck.fat -n reported errors for {label}:\n{stdout}\n{stderr}"
    );
    std::fs::remove_file(&path).unwrap();
}

fn open_image(image: Vec<u8>) -> (Vm, RawVolume, RawDirectory) {
    let disk = utils::RamDisk::new(image);
    let vm: Vm = VolumeManager::new_with_limits(disk, utils::make_time_source(), 0xAA00_0000);
    let volume = vm.open_raw_volume(VolumeIdx(0)).expect("open volume");
    let root = vm.open_root_dir(volume).expect("open root");
    (vm, volume, root)
}

fn close_image(vm: Vm, volume: RawVolume, root: RawDirectory) -> Vec<u8> {
    vm.close_dir(root).expect("close root");
    vm.close_volume(volume).expect("close volume");
    assert!(!vm.has_open_handles());
    let (disk, _) = vm.free();
    disk.into_inner()
}

/// A copy of the disk contents while the volume manager is still in use.
fn snapshot(vm: &Vm) -> Vec<u8> {
    vm.device(|d| {
        use embedded_sdmmc::BlockDevice;
        let mut copy = Vec::new();
        let blocks = d.num_blocks().unwrap().0;
        let mut block = [embedded_sdmmc::Block::new()];
        for i in 0..blocks {
            d.read(&mut block, embedded_sdmmc::BlockIdx(i)).unwrap();
            copy.extend_from_slice(block[0].as_slice());
        }
        copy
    })
}

// ****************************************************************************
//
// File helpers
//
// ****************************************************************************

fn create_file(vm: &Vm, dir: RawDirectory, name: &str, contents: &[u8]) {
    let f = vm
        .open_long_name_file_in_dir(dir, name, Mode::ReadWriteCreate)
        .unwrap_or_else(|e| panic!("create {name:?}: {e:?}"));
    if !contents.is_empty() {
        vm.write(f, contents).unwrap();
    }
    vm.close_file(f).unwrap();
}

fn read_all(vm: &Vm, f: embedded_sdmmc::RawFile) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 700];
    while !vm.file_eof(f).unwrap() {
        let n = vm.read(f, &mut buf).unwrap();
        out.extend_from_slice(&buf[..n]);
    }
    vm.close_file(f).unwrap();
    out
}

fn read_file(vm: &Vm, dir: RawDirectory, name: &str) -> Vec<u8> {
    let f = vm
        .open_long_name_file_in_dir(dir, name, Mode::ReadOnly)
        .unwrap_or_else(|e| panic!("open {name:?}: {e:?}"));
    read_all(vm, f)
}

fn read_file_short(vm: &Vm, dir: RawDirectory, name: ShortFileName) -> Vec<u8> {
    let f = vm
        .open_file_in_dir(dir, name, Mode::ReadOnly)
        .unwrap_or_else(|e| panic!("open {name}: {e:?}"));
    read_all(vm, f)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    /// The long name if the entry has one, otherwise the short name
    name: String,
    short: ShortFileName,
    is_dir: bool,
    size: u32,
}

/// List a directory (skipping `.`, `..` and the volume label).
fn list(vm: &Vm, dir: RawDirectory) -> Vec<Listed> {
    let mut storage = [0u8; 768];
    let mut lfn = LfnBuffer::new(&mut storage);
    let mut out = Vec::new();
    vm.iterate_dir_lfn(dir, &mut lfn, |entry, long| {
        if entry.attributes.is_volume() || entry.attributes.is_lfn() {
            return ControlFlow::Continue(());
        }
        if entry.name == ShortFileName::this_dir() || entry.name == ShortFileName::parent_dir() {
            return ControlFlow::Continue(());
        }
        out.push(Listed {
            name: long.map(String::from).unwrap_or_else(|| entry.name.to_string()),
            short: entry.name,
            is_dir: entry.attributes.is_directory(),
            size: entry.size,
        });
        ControlFlow::Continue(())
    })
    .unwrap();
    out
}

fn sfn(s: &str) -> ShortFileName {
    ShortFileName::create_from_str(s).unwrap()
}

// ****************************************************************************
//
// Raw on-disk helpers (independent of the crate)
//
// ****************************************************************************

struct Layout {
    sectors_per_cluster: u32,
    fat_start: u64,
    data_start: u64,
    root_cluster: u32,
}

fn layout(image: &[u8]) -> Layout {
    let u16_at = |i: usize| u16::from_le_bytes([image[i], image[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([image[i], image[i + 1], image[i + 2], image[i + 3]]);
    assert_eq!(u16_at(11), 512);
    let sectors_per_cluster = u32::from(image[13]);
    let reserved = u32::from(u16_at(14));
    let num_fats = u32::from(image[16]);
    let fat_size = u32_at(36);
    Layout {
        sectors_per_cluster,
        fat_start: u64::from(reserved) * 512,
        data_start: u64::from(reserved + num_fats * fat_size) * 512,
        root_cluster: u32_at(44),
    }
}

fn fat_entry(image: &[u8], layout: &Layout, cluster: u32) -> u32 {
    let i = (layout.fat_start + u64::from(cluster) * 4) as usize;
    u32::from_le_bytes([image[i], image[i + 1], image[i + 2], image[i + 3]]) & 0x0FFF_FFFF
}

fn chain(image: &[u8], layout: &Layout, first: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cluster = first;
    while (2..0x0FFF_FFF8).contains(&cluster) {
        out.push(cluster);
        cluster = fat_entry(image, layout, cluster);
        assert!(out.len() < 100_000, "runaway chain");
    }
    out
}

/// Every 32-byte entry of a directory, in on-disk order.
fn raw_dir(image: &[u8], first_cluster: u32) -> Vec<[u8; 32]> {
    let layout = layout(image);
    let mut out = Vec::new();
    for cluster in chain(image, &layout, first_cluster) {
        let start = (layout.data_start
            + u64::from(cluster - 2) * u64::from(layout.sectors_per_cluster) * 512)
            as usize;
        let len = (layout.sectors_per_cluster * 512) as usize;
        for entry in image[start..start + len].chunks_exact(32) {
            out.push(entry.try_into().unwrap());
        }
    }
    out
}

fn raw_cluster(entry: &[u8; 32]) -> u32 {
    (u32::from(u16::from_le_bytes([entry[20], entry[21]])) << 16)
        | u32::from(u16::from_le_bytes([entry[26], entry[27]]))
}

fn raw_find(entries: &[[u8; 32]], short: ShortFileName) -> Option<[u8; 32]> {
    let name = format!(
        "{:<8}{:<3}",
        String::from_utf8_lossy(short.base_name()),
        String::from_utf8_lossy(short.extension())
    );
    entries
        .iter()
        .find(|e| e[0] != 0xE5 && e[0] != 0x00 && e[11] != 0x0F && &e[..11] == name.as_bytes())
        .copied()
}

fn lfn_checksum(short: &[u8]) -> u8 {
    short
        .iter()
        .fold(0u8, |sum, &b| sum.rotate_right(1).wrapping_add(b))
}

/// Verify that the raw directory is well formed: every LFN entry belongs to a
/// complete, correctly ordered run immediately followed by a live short entry
/// with the matching checksum. Returns (live LFN entries, live short entries).
fn check_raw_dir(entries: &[[u8; 32]]) -> (usize, usize) {
    let mut lfn_count = 0;
    let mut short_count = 0;
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        if e[0] == 0x00 {
            // end of directory: everything after must be unused too
            assert!(
                entries[i..].iter().all(|e| e[0] == 0x00),
                "entries after the end marker at {i}"
            );
            break;
        }
        if e[0] == 0xE5 {
            i += 1;
            continue;
        }
        if e[11] == 0x0F {
            assert_ne!(
                e[0] & 0x40,
                0,
                "LFN run at {i} does not start with the last-entry flag"
            );
            let n = usize::from(e[0] & 0x1F);
            let csum = e[13];
            assert!((1..=20).contains(&n), "bad LFN sequence number at {i}");
            for k in 0..n {
                let part = &entries[i + k];
                assert_eq!(part[11], 0x0F, "LFN run at {i} interrupted at {}", i + k);
                assert_eq!(
                    usize::from(part[0] & 0x1F),
                    n - k,
                    "LFN order wrong at {}",
                    i + k
                );
                assert_eq!(
                    part[13],
                    csum,
                    "LFN checksum differs within run at {}",
                    i + k
                );
                assert_eq!(part[26..28], [0, 0], "LFN entry has a cluster at {}", i + k);
                // Unused character slots: 0x0000 terminator then 0xFFFF padding
                let units: Vec<u16> = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
                    .iter()
                    .map(|&o| u16::from_le_bytes([part[o], part[o + 1]]))
                    .collect();
                if let Some(term) = units.iter().position(|&u| u == 0x0000) {
                    assert!(
                        units[term + 1..].iter().all(|&u| u == 0xFFFF),
                        "LFN padding after terminator is not 0xFFFF at {}",
                        i + k
                    );
                }
            }
            let short = &entries[i + n];
            assert!(
                short[0] != 0xE5 && short[0] != 0x00 && short[11] != 0x0F,
                "orphan LFN run at {i}: no live short entry follows"
            );
            assert_eq!(
                lfn_checksum(&short[..11]),
                csum,
                "LFN checksum mismatch at {i}"
            );
            lfn_count += n;
            short_count += 1;
            i += n + 1;
        } else {
            short_count += 1;
            i += 1;
        }
    }
    (lfn_count, short_count)
}

// ****************************************************************************
//
// Tests
//
// ****************************************************************************

#[test]
fn fat32_three_hundred_long_names_grow_the_root_directory() {
    let (vm, volume, root) = open_image(fat32_image("many"));
    let names: Vec<String> = (0..300)
        .map(|i| format!("Quire Library Book Number {i:03} - A Reasonably Long Title.epub"))
        .collect();
    for (i, name) in names.iter().enumerate() {
        create_file(&vm, root, name, format!("book {i}").as_bytes());
    }

    // Everything is listed, once, with its long name
    let listed = list(&vm, root);
    assert_eq!(listed.len(), 300);
    let listed_names: BTreeSet<&str> = listed.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(listed_names.len(), 300);
    for name in &names {
        assert!(listed_names.contains(name.as_str()), "missing {name}");
    }

    // All the short names share one basis and get tails ~1 .. ~300
    let shorts: BTreeSet<ShortFileName> = listed.iter().map(|l| l.short).collect();
    assert_eq!(shorts.len(), 300, "short names must be unique");
    let basis = LongName::new(&names[0]).unwrap().basis();
    assert!(basis.tail_required());
    let expected: BTreeSet<ShortFileName> = (1..=300).map(|n| basis.with_tail(n)).collect();
    assert_eq!(shorts, expected);
    assert!(shorts.contains(&sfn("QUIREL~1.EPU")));
    assert!(shorts.contains(&sfn("QUIRE~10.EPU")));
    assert!(shorts.contains(&sfn("QUIR~300.EPU")));

    // Read back by long name and by generated short name
    for (i, name) in names.iter().enumerate() {
        let entry = vm.find_long_name_entry_in_dir(root, name).unwrap();
        assert_eq!(entry.size as usize, format!("book {i}").len());
        assert_eq!(read_file(&vm, root, name), format!("book {i}").as_bytes());
        assert_eq!(
            read_file_short(&vm, root, entry.name),
            format!("book {i}").as_bytes()
        );
    }
    // Case-insensitive lookup
    assert!(
        vm.find_long_name_entry_in_dir(root, &names[42].to_uppercase())
            .is_ok()
    );
    assert_err!(
        vm.find_long_name_entry_in_dir(
            root,
            "Quire Library Book Number 300 - A Reasonably Long Title.epub"
        ),
        Error::NotFound
    );

    let image = close_image(vm, volume, root);
    let layout = layout(&image);
    // 300 files x (5 LFN + 1 short) entries at 16 entries per cluster
    assert!(chain(&image, &layout, layout.root_cluster).len() >= 112);
    let (lfn_entries, short_entries) = check_raw_dir(&raw_dir(&image, layout.root_cluster));
    assert_eq!(lfn_entries, 300 * 5);
    assert_eq!(short_entries, 300 + 1); // + volume label
    fsck(&image, "many");
}

#[test]
fn fat32_colliding_names_get_distinct_numeric_tails() {
    let (vm, volume, root) = open_image(fat32_image("collide"));
    create_file(&vm, root, "My Book.epub", b"first");
    create_file(&vm, root, "My Book (1).epub", b"second");
    create_file(&vm, root, "My Book (2).epub", b"third");

    let listed = list(&vm, root);
    let short_of = |name: &str| listed.iter().find(|l| l.name == name).unwrap().short;
    assert_eq!(short_of("My Book.epub"), sfn("MYBOOK~1.EPU"));
    assert_eq!(short_of("My Book (1).epub"), sfn("MYBOOK~2.EPU"));
    assert_eq!(short_of("My Book (2).epub"), sfn("MYBOOK~3.EPU"));

    // Read back by long name (any case) and by short name
    assert_eq!(read_file(&vm, root, "my book (1).EPUB"), b"second");
    assert_eq!(read_file_short(&vm, root, sfn("MYBOOK~2.EPU")), b"second");
    assert_eq!(read_file(&vm, root, "MYBOOK~3.EPU"), b"third");
    assert_eq!(read_file(&vm, root, "mybook~1.epu"), b"first");

    // Creating an existing name (in another case) fails; opening it works
    assert_err!(
        vm.open_long_name_file_in_dir(root, "MY BOOK.EPUB", Mode::ReadWriteCreate),
        Error::FileAlreadyExists
    );
    let f = vm
        .open_long_name_file_in_dir(root, "MY BOOK.EPUB", Mode::ReadWriteCreateOrAppend)
        .unwrap();
    vm.write(f, b"+more").unwrap();
    vm.close_file(f).unwrap();
    assert_eq!(read_file(&vm, root, "My Book.epub"), b"first+more");
    let f = vm
        .open_long_name_file_in_dir(root, "My Book (2).epub", Mode::ReadWriteCreateOrTruncate)
        .unwrap();
    vm.write(f, b"new").unwrap();
    vm.close_file(f).unwrap();
    assert_eq!(read_file(&vm, root, "My Book (2).epub"), b"new");
    assert_err!(
        vm.open_long_name_file_in_dir(root, "No Such Book.epub", Mode::ReadOnly),
        Error::NotFound
    );
    assert_eq!(list(&vm, root).len(), 3);

    // Invalid long names are rejected
    for bad in ["", ".", "..", "a/b", "a:b", "a?b", "a\"b", "a|b"] {
        assert!(
            matches!(
                vm.open_long_name_file_in_dir(root, bad, Mode::ReadWriteCreate),
                Err(Error::FilenameError(_))
            ),
            "{bad:?} should be rejected"
        );
    }

    let image = close_image(vm, volume, root);
    let layout = layout(&image);
    let (lfn_entries, _) = check_raw_dir(&raw_dir(&image, layout.root_cluster));
    assert_eq!(lfn_entries, 1 + 2 + 2);
    fsck(&image, "collide");
}

#[test]
fn fat32_mixed_case_83_names_keep_their_case() {
    let (vm, volume, root) = open_image(fat32_image("case"));
    create_file(&vm, root, "MyBook.txt", b"mixed");
    create_file(&vm, root, "README.TXT", b"upper");
    create_file(&vm, root, "notes.md", b"lower");
    create_file(&vm, root, "Mixed", b"noext");

    let listed = list(&vm, root);
    let find = |name: &str| listed.iter().find(|l| l.name == name).cloned();
    assert_eq!(find("MyBook.txt").unwrap().short, sfn("MYBOOK.TXT"));
    assert_eq!(find("README.TXT").unwrap().short, sfn("README.TXT"));
    // Uniformly cased 8.3 names are stored as short names only
    assert_eq!(find("NOTES.MD").unwrap().short, sfn("NOTES.MD"));
    assert!(find("notes.md").is_none());
    assert_eq!(find("Mixed").unwrap().short, sfn("MIXED"));

    // Found by any case, via long or short name
    assert_eq!(read_file(&vm, root, "mybook.TXT"), b"mixed");
    assert_eq!(read_file_short(&vm, root, sfn("MYBOOK.TXT")), b"mixed");
    assert_eq!(read_file(&vm, root, "readme.txt"), b"upper");
    assert_eq!(read_file(&vm, root, "Notes.MD"), b"lower");
    assert_eq!(read_file(&vm, root, "MIXED"), b"noext");
    assert_eq!(
        vm.find_directory_entry(root, "MYBOOK.TXT").unwrap().size,
        5
    );

    let image = close_image(vm, volume, root);
    let layout = layout(&image);
    let entries = raw_dir(&image, layout.root_cluster);
    let (lfn_entries, short_entries) = check_raw_dir(&entries);
    assert_eq!(lfn_entries, 2, "only the mixed case names get LFN entries");
    assert_eq!(short_entries, 4 + 1);
    fsck(&image, "case");
}

#[test]
fn fat32_nested_long_name_directories() {
    let (vm, volume, root) = open_image(fat32_image("nested"));
    vm.make_long_name_dir_in_dir(root, "Library").unwrap();
    assert_err!(
        vm.make_long_name_dir_in_dir(root, "library"),
        Error::DirAlreadyExists
    );
    let library = vm.open_long_name_dir_in_dir(root, "LIBRARY").unwrap();
    vm.make_long_name_dir_in_dir(library, "Science Fiction & Fantasy")
        .unwrap();
    let genre = vm
        .open_long_name_dir_in_dir(library, "science fiction & fantasy")
        .unwrap();
    vm.make_long_name_dir_in_dir(genre, "Isaac Asimov").unwrap();
    let author = vm.open_long_name_dir_in_dir(genre, "Isaac Asimov").unwrap();
    create_file(&vm, author, "Foundation (1951).epub", b"psychohistory");
    assert_err!(
        vm.make_long_name_dir_in_dir(author, "Foundation (1951).epub"),
        Error::FileAlreadyExists
    );
    assert_err!(
        vm.open_long_name_dir_in_dir(author, "Foundation (1951).epub"),
        Error::OpenedFileAsDir
    );
    assert_err!(
        vm.open_long_name_dir_in_dir(author, "Missing"),
        Error::NotFound
    );

    // `..` leads back up
    let up = vm.open_long_name_dir_in_dir(author, "..").unwrap();
    assert!(
        vm.find_long_name_entry_in_dir(up, "Isaac Asimov")
            .unwrap()
            .attributes
            .is_directory()
    );
    vm.close_dir(up).unwrap();
    let same = vm.open_long_name_dir_in_dir(author, ".").unwrap();
    assert_eq!(
        read_file(&vm, same, "foundation (1951).EPUB"),
        b"psychohistory"
    );
    vm.close_dir(same).unwrap();

    // Fill the deepest directory past its first cluster, then empty it again
    let names: Vec<String> = (0..40)
        .map(|i| format!("The Robot Series - Volume {i} of Many.epub"))
        .collect();
    for name in &names {
        create_file(&vm, author, name, b"robot");
    }
    assert_eq!(list(&vm, author).len(), 41);
    for name in &names {
        vm.delete_long_name_entry_in_dir(author, name).unwrap();
    }
    assert_eq!(list(&vm, author).len(), 1);

    let listed = list(&vm, library);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Science Fiction & Fantasy");
    assert_eq!(listed[0].short, sfn("SCIENC~1"));
    assert!(listed[0].is_dir);

    vm.close_dir(author).unwrap();
    vm.close_dir(genre).unwrap();
    vm.close_dir(library).unwrap();
    let image = close_image(vm, volume, root);

    // Raw check of `.` and `..`
    let layout = layout(&image);
    let root_entries = raw_dir(&image, layout.root_cluster);
    check_raw_dir(&root_entries);
    let library_entry = raw_find(&root_entries, sfn("LIBRARY")).unwrap();
    assert_eq!(library_entry[11] & 0x10, 0x10);
    let library_cluster = raw_cluster(&library_entry);
    let library_entries = raw_dir(&image, library_cluster);
    check_raw_dir(&library_entries);
    assert_eq!(&library_entries[0][..11], b".          ");
    assert_eq!(raw_cluster(&library_entries[0]), library_cluster);
    assert_eq!(&library_entries[1][..11], b"..         ");
    assert_eq!(raw_cluster(&library_entries[1]), 0, "parent is root");
    let genre_cluster = raw_cluster(&raw_find(&library_entries, sfn("SCIENC~1")).unwrap());
    let genre_entries = raw_dir(&image, genre_cluster);
    check_raw_dir(&genre_entries);
    assert_eq!(raw_cluster(&genre_entries[1]), library_cluster);
    let author_cluster = raw_cluster(&raw_find(&genre_entries, sfn("ISAACA~1")).unwrap());
    let author_entries = raw_dir(&image, author_cluster);
    let (lfn_entries, short_entries) = check_raw_dir(&author_entries);
    assert_eq!(short_entries, 3, ". .. and the one file");
    assert_eq!(lfn_entries, 2);
    assert!(
        chain(&image, &layout, author_cluster).len() > 1,
        "the directory grew"
    );
    fsck(&image, "nested");
}

#[test]
fn fat32_delete_leaves_no_orphan_lfn_entries_and_frees_clusters() {
    let (vm, volume, root) = open_image(fat32_image("delete"));
    let names: Vec<String> = (0..20)
        .map(|i| format!("Deletable Document Number {i} With A Long Name.txt"))
        .collect();
    let payload = vec![b'x'; 4096];
    for name in &names {
        create_file(&vm, root, name, &payload);
    }
    vm.make_long_name_dir_in_dir(root, "Some Directory").unwrap();
    let subdir = vm.open_long_name_dir_in_dir(root, "Some Directory").unwrap();
    create_file(&vm, subdir, "Inside The Directory.txt", b"inside");

    // Snapshot of where things live on disk before deleting anything
    let before = snapshot(&vm);
    let layout = layout(&before);
    let root_before = raw_dir(&before, layout.root_cluster);
    let (lfn_before, _) = check_raw_dir(&root_before);
    let root_clusters_before = chain(&before, &layout, layout.root_cluster).len();
    let deleted_first_cluster = {
        let entry = vm.find_long_name_entry_in_dir(root, &names[2]).unwrap();
        raw_cluster(&raw_find(&root_before, entry.name).unwrap())
    };
    let deleted_chain = chain(&before, &layout, deleted_first_cluster);
    assert_eq!(deleted_chain.len(), 8);

    // Refusals
    assert_err!(
        vm.delete_long_name_entry_in_dir(root, "Some Directory"),
        Error::DirAlreadyOpen
    );
    vm.close_dir(subdir).unwrap();
    assert_err!(
        vm.delete_long_name_entry_in_dir(root, "some directory"),
        Error::DeleteNonEmptyDir
    );
    let f = vm
        .open_long_name_file_in_dir(root, &names[0], Mode::ReadOnly)
        .unwrap();
    assert_err!(
        vm.delete_long_name_entry_in_dir(root, &names[0]),
        Error::FileAlreadyOpen
    );
    vm.close_file(f).unwrap();
    assert_err!(
        vm.delete_long_name_entry_in_dir(root, "Not There.txt"),
        Error::NotFound
    );

    // Delete every other file, in the other case
    for name in names.iter().step_by(2) {
        vm.delete_long_name_entry_in_dir(root, &name.to_lowercase())
            .unwrap();
    }
    // Empty and delete the directory (using the short name API on the file)
    let subdir = vm.open_long_name_dir_in_dir(root, "Some Directory").unwrap();
    let inside = vm
        .find_long_name_entry_in_dir(subdir, "Inside The Directory.txt")
        .unwrap();
    vm.delete_entry_in_dir(subdir, inside.name).unwrap();
    assert_eq!(list(&vm, subdir).len(), 0);
    vm.close_dir(subdir).unwrap();
    vm.delete_long_name_entry_in_dir(root, "Some Directory").unwrap();

    for (i, name) in names.iter().enumerate() {
        if i % 2 == 0 {
            assert_err!(vm.find_long_name_entry_in_dir(root, name), Error::NotFound);
        } else {
            assert_eq!(read_file(&vm, root, name), payload);
        }
    }
    assert_eq!(list(&vm, root).len(), 10);

    // Every cluster of the deleted file's chain is free again
    let after_delete = snapshot(&vm);
    for cluster in &deleted_chain {
        assert_eq!(fat_entry(&after_delete, &layout, *cluster), 0);
    }

    // New files re-use the freed entries: the root does not grow
    for i in 0..10 {
        create_file(&vm, root, &format!("Replacement File Number {i}.txt"), b"r");
    }
    assert_eq!(list(&vm, root).len(), 20);

    let image = close_image(vm, volume, root);
    let root_after = raw_dir(&image, layout.root_cluster);
    let (lfn_after, short_after) = check_raw_dir(&root_after);
    assert_eq!(short_after, 20 + 1);
    let per_name = LongName::new(&names[0]).unwrap().num_entries();
    let per_replacement = LongName::new("Replacement File Number 0.txt")
        .unwrap()
        .num_entries();
    assert_eq!(lfn_before, 20 * per_name + 2);
    assert_eq!(lfn_after, 10 * per_name + 10 * per_replacement);
    assert_eq!(
        chain(&image, &layout, layout.root_cluster).len(),
        root_clusters_before,
        "deleted entries were re-used"
    );
    fsck(&image, "delete");
}

#[test]
fn fat32_rename_and_move() {
    let (vm, volume, root) = open_image(fat32_image("rename"));
    create_file(&vm, root, "Draft Chapter.txt", b"once upon a time");
    create_file(&vm, root, "Other File.txt", b"other");
    let original = vm
        .find_long_name_entry_in_dir(root, "Draft Chapter.txt")
        .unwrap();

    // Rename in place
    vm.rename_long_name_in_dir(root, "draft chapter.TXT", "Final Chapter v2.txt")
        .unwrap();
    assert_err!(
        vm.find_long_name_entry_in_dir(root, "Draft Chapter.txt"),
        Error::NotFound
    );
    let renamed = vm
        .find_long_name_entry_in_dir(root, "Final Chapter v2.txt")
        .unwrap();
    assert_eq!(renamed.cluster, original.cluster);
    assert_eq!(renamed.size, original.size);
    assert_eq!(renamed.ctime, original.ctime);
    assert_eq!(renamed.mtime, original.mtime);
    assert_eq!(renamed.name, sfn("FINALC~1.TXT"));
    assert_eq!(
        read_file(&vm, root, "Final Chapter v2.txt"),
        b"once upon a time"
    );

    // Case-only rename
    vm.rename_long_name_in_dir(root, "Final Chapter v2.txt", "FINAL CHAPTER V2.TXT")
        .unwrap();
    assert!(
        list(&vm, root)
            .iter()
            .any(|l| l.name == "FINAL CHAPTER V2.TXT")
    );
    assert_eq!(
        read_file(&vm, root, "final chapter v2.txt"),
        b"once upon a time"
    );

    // Rename to a name that is 8.3 drops the LFN entries
    vm.rename_long_name_in_dir(root, "FINAL CHAPTER V2.TXT", "FINAL.TXT")
        .unwrap();
    assert_eq!(
        vm.find_directory_entry(root, "FINAL.TXT").unwrap().cluster,
        original.cluster
    );

    // Refusals
    assert_err!(
        vm.rename_long_name_in_dir(root, "FINAL.TXT", "other file.txt"),
        Error::FileAlreadyExists
    );
    assert_err!(
        vm.rename_long_name_in_dir(root, "Missing.txt", "X.txt"),
        Error::NotFound
    );
    let f = vm
        .open_long_name_file_in_dir(root, "FINAL.TXT", Mode::ReadOnly)
        .unwrap();
    assert_err!(
        vm.rename_long_name_in_dir(root, "FINAL.TXT", "Y.txt"),
        Error::FileAlreadyOpen
    );
    vm.close_file(f).unwrap();

    // Move a file into a directory
    vm.make_long_name_dir_in_dir(root, "Archive").unwrap();
    let archive = vm.open_long_name_dir_in_dir(root, "Archive").unwrap();
    vm.move_long_name(root, "final.txt", archive, "Moved Chapter.txt")
        .unwrap();
    assert_err!(
        vm.find_long_name_entry_in_dir(root, "FINAL.TXT"),
        Error::NotFound
    );
    let moved = vm
        .find_long_name_entry_in_dir(archive, "Moved Chapter.txt")
        .unwrap();
    assert_eq!(moved.cluster, original.cluster);
    assert_eq!(
        read_file(&vm, archive, "moved chapter.txt"),
        b"once upon a time"
    );
    assert_err!(
        vm.move_long_name(root, "Other File.txt", archive, "Moved Chapter.txt"),
        Error::FileAlreadyExists
    );
    // Same directory through two handles is a rename
    let root2 = vm.open_long_name_dir_in_dir(archive, "..").unwrap();
    vm.move_long_name(root, "Other File.txt", root2, "Other File (renamed).txt")
        .unwrap();
    vm.close_dir(root2).unwrap();
    assert_eq!(read_file(&vm, root, "Other File (renamed).txt"), b"other");

    // Move a directory: its `..` must follow
    vm.make_long_name_dir_in_dir(root, "Series").unwrap();
    let series = vm.open_long_name_dir_in_dir(root, "Series").unwrap();
    create_file(&vm, series, "Book 1.epub", b"one");
    vm.close_dir(series).unwrap();
    assert_err!(
        vm.move_long_name(root, "Archive", archive, "Archive Inside Itself"),
        Error::Unsupported
    );
    vm.move_long_name(root, "Series", archive, "Series (moved)")
        .unwrap();
    assert_err!(
        vm.open_long_name_dir_in_dir(root, "Series"),
        Error::NotFound
    );
    let series = vm
        .open_long_name_dir_in_dir(archive, "Series (moved)")
        .unwrap();
    assert_eq!(read_file(&vm, series, "Book 1.epub"), b"one");
    let parent = vm.open_long_name_dir_in_dir(series, "..").unwrap();
    assert!(
        vm.find_long_name_entry_in_dir(parent, "Moved Chapter.txt")
            .is_ok(),
        "'..' of the moved directory points at Archive"
    );
    vm.close_dir(parent).unwrap();
    // A directory can't be moved below itself
    assert_err!(
        vm.move_long_name(root, "Archive", series, "Nope"),
        Error::Unsupported
    );
    vm.close_dir(series).unwrap();

    assert_eq!(list(&vm, root).len(), 2);
    assert_eq!(list(&vm, archive).len(), 2);
    vm.close_dir(archive).unwrap();

    let image = close_image(vm, volume, root);
    let layout = layout(&image);
    let root_entries = raw_dir(&image, layout.root_cluster);
    check_raw_dir(&root_entries);
    let archive_cluster = raw_cluster(&raw_find(&root_entries, sfn("ARCHIVE")).unwrap());
    let archive_entries = raw_dir(&image, archive_cluster);
    check_raw_dir(&archive_entries);
    let series_cluster = raw_cluster(&raw_find(&archive_entries, sfn("SERIES~1")).unwrap());
    let series_entries = raw_dir(&image, series_cluster);
    check_raw_dir(&series_entries);
    assert_eq!(&series_entries[1][..11], b"..         ");
    assert_eq!(raw_cluster(&series_entries[1]), archive_cluster);
    fsck(&image, "rename");
}

#[test]
fn fat32_directory_wrapper_api() {
    let (vm, volume, root) = open_image(fat32_image("wrapper"));
    vm.close_dir(root).unwrap();
    {
        let volume = volume.to_volume(&vm);
        let root = volume.open_root_dir().unwrap();
        root.make_long_name_dir_in_dir("Wrapped Directory").unwrap();
        let dir = root.open_long_name_dir("wrapped directory").unwrap();
        let file = dir
            .open_long_name_file_in_dir("Wrapped File.txt", Mode::ReadWriteCreate)
            .unwrap();
        file.write(b"wrapped").unwrap();
        file.close().unwrap();
        dir.rename_long_name_in_dir("Wrapped File.txt", "Renamed File.txt")
            .unwrap();
        assert_eq!(dir.find_long_name_entry("renamed file.txt").unwrap().size, 7);
        dir.delete_long_name_entry_in_dir("Renamed File.txt").unwrap();
        assert_err!(dir.find_long_name_entry("Renamed File.txt"), Error::NotFound);
        dir.close().unwrap();
        root.delete_long_name_entry_in_dir("Wrapped Directory").unwrap();
        root.close().unwrap();
        volume.close().unwrap();
    }
    let (disk, _) = vm.free();
    let image = disk.into_inner();
    let layout = layout(&image);
    let (lfn_entries, short_entries) = check_raw_dir(&raw_dir(&image, layout.root_cluster));
    assert_eq!((lfn_entries, short_entries), (0, 1));
    fsck(&image, "wrapper");
}

#[test]
fn fat16_long_names_in_fixed_root_and_subdirectory() {
    let disk = utils::make_block_device(utils::DISK_SOURCE).unwrap();
    let vm: Vm = VolumeManager::new_with_limits(disk, utils::make_time_source(), 0xAA00_0000);
    let volume = vm.open_raw_volume(VolumeIdx(0)).expect("open FAT16 volume");
    let root = vm.open_root_dir(volume).unwrap();

    create_file(&vm, root, "Long Name In The FAT16 Root.txt", b"root");
    create_file(&vm, root, "Another Long Name.txt", b"another");
    assert_eq!(
        read_file(&vm, root, "long name in the fat16 root.TXT"),
        b"root"
    );
    let listed = list(&vm, root);
    assert!(
        listed
            .iter()
            .any(|l| l.name == "Long Name In The FAT16 Root.txt")
    );
    assert!(listed.iter().any(|l| l.name == "README.TXT"));

    let test_dir = vm.open_long_name_dir_in_dir(root, "test").unwrap();
    create_file(&vm, test_dir, "Deep Long Name.txt", b"deep");
    vm.make_long_name_dir_in_dir(test_dir, "Nested Directory")
        .unwrap();
    let nested = vm
        .open_long_name_dir_in_dir(test_dir, "Nested Directory")
        .unwrap();
    create_file(&vm, nested, "Deeper Still.txt", b"deeper");
    assert_eq!(read_file(&vm, nested, "Deeper Still.txt"), b"deeper");
    vm.rename_long_name_in_dir(nested, "Deeper Still.txt", "Deepest.txt")
        .unwrap();
    vm.move_long_name(nested, "Deepest.txt", root, "Surfaced.txt")
        .unwrap();
    assert_eq!(read_file(&vm, root, "Surfaced.txt"), b"deeper");
    vm.close_dir(nested).unwrap();
    vm.delete_long_name_entry_in_dir(test_dir, "Nested Directory")
        .unwrap();
    vm.delete_long_name_entry_in_dir(test_dir, "Deep Long Name.txt")
        .unwrap();
    assert_err!(
        vm.find_long_name_entry_in_dir(test_dir, "Deep Long Name.txt"),
        Error::NotFound
    );
    assert_eq!(list(&vm, test_dir).len(), 1, "only TEST.DAT remains");
    vm.close_dir(test_dir).unwrap();

    vm.delete_long_name_entry_in_dir(root, "Another Long Name.txt")
        .unwrap();
    assert_err!(
        vm.find_long_name_entry_in_dir(root, "Another Long Name.txt"),
        Error::NotFound
    );
    assert_eq!(
        read_file(&vm, root, "Long Name In The FAT16 Root.txt"),
        b"root"
    );
    vm.close_dir(root).unwrap();
    vm.close_volume(volume).unwrap();
}
