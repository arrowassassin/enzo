//! Long File Name (VFAT) support.
//!
//! This module holds the pure, allocation-free parts of long file name
//! handling: validating a name, generating the 8.3 *basis name* and numeric
//! tails, splitting a name into the 13-character UCS-2 chunks that go into
//! LFN directory entries, and case-insensitive comparison. The on-disk work is
//! done by [`crate::fat::FatVolume`].
//!
//! Everything follows the Microsoft *FAT: General Overview of On-Disk Format*
//! specification (the "Long File Name" and "Generating Short Names" sections).

use super::{FilenameError, ShortFileName};

/// Maximum number of UTF-16 code units in a long file name.
pub const MAX_LFN_UNITS: usize = 255;

/// Number of UTF-16 code units stored in one LFN directory entry.
pub const LFN_UNITS_PER_ENTRY: usize = 13;

/// Maximum number of LFN directory entries one name can need (`ceil(255/13)`).
pub const MAX_LFN_ENTRIES: usize = MAX_LFN_UNITS.div_ceil(LFN_UNITS_PER_ENTRY);

/// Flag in the sequence byte of the first (highest numbered) LFN entry.
pub(crate) const LFN_LAST_ENTRY_FLAG: u8 = 0x40;

/// Largest numeric tail we try before falling back to a hashed tail.
const MAX_SEQUENTIAL_TAIL: u32 = 1023;

/// A validated long file name.
///
/// Trailing spaces and periods are stripped (as Windows does). The name is
/// guaranteed to be non-empty, free of characters that are illegal in a long
/// file name and at most 255 UTF-16 code units long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LongName<'a> {
    name: &'a str,
    units: u16,
}

impl<'a> LongName<'a> {
    /// Validate a long file name.
    pub fn new(name: &'a str) -> Result<Self, FilenameError> {
        let name = name.trim_end_matches([' ', '.']);
        if name.is_empty() {
            return Err(FilenameError::FilenameEmpty);
        }
        let mut units = 0usize;
        for ch in name.chars() {
            if is_invalid_lfn_char(ch) {
                return Err(FilenameError::InvalidCharacter);
            }
            units += ch.len_utf16();
        }
        if units > MAX_LFN_UNITS {
            return Err(FilenameError::NameTooLong);
        }
        Ok(LongName {
            name,
            units: units as u16,
        })
    }

