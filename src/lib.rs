use {
    crate::{
        append_vec::{AppendVec, StoredAccountMeta},
        solana::{
            deserialize_from, AccountsDbFields, DeserializableVersionedBank,
            SerializableAccountStorageEntry,
        },
    },
    std::{ffi::OsStr, io::Read, path::Path, str::FromStr},
    thiserror::Error,
};

pub mod append_vec;
pub mod archived;
pub mod parallel;
pub mod solana;
pub mod unpacked;

const SNAPSHOTS_DIR: &str = "snapshots";

#[derive(Error, Debug)]
pub enum SnapshotError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("Failed to deserialize: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("Missing status cache")]
    NoStatusCache,
    #[error("No snapshot manifest file found")]
    NoSnapshotManifest,
    #[error("Unexpected AppendVec")]
    UnexpectedAppendVec,
    #[error("Failed to create read progress tracking: {0}")]
    ReadProgressTracking(String),
}

pub type SnapshotResult<T> = Result<T, SnapshotError>;

pub type AppendVecIterator<'a> = Box<dyn Iterator<Item = SnapshotResult<AppendVec>> + 'a>;

pub trait SnapshotExtractor: Sized {
    fn iter(&mut self) -> AppendVecIterator<'_>;
}

fn parse_append_vec_name(name: &OsStr) -> Option<(u64, u64)> {
    let name = name.to_str()?;
    let mut parts = name.splitn(2, '.');
    let slot = u64::from_str(parts.next().unwrap_or(""));
    let id = u64::from_str(parts.next().unwrap_or(""));
    match (slot, id) {
        (Ok(slot), Ok(version)) => Some((slot, version)),
        _ => None,
    }
}

pub fn append_vec_iter(append_vec: &AppendVec) -> impl Iterator<Item = StoredAccountMetaHandle> {
    let mut offsets = Vec::<usize>::new();
    let mut offset = 0usize;
    loop {
        match append_vec.get_account(offset) {
            None => break,
            Some((_, next_offset)) => {
                offsets.push(offset);
                offset = next_offset;
            }
        }
    }
    offsets
        .into_iter()
        .map(move |offset| StoredAccountMetaHandle::new(append_vec, offset))
}

pub struct StoredAccountMetaHandle<'a> {
    append_vec: &'a AppendVec,
    offset: usize,
}

impl<'a> StoredAccountMetaHandle<'a> {
    pub const fn new(append_vec: &'a AppendVec, offset: usize) -> StoredAccountMetaHandle {
        Self { append_vec, offset }
    }

    pub fn access(&self) -> Option<StoredAccountMeta<'_>> {
        Some(self.append_vec.get_account(self.offset)?.0)
    }
}

pub trait ReadProgressTracking {
    fn new_read_progress_tracker(
        &self,
        path: &Path,
        rd: Box<dyn Read>,
        file_len: u64,
    ) -> SnapshotResult<Box<dyn Read>>;
}

struct NoopReadProgressTracking {}

impl ReadProgressTracking for NoopReadProgressTracking {
    fn new_read_progress_tracker(
        &self,
        _path: &Path,
        rd: Box<dyn Read>,
        _file_len: u64,
    ) -> SnapshotResult<Box<dyn Read>> {
        Ok(rd)
    }
}
