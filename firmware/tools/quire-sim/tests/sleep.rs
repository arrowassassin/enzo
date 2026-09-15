//! Sleep-image packs on the sleep screen: the pack image with its live clock, the minute
//! tick while asleep (a DU that repaints only the clock slot, nothing within the same
//! minute), the loose-image plate, the picker's Images options, and the heap the
//! compressed image costs to stream in.

use quire_sim::{fixture_card, frame_hash, Sim, FIXTURE_PACK, NOW};
use quire_ui::settings::{ImageRotation, SleepVariant};
use quire_ui::sleeppack::{self, ALL_PACKS};
use quire_ui::{Env, Event, Key, Refresh};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counts the heap this thread holds while `measure` runs (the other tests' threads are
/// invisible to it), so a load's peak allocation can be asserted.
struct Counting;

std::thread_local! {
    static LIVE: Cell<(bool, usize, usize)> = const { Cell::new((false, 0, 0)) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = LIVE.try_with(|c| {
            let (on, live, peak) = c.get();
            if on {
                let live = live + l.size();
                c.set((on, live, peak.max(live)));
            }
        });
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let _ = LIVE.try_with(|c| {
            let (on, live, peak) = c.get();
            if on {
                c.set((on, live.saturating_sub(l.size()), peak));
            }
        });
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Run `f`, returning its result and the peak heap it held beyond what was live before.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    LIVE.with(|c| c.set((true, 0, 0)));
    let out = f();
    let (_, _, peak) = LIVE.with(|c| c.replace((false, 0, 0)));
    (out, peak)
}

fn sleep_with_pack(sim: &mut Sim, image: &str) {
    sim.ui.settings.sleep = SleepVariant::Custom;
    sim.ui.settings.sleep_pack = Some(String::from(FIXTURE_PACK));
    sim.ui.settings.sleep_rotation = ImageRotation::Fixed;
    sim.ui.settings.sleep_image = Some(String::from(image));
    sim.press(Key::Power);
    assert_eq!(sim.top(), "40-sleep");
    assert!(sim.ui.asleep);
}

/// The slot of the fixture pack's image `file`, from its manifest on the card.
fn slot_of(sim: &Sim, file: &str) -> sleeppack::ClockSlot {
    let pack = sleeppack::load_pack(sim.env.fs(), "/sleep", FIXTURE_PACK).expect("pack installed");
    pack.images.iter().find(|i| i.file == file).and_then(|i| i.clock).expect("slot")
}

/// Ink pixels inside and outside a rectangle.
fn ink_in_out(f: &quire_gfx::Frame, r: quire_gfx::Rect) -> (usize, usize) {
    let (mut inside, mut outside) = (0, 0);
    for y in 0..f.height() as i32 {
        for x in 0..f.width() as i32 {
            if f.get(x, y) {
                if r.contains(x, y) {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
    }
    (inside, outside)
}

#[test]
fn pack_image_shows_the_time_and_ticks_once_a_minute() {
    let card = fixture_card("sleep-pack");
    let mut sim = Sim::boot(&card);
    sim.env.now = NOW; // 21:47
    sleep_with_pack(&mut sim, "02.pbm");
    assert_eq!(sim.last_refresh, Refresh::Gc);
    let slot = slot_of(&sim, "02.pbm");
    let first = sim.frame().clone();
    let (in_slot, outside) = ink_in_out(&first, slot.rect());
    assert!(outside > 20_000, "the image is there ({outside} px of art)");
    assert!(in_slot > 300, "the time is in the slot ({in_slot} px)");

    // The same minute: nothing to do.
    sim.env.now += 30;
    assert_eq!(sim.event(Event::Tick), Refresh::None);
    assert_eq!(frame_hash(sim.frame()), frame_hash(&first));

    // The next minute: a DU that changes only the slot.
    sim.env.now += 30;
    assert_eq!(sim.event(Event::Tick), Refresh::Du);
    let second = sim.frame().clone();
    assert_ne!(frame_hash(&second), frame_hash(&first), "21:48 differs from 21:47");
    for y in 0..first.height() as i32 {
        for x in 0..first.width() as i32 {
            if !slot.rect().contains(x, y) {
                assert_eq!(first.get(x, y), second.get(x, y), "pixel ({x}, {y}) outside the slot changed");
            }
        }
    }
    // Again within that minute: nothing; a minute later: another DU.
    assert_eq!(sim.event(Event::Tick), Refresh::None);
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::Du);
    assert_ne!(frame_hash(sim.frame()), frame_hash(&second));
    assert!(sim.ui.asleep && sim.top() == "40-sleep", "still asleep behind the same screen");

    // A battery change redraws the whole screen but keeps the same image.
    sim.event(Event::Battery(quire_ui::Battery { percent: 61, ..sim.env.battery }));
    let (in_slot2, outside2) = ink_in_out(sim.frame(), slot.rect());
    assert_eq!(outside2, outside, "same picture after a redraw");
    assert!(in_slot2 > 300);

    // Waking stops the ticks.
    sim.event(Event::Wake);
    assert_eq!(sim.top(), "20-reading");
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::None, "awake: the reading page's tick draws nothing");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn the_plain_image_and_rotations_load_too() {
    let card = fixture_card("sleep-pack-plain");
    let mut sim = Sim::boot(&card);
    sleep_with_pack(&mut sim, "01.pbm");
    let slot = slot_of(&sim, "01.pbm");
    let (in_slot, outside) = ink_in_out(sim.frame(), slot.rect());
    assert!(outside > 20_000 && in_slot > 300, "{outside} / {in_slot}");
    sim.event(Event::Wake);

    // Images 03–05 are not on the card: a fixed choice of one falls through to the
    // images that are (nothing is left blank).
    sim.ui.settings.sleep_image = Some(String::from("05.pbm"));
    sim.press(Key::Power);
    assert!(sim.frame().ink_count() > 20_000, "a missing image falls back to one on the card");
    sim.event(Event::Wake);

    // Every rotation, and every source, draws a picture with a clock.
    for pack in [Some(String::from(FIXTURE_PACK)), Some(String::from(ALL_PACKS))] {
        for rotation in [ImageRotation::Fixed, ImageRotation::Daily, ImageRotation::EachSleep] {
            sim.ui.settings.sleep_pack = pack.clone();
            sim.ui.settings.sleep_rotation = rotation;
            sim.ui.settings.sleep_image = None;
            sim.press(Key::Power);
            assert!(sim.frame().ink_count() > 20_000, "{pack:?} {rotation:?}");
            sim.event(Event::Wake);
        }
    }
    // The clock off: the slot stays clean.
    sim.ui.settings.sleep_pack = Some(String::from(FIXTURE_PACK));
    sim.ui.settings.sleep_rotation = ImageRotation::Fixed;
    sim.ui.settings.sleep_image = Some(String::from("01.pbm"));
    sim.ui.settings.sleep_clock = false;
    sim.press(Key::Power);
    let (in_slot, outside) = ink_in_out(sim.frame(), slot.rect());
    assert_eq!(in_slot, 0, "no clock: the slot is paper");
    assert!(outside > 20_000);
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::None, "no clock, nothing to repaint");
    sim.event(Event::Wake);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn loose_images_get_a_clock_plate() {
    let card = fixture_card("sleep-loose");
    // A loose image next to the packs folder: all ink, so the plate is the only paper.
    let mut pbm = b"P4\n528 792\n".to_vec();
    pbm.extend(std::iter::repeat_n(0xFFu8, 66 * 792));
    std::fs::write(card.join("sleep/night.pbm"), &pbm).unwrap();
    let mut sim = Sim::boot(&card);
    sim.ui.settings.sleep = SleepVariant::Custom;
    sim.ui.settings.sleep_pack = None;
    sim.press(Key::Power);
    let plate = quire_ui::screens::sleep::plate_clock(528, true);
    let r = plate.slot.rect();
    assert!(plate.plate && r.y == 24 && (r.x - (528 - r.right())).abs() <= 1, "centred near the top: {r:?}");
    let f = sim.frame();
    let paper = (0..f.height() as i32).flat_map(|y| (0..f.width() as i32).map(move |x| (x, y))).filter(|&(x, y)| !f.get(x, y)).count();
    let (in_plate, _) = ink_in_out(f, r);
    assert!(paper > 0 && paper < (r.w * r.h) as usize, "the plate is the only paper: {paper} px");
    assert!(in_plate > 100 && in_plate < (r.w * r.h / 2) as usize, "the time and its rule: {in_plate} px");
    // The rule: the plate's edge rows are ink.
    for x in r.x..r.right() {
        assert!(f.get(x, r.y) && f.get(x, r.bottom() - 1));
    }
    let before = frame_hash(f);
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::Du);
    assert_ne!(frame_hash(sim.frame()), before);
    sim.event(Event::Wake);

    // Nothing on the card at all: the empty state, and ticks do nothing.
    std::fs::remove_file(card.join("sleep/night.pbm")).unwrap();
    std::fs::remove_dir_all(card.join("sleep/packs")).unwrap();
    sim.press(Key::Power);
    assert!(sim.frame().ink_count() > 200, "the empty state has text");
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::None);
    sim.event(Event::Wake);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn the_poster_clock_ticks_when_no_book_is_open() {
    let card = fixture_card("sleep-poster-tick");
    let mut sim = Sim::boot(&card);
    sim.ui.close_book(&mut sim.env);
    sim.reset();
    sim.ui.settings.sleep = SleepVariant::Poster;
    sim.press(Key::Power);
    let before = frame_hash(sim.frame());
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::Du);
    assert_ne!(frame_hash(sim.frame()), before, "the poster shows the new minute");
    sim.event(Event::Wake);
    // A blank sleep screen never repaints.
    sim.ui.settings.sleep = SleepVariant::Blank;
    sim.press(Key::Power);
    sim.env.now += 60;
    assert_eq!(sim.event(Event::Tick), Refresh::None);
    sim.event(Event::Wake);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn picker_offers_the_images_options() {
    let card = fixture_card("sleep-picker");
    let mut sim = Sim::boot(&card);
    sim.ui.settings.sleep_pack = None;
    sim.open("44-picker");
    sim.press(Key::Down); // focus the Images thumbnail (row 2, column 1)
    sim.press(Key::Down); // into its options: Source
    assert_eq!(sim.top(), "44-picker");
    sim.press(Key::Confirm);
    assert_eq!(sim.ui.settings.sleep_pack.as_deref(), Some(FIXTURE_PACK), "Source cycles to the installed pack");
    sim.press(Key::Confirm);
    assert_eq!(sim.ui.settings.sleep_pack.as_deref(), Some(ALL_PACKS), "then to all packs");
    sim.press(Key::Confirm);
    assert_eq!(sim.ui.settings.sleep_pack, None, "then back to loose images");
    sim.press(Key::Down); // Rotation
    let r0 = sim.ui.settings.sleep_rotation;
    sim.press(Key::Confirm);
    assert_ne!(sim.ui.settings.sleep_rotation, r0);
    sim.press(Key::Down); // Clock
    assert!(sim.ui.settings.sleep_clock);
    sim.press(Key::Confirm);
    assert!(!sim.ui.settings.sleep_clock, "Clock toggles");
    sim.press(Key::Down);
    assert!(!sim.ui.settings.sleep_clock, "no fourth option");
    sim.press(Key::Confirm);
    assert!(sim.ui.settings.sleep_clock);
    // Up out of the options and Confirm: Images becomes the sleep screen.
    for _ in 0..3 {
        sim.press(Key::Up);
    }
    sim.press(Key::Confirm);
    assert_eq!(sim.ui.settings.sleep, SleepVariant::Custom);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn a_compressed_image_streams_in_under_the_heap_budget() {
    let card = fixture_card("sleep-heap");
    let sim = Sim::boot(&card);
    let fs = sim.env.fs();
    let pack = sleeppack::load_pack(fs, "/sleep", FIXTURE_PACK).unwrap();
    let z = pack.images.iter().position(|i| i.file == "02.pbm").unwrap();
    let plain = pack.images.iter().position(|i| i.file == "01.pbm").unwrap();
    let mut frame = quire_gfx::Frame::panel();
    let (ok, peak) = measure(|| sleeppack::draw_image_into(fs, &pack, z, &mut frame));
    assert!(ok);
    eprintln!("02.pbm.z streamed in with a peak heap of {peak} bytes");
    assert!(peak < 80 * 1024, "peak heap {peak} bytes");
    // The same image, plain, from the repository: the inflated rows match it exactly.
    let want = std::fs::read(quire_sim::sleep_packs().join(FIXTURE_PACK).join("02.pbm")).unwrap();
    let hdr = want.len() - 66 * 792;
    assert_eq!(frame.bits(), &want[hdr..], "inflated rows match the plain PBM");
    let (ok, peak_plain) = measure(|| sleeppack::draw_image_into(fs, &pack, plain, &mut frame));
    assert!(ok);
    eprintln!("01.pbm streamed in with a peak heap of {peak_plain} bytes");
    assert!(peak_plain < 4 * 1024, "plain PBM peak heap {peak_plain} bytes");
    // The whole sleep draw, clock included, stays well inside the budget too.
    let mut sim = sim;
    let (_, peak_draw) = measure(|| {
        sleep_with_pack(&mut sim, "02.pbm");
    });
    eprintln!("Power -> pack sleep screen: peak heap {peak_draw} bytes");
    assert!(peak_draw < 80 * 1024, "sleep draw peak heap {peak_draw} bytes");
    sim.event(Event::Wake);
    let _ = std::fs::remove_dir_all(&card);
}
