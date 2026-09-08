//! Index structure and operations for log files.

use crate::error::TcsLogError;
use crate::{Uid, BLOCK_SIZE};
use std::mem::size_of;

/// Size of an index entry (offset + packed Uid). The Uid's
/// packed size (`Uid::PACKLEN`) is used rather than `size_of::<Uid>()`,
/// which includes struct padding.
pub const INDEX_ENTRY_SIZE: usize = size_of::<u64>() + Uid::PACKLEN;

/// Number of index entries per block.
pub const ENTRIES_PER_BLOCK: usize = BLOCK_SIZE / INDEX_ENTRY_SIZE;

/// Null offset value indicating no reference.
pub const FILE_NULL: u64 = 0;

/// Represents a single index entry.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexEntry {
    /// Offset of the referenced file block within the file.
    /// FILE_NULL if this entry does not reference any block.
    pub offset: u64,
    /// Uid in nanoseconds since UNIX epoch.
    pub Uid: Uid,
}

impl IndexEntry {
    /// Creates a new index entry.
    pub fn new(offset: u64, Uid: Uid) -> Self {
        IndexEntry { offset, Uid }
    }

    /// Creates a null index entry.
    pub fn null() -> Self {
        IndexEntry {
            offset: FILE_NULL,
            Uid: Uid::ZERO,
        }
    }

    /// Returns true if this is a null entry.
    pub fn is_null(&self) -> bool {
        self.offset == FILE_NULL
    }

    /// Serializes the entry to bytes (little-endian).
    pub fn to_bytes(&self) -> [u8; INDEX_ENTRY_SIZE] {
        let mut bytes = [0u8; INDEX_ENTRY_SIZE];
        let i = size_of::<u64>();
        bytes[0..i].copy_from_slice(&self.offset.to_le_bytes());
        bytes[i..i + Uid::PACKLEN].copy_from_slice(&self.Uid.to_le_bytes());
        bytes
    }

    /// Deserializes an entry from bytes.
    pub fn from_bytes(bytes: &[u8; INDEX_ENTRY_SIZE]) -> Self {
        let i = size_of::<u64>();
        let offset = u64::from_le_bytes(bytes[0..i].try_into().unwrap());
        let Uid = Uid::from_le_bytes(bytes[i..i + Uid::PACKLEN].try_into().unwrap());
        IndexEntry { offset, Uid }
    }

    pub fn pathlen(&self) -> usize {
        unimplemented!();
    }
}

/// Represents a block of index entries.
#[derive(Debug, Clone)]
pub struct IndexBlock {
    /// The entries in this block.
    pub entries: [IndexEntry; ENTRIES_PER_BLOCK],
}

impl Default for IndexBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexBlock {
    /// Creates a new empty index block.
    pub fn new() -> Self {
        IndexBlock {
            entries: [IndexEntry::null(); ENTRIES_PER_BLOCK],
        }
    }

    /// Serializes the block to bytes.
    pub fn to_bytes(&self) -> [u8; BLOCK_SIZE] {
        let mut bytes = [0u8; BLOCK_SIZE];
        for (i, entry) in self.entries.iter().enumerate() {
            let offset = i * INDEX_ENTRY_SIZE;
            bytes[offset..offset + INDEX_ENTRY_SIZE].copy_from_slice(&entry.to_bytes());
        }
        bytes
    }

    /// Deserializes a block from bytes.
    pub fn from_bytes(bytes: &[u8; BLOCK_SIZE]) -> Self {
        let mut entries = [IndexEntry::null(); ENTRIES_PER_BLOCK];
        for (i, entry) in entries.iter_mut().enumerate() {
            let offset = i * INDEX_ENTRY_SIZE;
            let entry_bytes: [u8; INDEX_ENTRY_SIZE] =
                bytes[offset..offset + INDEX_ENTRY_SIZE].try_into().unwrap();
            *entry = IndexEntry::from_bytes(&entry_bytes);
        }
        IndexBlock { entries }
    }

    /// Finds the entry with the largest Uid less than or equal to the given Uid.
    /// Returns the index of the entry, or None if no such entry exists.
    pub fn find_le(&self, Uid: Uid) -> Option<usize> {
        let mut result = None;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.is_null() {
                break;
            }
            if entry.Uid <= Uid {
                result = Some(i);
            } else {
                break;
            }
        }
        result
    }

    /// Returns the number of non-null entries.
    pub fn count(&self) -> usize {
        self.entries.iter().take_while(|e| !e.is_null()).count()
    }
}

