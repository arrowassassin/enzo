//! Regenerates one pack into a temporary directory and checks what the device and
//! the firmware rely on: file sizes, the PBM header, an ink density the panel is
//! happy with, and a clock slot that is entirely paper or entirely ink.

use quire_sleep::output::{compress, pbm_bytes, write_pack, PackJson};
use quire_sleep::packs;
use quire_sleep::slot::Surface;
use std::fs;
use std::path::PathBuf;

const PBM_BYTES: u64 = 528 * 792 / 8 + "P4\n528 792\n".len() as u64;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quire-sleep-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Minimal P4 reader mirroring `quire_library::cache::read_pbm`: returns (w, h, bits).
fn read_pbm(bytes: &[u8]) -> (u32, u32, &[u8]) {
    assert_eq!(&bytes[..3], b"P4\n", "magic and newline");
    let header_end = 3 + bytes[3..].iter().position(|&b| b == b'\n').expect("dimension line") + 1;
    let dims = std::str::from_utf8(&bytes[3..header_end - 1]).unwrap();
    let mut it = dims.split(' ');
    let w: u32 = it.next().unwrap().parse().unwrap();
    let h: u32 = it.next().unwrap().parse().unwrap();
    assert!(it.next().is_none(), "exactly two numbers");
    let need = (w as usize).div_ceil(8) * h as usize;
    assert_eq!(bytes.len(), header_end + need, "bits follow the header exactly");
    (w, h, &bytes[header_end..])
}

fn bit(bits: &[u8], w: u32, x: i32, y: i32) -> bool {
    let stride = (w as usize).div_ceil(8);
    bits[y as usize * stride + (x as usize >> 3)] & (0x80 >> (x & 7)) != 0
}

#[test]
fn deco_pack_regenerates_cleanly() {
    let dir = temp_dir("deco");
    let pack = packs::find("deco").expect("deco pack");
    let entry = write_pack(&dir, &pack, "21:47").expect("write pack");
    assert_eq!(entry.images, pack.count);
    assert_eq!(entry.bytes, PBM_BYTES * pack.count as u64);
    assert!(entry.bytes_z < entry.bytes / 2, "compressed variant should be well under half: {}", entry.bytes_z);
    assert!(dir.join("preview").join("deco.png").exists(), "contact sheet");

    let json: PackJson = serde_json::from_slice(&fs::read(dir.join("deco").join("pack.json")).unwrap()).unwrap();
    assert_eq!(json.id, "deco");
    assert_eq!(json.licence, "CC0-1.0");
    assert_eq!(json.images.len(), pack.count);

    for img in &json.images {
        let pbm = fs::read(dir.join("deco").join(&img.file)).unwrap();
        assert_eq!(pbm.len() as u64, img.bytes);
        assert_eq!(pbm.len() as u64, PBM_BYTES);
        let (w, h, bits) = read_pbm(&pbm);
        assert_eq!((w, h), (528, 792));

        // The compressed variant is what the device fetches: zlib-framed, round-trips.
        let z = fs::read(dir.join("deco").join(&img.z)).unwrap();
        assert_eq!(z.len() as u64, img.bytes_z);
        assert_eq!(z[0] & 0x0f, 8, "zlib header: deflate method");
        let back = miniz_oxide::inflate::decompress_to_vec_zlib(&z).expect("inflate");
        assert_eq!(back, pbm, "{}: inflate round trip", img.file);

        // Ink density the panel is happy with.
        let set: u32 = bits.iter().map(|b| b.count_ones()).sum();
        let density = set as f32 / (w * h) as f32;
        assert!((0.05..=0.70).contains(&density), "{}: ink density {density}", img.file);
        assert!((density - img.ink).abs() < 0.002, "{}: sidecar density {} vs {density}", img.file, img.ink);

        // The clock slot is entirely its declared surface and inside the panel.
        let s = img.clock;
        assert!(s.x >= 0 && s.y >= 0 && s.x + s.w <= 528 && s.y + s.h <= 792, "{}: slot {s:?}", img.file);
        let want_ink = s.on == Surface::Ink;
        for y in s.y..s.y + s.h {
            for x in s.x..s.x + s.w {
                assert_eq!(bit(bits, w, x, y), want_ink, "{}: slot pixel {x},{y} is not clean", img.file);
            }
        }
        // And the slot is big enough for the widest time in its style.
        let (min_w, min_h) = s.style.slot_size();
        assert!(s.w >= min_w && s.h >= min_h, "{}: slot {}x{} smaller than {min_w}x{min_h}", img.file, s.w, s.h);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_pack_renders_with_a_clean_slot_and_sane_density() {
    // Cheaper than writing files: render once, check the invariants the device needs.
    for pack in packs::all() {
        for i in 0..pack.count {
            let mut art = (pack.render)(i);
            art.slot.clear(&mut art.canvas);
            let bm = art.canvas.finish();
            let density = quire_sleep::canvas::ink_density(&bm);
            assert!((0.05..=0.70).contains(&density), "{}/{i}: ink density {density}", pack.id);
            let s = art.slot;
            let want_ink = s.on == Surface::Ink;
            for y in s.y..s.y + s.h {
                for x in s.x..s.x + s.w {
                    assert_eq!(bm.get(x as u32, y as u32), want_ink, "{}/{i}: slot pixel {x},{y}", pack.id);
                }
            }
            let pbm = pbm_bytes(&bm);
            assert_eq!(pbm.len() as u64, PBM_BYTES);
            assert!(compress(&pbm).len() < pbm.len());
        }
    }
}

#[test]
fn rendering_is_reproducible() {
    let pack = packs::find("mountains").unwrap();
    let a = pbm_bytes(&(pack.render)(0).canvas.finish());
    let b = pbm_bytes(&(pack.render)(0).canvas.finish());
    assert_eq!(a, b);
}