    /// The (trimmed) name.
    pub fn as_str(&self) -> &'a str {
        self.name
    }

    /// Length of the name in UTF-16 code units.
    pub fn utf16_len(&self) -> usize {
        usize::from(self.units)
    }

    /// How many LFN directory entries are needed to store this name.
    pub fn num_entries(&self) -> usize {
        self.utf16_len().div_ceil(LFN_UNITS_PER_ENTRY)
    }

    /// If this name is already a valid 8.3 name (ignoring case), return the
    /// short name it maps to.
    ///
    /// Only ASCII names qualify: non-ASCII characters always go through the
    /// lossy basis-name conversion and get a long name entry.
    pub fn short_form(&self) -> Option<ShortFileName> {
        let (base, ext) = match self.name.rfind('.') {
            Some(idx) => (&self.name[..idx], &self.name[idx + 1..]),
            None => (self.name, ""),
        };
        if base.is_empty() || base.len() > ShortFileName::BASE_LEN || ext.len() > 3 {
            return None;
        }
        if !base.bytes().all(is_short_name_char) || !ext.bytes().all(is_short_name_char) {
            return None;
        }
        ShortFileName::create_from_str(self.name).ok()
    }

    /// Does this name need LFN directory entries to be stored losslessly?
    ///
    /// Names which are already valid 8.3 names (compared case-insensitively)
    /// and whose base and extension are each uniformly cased do not: they are
    /// stored as an upper-case short name only. An 8.3-shaped name with mixed
    /// case (`MyBook.txt`) gets LFN entries so that the case is preserved.
    pub fn needs_lfn_entries(&self) -> bool {
        if self.short_form().is_none() {
            return true;
        }
        let (base, ext) = match self.name.rfind('.') {
            Some(idx) => (&self.name[..idx], &self.name[idx + 1..]),
            None => (self.name, ""),
        };
        is_mixed_case(base) || is_mixed_case(ext)
    }

    /// The 13 UCS-2 characters of LFN entry number `seq` (1-based).
    ///
    /// The name is terminated with a `0x0000` if it ends inside the chunk and
    /// the rest of the chunk is padded with `0xFFFF`.
    pub fn chunk(&self, seq: usize) -> [u16; LFN_UNITS_PER_ENTRY] {
        debug_assert!((1..=self.num_entries()).contains(&seq));
        let mut out = [0xFFFFu16; LFN_UNITS_PER_ENTRY];
        let start = (seq - 1) * LFN_UNITS_PER_ENTRY;
        let len = self.utf16_len();
        let mut units = self.name.encode_utf16().skip(start);
        for (i, slot) in out.iter_mut().enumerate() {
            let idx = start + i;
            if idx < len {
                *slot = units.next().unwrap_or(0xFFFF);
            } else if idx == len {
                *slot = 0x0000;
            } else {
                break;
            }
        }
        out
    }

    /// Build the on-disk bytes of LFN entry number `seq` (1-based) for a file
    /// whose short name has the checksum `csum`.
    pub(crate) fn entry_bytes(&self, seq: usize, csum: u8) -> [u8; 32] {
        let chunk = self.chunk(seq);
        let mut data = [0u8; 32];
        data[0] = seq as u8;
        if seq == self.num_entries() {
            data[0] |= LFN_LAST_ENTRY_FLAG;
        }
        for (i, unit) in chunk[0..5].iter().enumerate() {
            data[1 + i * 2..3 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        data[11] = super::Attributes::LFN;
        data[12] = 0;
        data[13] = csum;
        for (i, unit) in chunk[5..11].iter().enumerate() {
            data[14 + i * 2..16 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        // 26..28: first cluster, must be zero for LFN entries
        for (i, unit) in chunk[11..13].iter().enumerate() {
            data[28 + i * 2..30 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        data
    }

    /// Does the on-disk LFN chunk with sequence number `seq` match the
    /// corresponding part of this name (case-insensitively)?
    pub(crate) fn chunk_matches(&self, seq: usize, chunk: &[u16; LFN_UNITS_PER_ENTRY]) -> bool {
        if seq == 0 || seq > self.num_entries() {
            return false;
        }
        let start = (seq - 1) * LFN_UNITS_PER_ENTRY;
        let len = self.utf16_len();
        let mut units = self.name.encode_utf16().skip(start);
        for (i, on_disk) in chunk.iter().enumerate() {
            let idx = start + i;
            if idx < len {
                let Some(want) = units.next() else {
                    return false;
                };
                if fold_unit(want) != fold_unit(*on_disk) {
                    return false;
                }
            } else if idx == len {
                // the name must be terminated here
                if *on_disk != 0x0000 {
                    return false;
                }
            } else {
                // padding: be tolerant about what is in there
                break;
            }
        }
        true
    }

    /// Compare this name to another string, ignoring case.
    pub fn eq_ignore_case(&self, other: &str) -> bool {
        eq_ignore_case(self.name, other)
    }

    /// Compute the 8.3 *basis name* for this long name, following the
    /// Microsoft specification.
    pub fn basis(&self) -> ShortNameBasis {
        let mut basis = ShortNameBasis {
            base: [b' '; ShortFileName::BASE_LEN],
            base_len: 0,
            ext: [b' '; 3],
            ext_len: 0,
            tail_required: self.short_form().is_none(),
        };
        // 1. Strip leading spaces and periods.
        let stripped = self.name.trim_start_matches([' ', '.']);
        // 2. Split at the last embedded period.
        let (base_part, ext_part) = match stripped.rfind('.') {
            Some(idx) => (&stripped[..idx], &stripped[idx + 1..]),
            None => (stripped, ""),
        };
        // 3. Copy the primary name: upper-cased, spaces removed, invalid
        //    characters replaced by '_', stopping at the first period.
        for ch in base_part.chars() {
            if ch == '.' {
                basis.tail_required = true;
                break;
            }
            if ch == ' ' {
                continue;
            }
            let (b, lossy) = to_short_char(ch);
            basis.tail_required |= lossy;
            if usize::from(basis.base_len) < ShortFileName::BASE_LEN {
                basis.base[usize::from(basis.base_len)] = b;
                basis.base_len += 1;
            } else {
                basis.tail_required = true;
            }
        }
        // 4. Copy up to three characters of extension.
        for ch in ext_part.chars() {
            if ch == ' ' {
                continue;
            }
            let (b, lossy) = to_short_char(ch);
            basis.tail_required |= lossy;
            if usize::from(basis.ext_len) < 3 {
                basis.ext[usize::from(basis.ext_len)] = b;
                basis.ext_len += 1;
            } else {
                basis.tail_required = true;
            }
        }
        if basis.base_len == 0 {
            // e.g. the name was only made of spaces and periods before the
            // extension; make something up.
            basis.base[0] = b'_';
            basis.base_len = 1;
            basis.tail_required = true;
        }
        basis
    }

    /// A 16-bit hash of the name, used for hashed numeric tails when more than
    /// [`MAX_SEQUENTIAL_TAIL`] files share a basis name.
    fn hash16(&self, salt: u32) -> u16 {
        let mut h: u32 = 0x811C_9DC5 ^ salt;
        for unit in self.name.encode_utf16() {
            h ^= u32::from(unit);
            h = h.wrapping_mul(0x0100_0193);
        }
        ((h >> 16) ^ (h & 0xFFFF)) as u16
    }
}

/// An 8.3 basis name generated from a long name, before any numeric tail is
/// added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortNameBasis {
    base: [u8; ShortFileName::BASE_LEN],
    base_len: u8,
    ext: [u8; 3],
    ext_len: u8,
    /// Must a numeric tail always be appended (because the conversion was
    /// lossy or the name does not fit 8.3)?
    tail_required: bool,
}

impl ShortNameBasis {
    /// Is a numeric tail mandatory for this basis name?
    pub fn tail_required(&self) -> bool {
        self.tail_required
    }

    /// The basis name without a numeric tail.
    pub fn plain(&self) -> ShortFileName {
        let mut contents = [b' '; ShortFileName::TOTAL_LEN];
        contents[..ShortFileName::BASE_LEN].copy_from_slice(&self.base);
        contents[ShortFileName::BASE_LEN..].copy_from_slice(&self.ext);
        ShortFileName { contents }
    }

    /// The part of the base name kept in front of the numeric tail `~n`.
    pub(crate) fn prefix_for_tail(&self, n: u32) -> &[u8] {
        let keep = ShortFileName::BASE_LEN - 1 - decimal_digits(n);
        &self.base[..usize::from(self.base_len).min(keep)]
    }

    /// The basis name with the numeric tail `~n` appended.
    pub fn with_tail(&self, n: u32) -> ShortFileName {
        let mut sfn = self.plain();
        let prefix = self.prefix_for_tail(n);
        let mut idx = prefix.len();
        sfn.contents[idx..ShortFileName::BASE_LEN].fill(b' ');
        sfn.contents[idx] = b'~';
        idx += 1;
        let digits = decimal_digits(n);
        let mut value = n;
        for i in (0..digits).rev() {
            sfn.contents[idx + i] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        sfn
    }

    /// A short name with a hashed tail (`XX1234~n`), used once all the
    /// sequential tails are exhausted.
    pub(crate) fn with_hashed_tail(&self, name: &LongName<'_>, attempt: u32) -> ShortFileName {
        let mut sfn = self.plain();
        let keep = usize::from(self.base_len).min(2);
        sfn.contents[keep..ShortFileName::BASE_LEN].fill(b' ');
        let hash = name.hash16(attempt);
        let mut idx = keep;
        for shift in [12u32, 8, 4, 0] {
            let nibble = ((hash >> shift) & 0xF) as u8;
            sfn.contents[idx] = if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            };
            idx += 1;
        }
        sfn.contents[idx] = b'~';
        sfn.contents[idx + 1] = b'1' + (attempt % 9) as u8;
        sfn
    }

    /// Decode the numeric tail of an existing short name, if it is derived
    /// from this basis: returns `n` when `existing` is `prefix_for_tail(n) ~n`
    /// with the same extension.
    pub(crate) fn tail_of(&self, existing: &ShortFileName) -> Option<u32> {
        if existing.contents[ShortFileName::BASE_LEN..] != self.ext[..] {
            return None;
        }
        let base = &existing.contents[..ShortFileName::BASE_LEN];
        let tilde = base.iter().position(|&b| b == b'~')?;
        let digits = &base[tilde + 1..];
        let digit_count = digits.iter().take_while(|b| b.is_ascii_digit()).count();
        if digit_count == 0 || digits[digit_count..].iter().any(|&b| b != b' ') {
            return None;
        }
        if digits[0] == b'0' {
            return None;
        }
        let mut n: u32 = 0;
        for &d in &digits[..digit_count] {
            n = n.checked_mul(10)?.checked_add(u32::from(d - b'0'))?;
        }
        if n == 0 || n > MAX_SEQUENTIAL_TAIL {
            return None;
        }
        if &base[..tilde] == self.prefix_for_tail(n) {
            Some(n)
        } else {
            None
        }
    }

    /// Does an existing short name equal the plain basis name?
    pub(crate) fn is_plain(&self, existing: &ShortFileName) -> bool {
        existing.contents[..ShortFileName::BASE_LEN] == self.base
            && existing.contents[ShortFileName::BASE_LEN..] == self.ext
    }
}

/// Largest sequential numeric tail: see [`MAX_SEQUENTIAL_TAIL`].
pub(crate) const fn max_sequential_tail() -> u32 {
    MAX_SEQUENTIAL_TAIL
}

fn decimal_digits(mut n: u32) -> usize {
    let mut digits = 1;
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits
}

/// Characters that are not allowed in a long file name.
fn is_invalid_lfn_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{001F}' | '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|'
    )
}

/// Characters allowed (as-is) in an 8.3 short name. Only ASCII qualifies here;
/// lower-case letters are accepted because they fold to upper case.
fn is_short_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"$%'-_@~`!(){}^#&".contains(&b)
}

/// Convert a long-name character to a short-name byte, returning whether the
/// conversion was lossy.
fn to_short_char(ch: char) -> (u8, bool) {
    if ch.is_ascii() {
        let b = ch.to_ascii_uppercase() as u8;
        if is_short_name_char(b) {
            (b, false)
        } else {
            (b'_', true)
        }
    } else {
        (b'_', true)
    }
}

fn is_mixed_case(s: &str) -> bool {
    let has_upper = s.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = s.bytes().any(|b| b.is_ascii_lowercase());
    has_upper && has_lower
}

/// Fold a UTF-16 code unit for case-insensitive comparison.
fn fold_unit(u: u16) -> u16 {
    match char::from_u32(u32::from(u)) {
        Some(c) => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) if (l as u32) <= 0xFFFF => l as u16,
                _ => u,
            }
        }
        None => u,
    }
}

