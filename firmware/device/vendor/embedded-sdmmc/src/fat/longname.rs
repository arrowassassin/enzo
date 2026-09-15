//! Long File Name (VFAT) operations on a FAT volume.
//!
//! Everything here works directly on directory entries: finding entries by
//! long name (case-insensitively), finding runs of free entries (growing the
//! directory by a cluster when needed), writing the LFN entries plus the short
//! entry, and deleting an entry together with its LFN entries.
//!
//! Directories are walked with a [`DirPos`] cursor, which is a `Copy` value so
//! a position can be remembered and returned to.

use core::ops::ControlFlow;

use crate::{
    Attributes, Block, BlockCache, BlockCount, BlockDevice, BlockIdx, ClusterId, DirEntry, DirectoryInfo, Error, ShortFileName, Timestamp,
    debug,
    fat::{FatSpecificInfo, FatVolume, OnDiskDirEntry, RESERVED_ENTRIES},
    filesystem::{LongName, longname},
    trace,
};

/// Number of 32-byte directory entries in one block.
const ENTRIES_PER_BLOCK: u32 = Block::LEN_U32 / OnDiskDirEntry::LEN_U32;

/// Marker in the first byte of a deleted directory entry.
const DELETED_ENTRY: u8 = 0xE5;

/// Marker in the first byte of the entry that ends a directory.
const END_ENTRY: u8 = 0x00;

/// How many hashed short names we try when the sequential tails are all in use.
const HASHED_TAIL_ATTEMPTS: u32 = 32;

/// Position of one 32-byte entry within a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirPos {
    /// Cluster holding the entry. [`ClusterId::ROOT_DIR`] denotes the reserved
    /// root directory region of a FAT16 volume; on FAT32 the root directory's
    /// real cluster number is used.
    cluster: ClusterId,
    /// Block within the cluster (or within the FAT16 root region).
    block_in_cluster: u32,
    /// Entry within the block.
    entry_in_block: u32,
}

impl DirPos {
    fn byte_offset(&self) -> usize {
        (self.entry_in_block * OnDiskDirEntry::LEN_U32) as usize
    }
}

impl FatVolume {
    /// The cluster a directory walk starts in.
    fn dir_start_cluster(&self, dir_cluster: ClusterId) -> ClusterId {
        match (&self.fat_specific_info, dir_cluster) {
            (FatSpecificInfo::Fat32(info), ClusterId::ROOT_DIR) => info.first_root_dir_cluster,
            _ => dir_cluster,
        }
    }

    /// The first position of a directory.
    fn dir_first_pos(&self, dir_cluster: ClusterId) -> DirPos {
        DirPos { cluster: self.dir_start_cluster(dir_cluster), block_in_cluster: 0, entry_in_block: 0 }
    }

    /// How many blocks the cluster (or root region) at `pos` has.
    fn dir_blocks_in(&self, cluster: ClusterId) -> u32 {
        match (&self.fat_specific_info, cluster) {
            (FatSpecificInfo::Fat16(info), ClusterId::ROOT_DIR) => {
                BlockCount::from_bytes(u32::from(info.root_entries_count) * OnDiskDirEntry::LEN_U32).0
            }
            _ => u32::from(self.blocks_per_cluster),
        }
    }

    /// The absolute block holding the entry at `pos`.
    fn dir_pos_block(&self, pos: DirPos) -> BlockIdx {
        let first = match (&self.fat_specific_info, pos.cluster) {
            (FatSpecificInfo::Fat16(info), ClusterId::ROOT_DIR) => self.lba_start + info.first_root_dir_block,
            _ => self.cluster_to_block(pos.cluster),
        };
        first + BlockCount(pos.block_in_cluster)
    }