/// Computes the minimum number of index blocks needed for the given number of data blocks.
#[allow(unused)]
pub fn compute_index_blocks(data_blocks: usize) -> usize {
    if data_blocks == 0 {
        return 1;
    }

    let mut blocks_to_index = data_blocks;
    let mut total_index_blocks = 0;

    // Each level indexes the blocks from the level above (or data blocks for the top level)
    while blocks_to_index > 0 {
        let blocks_this_level = (blocks_to_index + ENTRIES_PER_BLOCK - 1) / ENTRIES_PER_BLOCK;
        total_index_blocks += blocks_this_level;

        if blocks_this_level == 1 {
            break;
        }
        blocks_to_index = blocks_this_level;
    }

    total_index_blocks.max(1)
}

/// Computes the index structure for a given file size.
#[derive(Debug)]
pub struct IndexStructure {
    /// Number of levels in the index.
    pub levels: usize,
    /// Total number of index blocks.
    pub total_blocks: usize,
    /// Number of blocks at each level (level 0 is closest to header).
    pub blocks_per_level: Vec<usize>,
}

impl IndexStructure {
    /// Computes the index structure for the given number of data blocks.
    pub fn compute<'a>(data_blocks: usize) -> Result<Self, TcsLogError<'a>> {
        if data_blocks == 0 {
            return Ok(IndexStructure {
                levels: 1,
                total_blocks: 1,
                blocks_per_level: vec![1],
            });
        }

        let mut blocks_per_level = Vec::new();
        let mut blocks_to_index = data_blocks;

        // Build levels from top (closest to data) to bottom (closest to header)
        while blocks_to_index > 0 {
            let blocks_this_level = (blocks_to_index + ENTRIES_PER_BLOCK - 1) / ENTRIES_PER_BLOCK;
            blocks_per_level.push(blocks_this_level);

            if blocks_this_level == 1 {
                break;
            }
            blocks_to_index = blocks_this_level;
        }

        // Reverse so level 0 is at the root (closest to header)
        blocks_per_level.reverse();

        let total_blocks = blocks_per_level.iter().sum();
        let levels = blocks_per_level.len();

        Ok(IndexStructure {
            levels,
            total_blocks,
            blocks_per_level,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_entry_roundtrip() {
        let entry = IndexEntry::new(
            0x1234_5678_9ABC_DEF0,
            Uid::from_nanos(0xFEDC_BA98_7654_3210),
        );
        let bytes = entry.to_bytes();
        let restored = IndexEntry::from_bytes(&bytes);
        assert_eq!(entry.offset, restored.offset);
        assert_eq!(entry.Uid, restored.Uid);
    }

    #[test]
    fn test_index_block_roundtrip() {
        let mut block = IndexBlock::new();
        block.entries[0] = IndexEntry::new(4096, Uid::from_nanos(1000));
        block.entries[1] = IndexEntry::new(8192, Uid::from_nanos(2000));

        let bytes = block.to_bytes();
        let restored = IndexBlock::from_bytes(&bytes);

        assert_eq!(block.entries[0].offset, restored.entries[0].offset);
        assert_eq!(block.entries[0].Uid, restored.entries[0].Uid);
        assert_eq!(block.entries[1].offset, restored.entries[1].offset);
        assert_eq!(block.entries[1].Uid, restored.entries[1].Uid);
    }

    #[test]
    fn test_find_le() {
        let mut block = IndexBlock::new();
        block.entries[0] = IndexEntry::new(4096, Uid::from_nanos(1000));
        block.entries[1] = IndexEntry::new(8192, Uid::from_nanos(2000));
        block.entries[2] = IndexEntry::new(12288, Uid::from_nanos(3000));

        assert_eq!(block.find_le(Uid::from_nanos(500)), None);
        assert_eq!(block.find_le(Uid::from_nanos(1000)), Some(0));
        assert_eq!(block.find_le(Uid::from_nanos(1500)), Some(0));
        assert_eq!(block.find_le(Uid::from_nanos(2000)), Some(1));
        assert_eq!(block.find_le(Uid::from_nanos(3500)), Some(2));
    }

    #[test]
    fn test_index_structure_single_level() {
        let structure = IndexStructure::compute(100).unwrap();
        assert_eq!(structure.levels, 1);
        assert_eq!(structure.total_blocks, 1);
    }

    #[test]
    fn test_entries_per_block() {
        // With BLOCK_SIZE = 4096 and INDEX_ENTRY_SIZE = 16
        // ENTRIES_PER_BLOCK should be 256
        assert_eq!(ENTRIES_PER_BLOCK, BLOCK_SIZE / INDEX_ENTRY_SIZE);
    }
}