/// Compare two strings ignoring case (Unicode simple lower-case mapping).
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

// ****************************************************************************
//
// Unit Tests
//
// ****************************************************************************

#[cfg(test)]
mod test {
    use super::*;

    fn sfn(s: &str) -> ShortFileName {
        ShortFileName::create_from_str(s).unwrap()
    }

    #[test]
    fn validation() {
        assert_eq!(LongName::new("").unwrap_err(), FilenameError::FilenameEmpty);
        assert_eq!(LongName::new("...").unwrap_err(), FilenameError::FilenameEmpty);
        assert_eq!(LongName::new("  . ").unwrap_err(), FilenameError::FilenameEmpty);
        assert_eq!(
            LongName::new("a:b").unwrap_err(),
            FilenameError::InvalidCharacter
        );
        assert_eq!(
            LongName::new("a\u{7}b").unwrap_err(),
            FilenameError::InvalidCharacter
        );
        assert_eq!(LongName::new("hello. ").unwrap().as_str(), "hello");
        let long: String = core::iter::repeat_n('x', 255).collect();
        assert!(LongName::new(&long).is_ok());
        let too_long: String = core::iter::repeat_n('x', 256).collect();
        assert_eq!(
            LongName::new(&too_long).unwrap_err(),
            FilenameError::NameTooLong
        );
        // An emoji is two UTF-16 units
        let emoji: String = core::iter::repeat_n('😀', 128).collect();
        assert_eq!(
            LongName::new(&emoji).unwrap_err(),
            FilenameError::NameTooLong
        );
    }