    /// Move to the next entry, following the cluster chain. Returns `None`
    /// when the directory has no more allocated space.
    fn dir_next_pos<D>(&self, block_cache: &mut BlockCache<D>, pos: DirPos) -> Result<Option<DirPos>, Error<D::Error>>
    where
        D: BlockDevice,
    {
        if pos.entry_in_block + 1 < ENTRIES_PER_BLOCK {
            return Ok(Some(DirPos { entry_in_block: pos.entry_in_block + 1, ..pos }));
        }
        if pos.block_in_cluster + 1 < self.dir_blocks_in(pos.cluster) {
            return Ok(Some(DirPos { block_in_cluster: pos.block_in_cluster + 1, entry_in_block: 0, ..pos }));
        }
        if pos.cluster == ClusterId::ROOT_DIR {
            // Fixed size FAT16 root directory
            return Ok(None);
        }
        match self.next_cluster(block_cache, pos.cluster) {
            Ok(next) if next.0 < RESERVED_ENTRIES => Err(Error::UnterminatedFatChain),
            Ok(next) => Ok(Some(DirPos { cluster: next, block_in_cluster: 0, entry_in_block: 0 })),
            Err(Error::EndOfFile) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Add a zeroed cluster to the end of a directory whose last cluster is
    /// `last`, returning the position of its first entry.
    fn dir_grow<D>(&mut self, block_cache: &mut BlockCache<D>, last: ClusterId) -> Result<DirPos, Error<D::Error>>
    where
        D: BlockDevice,
    {
        if last == ClusterId::ROOT_DIR {
            // The FAT16 root directory cannot grow.
            return Err(Error::NotEnoughSpace);
        }
        let cluster = self.alloc_cluster(block_cache, Some(last), true)?;
        debug!("Directory grown with cluster {:?}", cluster);
        Ok(DirPos { cluster, block_in_cluster: 0, entry_in_block: 0 })
    }

    /// Write 32 bytes at `pos`. The block is only written back to disk when
    /// this entry is the last in its block; the caller must call
    /// [`BlockCache::write_back`] after the final entry it writes.
    fn dir_put_entry<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        pos: DirPos,
        bytes: &[u8; OnDiskDirEntry::LEN],
    ) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        let block = block_cache.read_mut(self.dir_pos_block(pos)).map_err(Error::DeviceError)?;
        let start = pos.byte_offset();
        block[start..start + OnDiskDirEntry::LEN].copy_from_slice(bytes);
        if pos.entry_in_block + 1 == ENTRIES_PER_BLOCK {
            trace!("Updating directory");
            block_cache.write_back()?;
        }
        Ok(())
    }