    #[test]
    fn short_form_and_lfn_need() {
        let n = LongName::new("README.TXT").unwrap();
        assert_eq!(n.short_form(), Some(sfn("README.TXT")));
        assert!(!n.needs_lfn_entries());

        let n = LongName::new("readme.txt").unwrap();
        assert_eq!(n.short_form(), Some(sfn("README.TXT")));
        assert!(!n.needs_lfn_entries());

        let n = LongName::new("MyBook.txt").unwrap();
        assert_eq!(n.short_form(), Some(sfn("MYBOOK.TXT")));
        assert!(n.needs_lfn_entries());

        let n = LongName::new("readme.Txt").unwrap();
        assert!(n.needs_lfn_entries());

        let n = LongName::new("My Book.epub").unwrap();
        assert_eq!(n.short_form(), None);
        assert!(n.needs_lfn_entries());

        let n = LongName::new("a+b.txt").unwrap();
        assert_eq!(n.short_form(), None);

        let n = LongName::new("café.txt").unwrap();
        assert_eq!(n.short_form(), None);

        let n = LongName::new("NOEXT").unwrap();
        assert_eq!(n.short_form(), Some(sfn("NOEXT")));
        assert!(!n.needs_lfn_entries());

        let n = LongName::new(".hidden").unwrap();
        assert_eq!(n.short_form(), None);
    }

    #[test]
    fn basis_names() {
        let b = LongName::new("My Book.epub").unwrap().basis();
        assert_eq!(b.plain(), sfn("MYBOOK.EPU"));
        assert!(b.tail_required());
        assert_eq!(b.with_tail(1), sfn("MYBOOK~1.EPU"));
        assert_eq!(b.with_tail(10), sfn("MYBOO~10.EPU"));
        assert_eq!(b.with_tail(123), sfn("MYBO~123.EPU"));

        let b = LongName::new("My Book (1).epub").unwrap().basis();
        assert_eq!(b.plain(), sfn("MYBOOK(1.EPU"));
        assert_eq!(b.with_tail(2), sfn("MYBOOK~2.EPU"));

        let b = LongName::new("MyBook.txt").unwrap().basis();
        assert_eq!(b.plain(), sfn("MYBOOK.TXT"));
        assert!(!b.tail_required());

        let b = LongName::new("a.b.c").unwrap().basis();
        assert_eq!(b.plain(), sfn("A.C"));
        assert!(b.tail_required());

        let b = LongName::new(".hidden").unwrap().basis();
        assert_eq!(b.plain(), sfn("HIDDEN"));
        assert!(b.tail_required());

        let b = LongName::new("a+b=c.txt").unwrap().basis();
        assert_eq!(b.plain(), sfn("A_B_C.TXT"));
        assert!(b.tail_required());

        let b = LongName::new("ünïcödé.md").unwrap().basis();
        assert_eq!(b.plain(), sfn("_N_C_D_.MD"));
        assert!(b.tail_required());

        let b = LongName::new("x").unwrap().basis();
        assert_eq!(b.with_tail(1), sfn("X~1"));
        assert_eq!(b.with_tail(999999), sfn("X~999999"));
    }

    #[test]
    fn tail_decoding() {
        let b = LongName::new("My Book.epub").unwrap().basis();
        assert_eq!(b.tail_of(&sfn("MYBOOK~1.EPU")), Some(1));
        assert_eq!(b.tail_of(&sfn("MYBOO~10.EPU")), Some(10));
        assert_eq!(b.tail_of(&sfn("MYBO~123.EPU")), Some(123));
        assert_eq!(b.tail_of(&sfn("MYBOOK~1.TXT")), None);
        assert_eq!(b.tail_of(&sfn("MYBOOK~0.EPU")), None);
        assert_eq!(b.tail_of(&sfn("MYBOO~01.EPU")), None);
        assert_eq!(b.tail_of(&sfn("OTHER~1.EPU")), None);
        assert_eq!(b.tail_of(&sfn("MYBOOK.EPU")), None);
        assert!(b.is_plain(&sfn("MYBOOK.EPU")));
        assert!(!b.is_plain(&sfn("MYBOOK~1.EPU")));
    }