    /// Find an entry in a directory by its long name, or by its short name.
    ///
    /// Both comparisons ignore case. Volume label entries never match.
    pub(crate) fn find_entry_by_long_name<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        dir_info: &DirectoryInfo,
        name: &LongName<'_>,
    ) -> Result<DirEntry, Error<D::Error>>
    where
        D: BlockDevice,
    {
        #[derive(Clone, Copy)]
        enum State {
            /// Looking for the first (highest numbered) LFN entry
            Waiting,
            /// Found the start; now expecting entry number `next`
            Scanning { next: u8, csum: u8 },
            /// All LFN entries matched; the next short entry is ours if the
            /// checksum agrees
            Matched { csum: u8 },
        }

        let short = name.short_form();
        let num_entries = name.num_entries() as u8;
        let mut state = State::Waiting;
        let mut result = Err(Error::NotFound);
        self.iterate_dir_internal(block_cache, dir_info, |de, odde| {
            if let Some((start, seq, csum, chunk)) = odde.lfn_contents() {
                state = match (start, state) {
                    (true, _) => {
                        if seq == num_entries && name.chunk_matches(usize::from(seq), &chunk) {
                            if seq == 1 { State::Matched { csum } } else { State::Scanning { next: seq - 1, csum } }
                        } else {
                            State::Waiting
                        }
                    }
                    (false, State::Scanning { next, csum: want_csum })
                        if seq == next && csum == want_csum && name.chunk_matches(usize::from(seq), &chunk) =>
                    {
                        if seq == 1 {
                            State::Matched { csum }
                        } else {
                            State::Scanning { next: seq - 1, csum }
                        }
                    }
                    _ => State::Waiting,
                };
                return ControlFlow::Continue(());
            }
            let by_lfn = matches!(state, State::Matched { csum } if csum == de.name.csum());
            state = State::Waiting;
            if de.attributes.is_volume() {
                return ControlFlow::Continue(());
            }
            let by_sfn = short.as_ref().is_some_and(|s| de.name == *s);
            if by_lfn || by_sfn {
                result = Ok(de.clone());
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        result
    }

    /// Pick a short name for `name` that is unique within the directory.
    ///
    /// If the name fits 8.3 and the basis name is unused, the basis name is
    /// used as is. Otherwise the lowest free numeric tail `~1`, `~2`, ... is
    /// chosen (the directory is scanned once to collect the tails in use), and
    /// if more than 1023 files share the basis a hashed tail is used.
    ///
    /// `skip` names the on-disk location of one entry (block and offset)
    /// which is ignored during the scan: used when renaming, so the entry
    /// being renamed does not block its own short name.
    pub(crate) fn find_unique_short_name<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        dir_info: &DirectoryInfo,
        name: &LongName<'_>,
        skip: Option<(BlockIdx, u32)>,
    ) -> Result<ShortFileName, Error<D::Error>>
    where
        D: BlockDevice,
    {
        let basis = name.basis();
        // One bit per numeric tail 1..=MAX_SEQUENTIAL_TAIL
        let mut used = [0u32; (longname::max_sequential_tail() as usize + 1).div_ceil(32)];
        let mut plain_used = false;
        self.iterate_dir(block_cache, dir_info, |de| {
            if de.attributes.is_lfn() || de.attributes.is_volume() {
                return ControlFlow::Continue(());
            }
            if skip == Some((de.entry_block, de.entry_offset)) {
                return ControlFlow::Continue(());
            }
            if basis.is_plain(&de.name) {
                plain_used = true;
            } else if let Some(n) = basis.tail_of(&de.name) {
                used[(n / 32) as usize] |= 1 << (n % 32);
            }
            ControlFlow::Continue(())
        })?;
        if !basis.tail_required() && !plain_used {
            return Ok(basis.plain());
        }
        for n in 1..=longname::max_sequential_tail() {
            if used[(n / 32) as usize] & (1 << (n % 32)) == 0 {
                return Ok(basis.with_tail(n));
            }
        }
        // Over a thousand files with the same basis name: use a hashed tail
        for attempt in 0..HASHED_TAIL_ATTEMPTS {
            let candidate = basis.with_hashed_tail(name, attempt);
            match self.find_directory_entry(block_cache, dir_info, &candidate) {
                Err(Error::NotFound) => return Ok(candidate),
                Ok(_) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(Error::NotEnoughSpace)
    }

    /// Write the directory entries for a new name: the LFN entries (if the
    /// name needs any) immediately followed by the short entry.
    ///
    /// A run of free entries long enough for all of them is found first,
    /// re-using deleted entries where possible and growing the directory by a
    /// cluster when it is full. The entries are written last-LFN-part-first,
    /// as required by the specification.
    ///
    /// `template` supplies the attributes, cluster, size and timestamps of the
    /// short entry; its name and location are ignored. The returned entry
    /// describes the short entry as written.
    pub(crate) fn write_long_name_entries<D>(
        &mut self,
        block_cache: &mut BlockCache<D>,
        dir_info: &DirectoryInfo,
        name: &LongName<'_>,
        sfn: ShortFileName,
        template: &DirEntry,
    ) -> Result<DirEntry, Error<D::Error>>
    where
        D: BlockDevice,
    {
        let num_lfn = if name.needs_lfn_entries() { name.num_entries() } else { 0 };
        let slots = num_lfn + 1;

        // Find a run of `slots` free entries
        let mut pos = self.dir_first_pos(dir_info.cluster);
        let mut run_start: Option<DirPos> = None;
        let mut run_len = 0;
        let mut past_end = false;
        loop {
            let free = past_end || {
                let block = block_cache.read(self.dir_pos_block(pos)).map_err(Error::DeviceError)?;
                match block[pos.byte_offset()] {
                    END_ENTRY => {
                        past_end = true;
                        true
                    }
                    DELETED_ENTRY => true,
                    _ => false,
                }
            };
            if free {
                if run_start.is_none() {
                    run_start = Some(pos);
                }
                run_len += 1;
                if run_len == slots {
                    break;
                }
            } else {
                run_start = None;
                run_len = 0;
            }
            pos = match self.dir_next_pos(block_cache, pos)? {
                Some(next) => next,
                None => self.dir_grow(block_cache, pos.cluster)?,
            };
        }

        // Write the LFN entries, highest sequence number first
        let csum = sfn.csum();
        let mut pos = run_start.unwrap_or(pos);
        for seq in (1..=num_lfn).rev() {
            self.dir_put_entry(block_cache, pos, &name.entry_bytes(seq, csum))?;
            pos = self.dir_next_pos(block_cache, pos)?.ok_or(Error::FormatError("Directory run ended early"))?;
        }
        // Then the short entry
        let entry = DirEntry {
            name: sfn,
            mtime: template.mtime,
            ctime: template.ctime,
            attributes: template.attributes,
            cluster: template.cluster,
            size: template.size,
            entry_block: self.dir_pos_block(pos),
            entry_offset: pos.byte_offset() as u32,
        };
        self.dir_put_entry(block_cache, pos, &entry.serialize(self.get_fat_type()))?;
        trace!("Updating directory");
        block_cache.write_back()?;
        debug!("Wrote {:?} for {:?}", entry, name.as_str());
        Ok(entry)
    }

    /// Create a brand new entry (a file or a directory) with a long name.
    ///
    /// Picks a unique short name and writes the entries. The entry starts out
    /// with no cluster and a size of zero.
    pub(crate) fn create_long_name_entry<D>(
        &mut self,
        block_cache: &mut BlockCache<D>,
        dir_info: &DirectoryInfo,
        name: &LongName<'_>,
        attributes: Attributes,
        now: Timestamp,
    ) -> Result<DirEntry, Error<D::Error>>
    where
        D: BlockDevice,
    {
        let sfn = self.find_unique_short_name(block_cache, dir_info, name, None)?;
        let template = DirEntry::new(sfn, attributes, ClusterId::EMPTY, now, BlockIdx(0), 0);
        self.write_long_name_entries(block_cache, dir_info, name, sfn, &template)
    }

    /// Delete the short entry at `target` (block, byte offset) together with
    /// the LFN entries that precede it, by marking them all as unused.
    ///
    /// The file's cluster chain is not touched: see
    /// [`FatVolume::free_cluster_chain`].
    pub(crate) fn delete_long_name_entries<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        dir_info: &DirectoryInfo,
        target: (BlockIdx, u32),
    ) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        let mut pos = self.dir_first_pos(dir_info.cluster);
        // Where the LFN entries of the entry we are looking at started
        let mut run_start: Option<DirPos> = None;
        loop {
            let block_idx = self.dir_pos_block(pos);
            let offset = pos.byte_offset();
            let (first, attributes) = {
                let block = block_cache.read(block_idx).map_err(Error::DeviceError)?;
                (block[offset], Attributes::create_from_fat(block[offset + 11]))
            };
            if first == END_ENTRY {
                return Err(Error::NotFound);
            } else if first == DELETED_ENTRY {
                run_start = None;
            } else if attributes.is_lfn() {
                if first & longname::LFN_LAST_ENTRY_FLAG != 0 {
                    run_start = Some(pos);
                }
            } else if block_idx == target.0 && offset as u32 == target.1 {
                // Found it: wipe from the start of its LFN run to here
                let mut p = run_start.unwrap_or(pos);
                loop {
                    let block = block_cache.read_mut(self.dir_pos_block(p)).map_err(Error::DeviceError)?;
                    block[p.byte_offset()] = DELETED_ENTRY;
                    if p == pos {
                        trace!("Updating directory");
                        block_cache.write_back()?;
                        return Ok(());
                    }
                    if p.entry_in_block + 1 == ENTRIES_PER_BLOCK {
                        // Leaving this block: flush before the FAT is consulted
                        trace!("Updating directory");
                        block_cache.write_back()?;
                    }
                    p = self.dir_next_pos(block_cache, p)?.ok_or(Error::FormatError("Directory ended inside an entry run"))?;
                }
            } else {
                run_start = None;
            }
            pos = match self.dir_next_pos(block_cache, pos)? {
                Some(next) => next,
                None => return Err(Error::NotFound),
            };
        }
    }

    /// Free every cluster of the chain starting at `first`.
    pub(crate) fn free_cluster_chain<D>(&mut self, block_cache: &mut BlockCache<D>, first: ClusterId) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        if first.0 < RESERVED_ENTRIES || first == ClusterId::ROOT_DIR {
            // Nothing allocated (or the root directory, which is never freed)
            return Ok(());
        }
        let mut cluster = first;
        loop {
            let next = self.next_cluster(block_cache, cluster);
            self.update_fat(block_cache, cluster, ClusterId::EMPTY)?;
            if let Some(ref mut count) = self.free_clusters_count {
                *count += 1;
            }
            match self.next_free_cluster {
                Some(c) if c.0 <= cluster.0 => {}
                _ => self.next_free_cluster = Some(cluster),
            }
            match next {
                Ok(n) if n.0 >= RESERVED_ENTRIES => cluster = n,
                Ok(_) | Err(Error::EndOfFile) | Err(Error::UnterminatedFatChain) => {
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Allocate a cluster for a new directory and write its `.` and `..`
    /// entries. The cluster number is stored into `entry` (which must be the
    /// entry of the directory in its parent) and the entry is rewritten.
    pub(crate) fn init_new_directory<D>(
        &mut self,
        block_cache: &mut BlockCache<D>,
        entry: &mut DirEntry,
        parent: ClusterId,
        now: Timestamp,
    ) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        if entry.cluster == ClusterId::EMPTY || entry.cluster == ClusterId::ROOT_DIR {
            entry.cluster = self.alloc_cluster(block_cache, None, false)?;
            self.write_entry_to_disk(block_cache, entry)?;
        }
        self.init_dir_cluster(block_cache, entry.cluster, parent, entry.attributes, now)
    }

    /// Fill a directory's first cluster with the `.` and `..` entries and
    /// zero the rest of it.
    pub(crate) fn init_dir_cluster<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        cluster: ClusterId,
        parent: ClusterId,
        attributes: Attributes,
        now: Timestamp,
    ) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        let start_block = self.cluster_to_block(cluster);
        let fat_type = self.get_fat_type();
        let block = block_cache.blank_mut(start_block);
        let dot = DirEntry {
            name: ShortFileName::this_dir(),
            mtime: now,
            ctime: now,
            attributes,
            cluster,
            size: 0,
            entry_block: start_block,
            entry_offset: 0,
        };
        block[..OnDiskDirEntry::LEN].copy_from_slice(&dot.serialize(fat_type));
        let dot_dot = DirEntry {
            name: ShortFileName::parent_dir(),
            mtime: now,
            ctime: now,
            attributes,
            cluster: self.dot_dot_cluster(parent),
            size: 0,
            entry_block: start_block,
            entry_offset: OnDiskDirEntry::LEN_U32,
        };
        block[OnDiskDirEntry::LEN..2 * OnDiskDirEntry::LEN].copy_from_slice(&dot_dot.serialize(fat_type));
        block_cache.write_back()?;
        for block_idx in start_block.range(BlockCount(u32::from(self.blocks_per_cluster))).skip(1) {
            let _ = block_cache.blank_mut(block_idx);
            block_cache.write_back()?;
        }
        Ok(())
    }

    /// The cluster number a `..` entry stores for the given parent: `0`
    /// means the root directory.
    fn dot_dot_cluster(&self, parent: ClusterId) -> ClusterId {
        if parent == ClusterId::ROOT_DIR { ClusterId::EMPTY } else { parent }
    }

    /// Point the `..` entry of the directory in `cluster` at `new_parent`.
    pub(crate) fn update_dot_dot<D>(
        &self,
        block_cache: &mut BlockCache<D>,
        cluster: ClusterId,
        new_parent: ClusterId,
    ) -> Result<(), Error<D::Error>>
    where
        D: BlockDevice,
    {
        let block_idx = self.cluster_to_block(cluster);
        let block = block_cache.read_mut(block_idx).map_err(Error::DeviceError)?;
        let entry = &mut block[OnDiskDirEntry::LEN..2 * OnDiskDirEntry::LEN];
        if entry[..11] != ShortFileName::parent_dir().contents {
            return Err(Error::FormatError("Directory has no '..' entry"));
        }
        let value = self.dot_dot_cluster(new_parent).0;
        entry[20..22].copy_from_slice(&((value >> 16) as u16).to_le_bytes());
        entry[26..28].copy_from_slice(&(value as u16).to_le_bytes());
        trace!("Updating '..' entry");
        block_cache.write_back()?;
        Ok(())
    }

    /// The parent of the directory in `cluster`, read from its `..` entry.
    pub(crate) fn parent_of_dir<D>(&self, block_cache: &mut BlockCache<D>, cluster: ClusterId) -> Result<ClusterId, Error<D::Error>>
    where
        D: BlockDevice,
    {
        if cluster == ClusterId::ROOT_DIR {
            return Ok(ClusterId::ROOT_DIR);
        }
        let block_idx = self.cluster_to_block(cluster);
        let block = block_cache.read(block_idx).map_err(Error::DeviceError)?;
        let odde = OnDiskDirEntry::new(&block[OnDiskDirEntry::LEN..2 * OnDiskDirEntry::LEN]);
        if !odde.matches(&ShortFileName::parent_dir()) {
            return Err(Error::FormatError("Directory has no '..' entry"));
        }
        let entry = odde.get_entry(self.get_fat_type(), block_idx, OnDiskDirEntry::LEN_U32);
        Ok(self.dir_start_cluster_to_root(entry.cluster))
    }

    /// Map the FAT32 root directory's real cluster number back to
    /// [`ClusterId::ROOT_DIR`].
    fn dir_start_cluster_to_root(&self, cluster: ClusterId) -> ClusterId {
        match &self.fat_specific_info {
            FatSpecificInfo::Fat32(info) if cluster == info.first_root_dir_cluster => ClusterId::ROOT_DIR,
            _ => cluster,
        }
    }

    /// Is the directory in `cluster` empty (apart from `.` and `..`)?
    pub(crate) fn dir_is_empty<D>(&self, block_cache: &mut BlockCache<D>, dir_info: &DirectoryInfo) -> Result<bool, Error<D::Error>>
    where
        D: BlockDevice,
    {
        let mut empty = true;
        self.iterate_dir(block_cache, dir_info, |de| {
            if !de.attributes.is_lfn() && de.name != ShortFileName::this_dir() && de.name != ShortFileName::parent_dir() {
                empty = false;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        Ok(empty)
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