    #[test]
    fn chunks() {
        let n = LongName::new("overlays").unwrap();
        assert_eq!(n.num_entries(), 1);
        assert_eq!(
            n.chunk(1),
            [
                'o' as u16, 'v' as u16, 'e' as u16, 'r' as u16, 'l' as u16, 'a' as u16,
                'y' as u16, 's' as u16, 0x0000, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
            ]
        );
        let n = LongName::new("bcm2708-rpi-b-plus.dtb").unwrap();
        assert_eq!(n.num_entries(), 2);
        assert_eq!(
            n.chunk(2),
            [
                '-' as u16, 'p' as u16, 'l' as u16, 'u' as u16, 's' as u16, '.' as u16,
                'd' as u16, 't' as u16, 'b' as u16, 0x0000, 0xFFFF, 0xFFFF, 0xFFFF,
            ]
        );
        assert_eq!(
            n.chunk(1),
            [
                'b' as u16, 'c' as u16, 'm' as u16, '2' as u16, '7' as u16, '0' as u16,
                '8' as u16, '-' as u16, 'r' as u16, 'p' as u16, 'i' as u16, '-' as u16,
                'b' as u16,
            ]
        );
        // Exactly 13 characters: no terminator fits, no padding.
        let n = LongName::new("COPYING.linux").unwrap();
        assert_eq!(n.num_entries(), 1);
        assert_eq!(n.chunk(1)[12], 'x' as u16);
    }

    #[test]
    fn entry_bytes_match_real_disk() {
        // Taken from the directory listing in `fat::test::test_dir_entries`
        let n = LongName::new("overlays").unwrap();
        let csum = sfn("OVERLAYS").csum();
        assert_eq!(csum, 0x47);
        let bytes = n.entry_bytes(1, csum);
        let expected = hex_literal::hex!(
            "416f007600650072006c000f00476100790073000000ffffffff0000ffffffff"
        );
        assert_eq!(bytes, expected);

        let n = LongName::new("bcm2708-rpi-b-plus.dtb").unwrap();
        let csum = sfn("BCM270~1.DTB").csum();
        assert_eq!(csum, 0x79);
        assert_eq!(
            n.entry_bytes(2, csum),
            hex_literal::hex!("422d0070006c00750073000f00792e006400740062000000ffff0000ffffffff")
        );
        assert_eq!(
            n.entry_bytes(1, csum),
            hex_literal::hex!("01620063006d00320037000f0079300038002d0072007000690000002d006200")
        );
    }

    #[test]
    fn chunk_matching() {
        let n = LongName::new("Bcm2708-RPI-b-plus.DTB").unwrap();
        let disk = LongName::new("bcm2708-rpi-b-plus.dtb").unwrap();
        assert!(n.chunk_matches(2, &disk.chunk(2)));
        assert!(n.chunk_matches(1, &disk.chunk(1)));
        assert!(!n.chunk_matches(3, &disk.chunk(1)));
        let other = LongName::new("bcm2708-rpi-b-plux.dtb").unwrap();
        assert!(!n.chunk_matches(2, &other.chunk(2)));
        // A shorter name whose prefix matches is not a match: the terminator
        // is in the wrong place.
        let shorter = LongName::new("bcm2708-rpi-b-plus.dt").unwrap();
        assert!(!n.chunk_matches(2, &shorter.chunk(2)));
    }

    #[test]
    fn case_insensitive_compare() {
        assert!(eq_ignore_case("Hello.TXT", "hello.txt"));
        assert!(eq_ignore_case("ÉCOLE", "école"));
        assert!(!eq_ignore_case("Hello", "Hello!"));
        assert!(!eq_ignore_case("Hello!", "Hello"));
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
