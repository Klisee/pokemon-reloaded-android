//! Clean-room save interception, atomic commit, snapshot backup, and rollback.
//!
//! Derived only from the behavioural requirements in `save-hooks.md`,
//! `interfaces.md`, and `security.md`. Persistent bodies are opaque bytes and
//! filesystem paths stay inside the adapter.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type Revision = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub fn now() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .min(i64::MAX as u64);
        Timestamp(seconds as i64)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    pub const ZERO: Digest = Digest([0u8; 32]);
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorldId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SaveSlot(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SnapshotId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FormatId(pub [u8; 8]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ErrorKind {
    Unavailable,
    Timeout,
    Cancelled,
    Corrupt,
    UnsupportedVersion,
    Conflict,
    Rejected,
    Protocol,
    Overloaded,
    Io,
    Internal,
}

pub trait PortError: std::error::Error + Send + Sync + 'static {
    fn kind(&self) -> ErrorKind;
    fn retryable(&self) -> bool;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveError {
    kind: ErrorKind,
    message: String,
    retryable: bool,
}

impl SaveError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        let retryable = matches!(
            kind,
            ErrorKind::Unavailable | ErrorKind::Timeout | ErrorKind::Overloaded | ErrorKind::Io
        );
        SaveError {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn rejected(message: impl Into<String>) -> Self {
        SaveError::new(ErrorKind::Rejected, message)
    }

    pub fn corrupt(message: impl Into<String>) -> Self {
        SaveError::new(ErrorKind::Corrupt, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        SaveError::new(ErrorKind::Conflict, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        SaveError::new(ErrorKind::UnsupportedVersion, message)
    }

    pub fn overloaded(message: impl Into<String>) -> Self {
        SaveError::new(ErrorKind::Overloaded, message)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SaveError {}

impl PortError for SaveError {
    fn kind(&self) -> ErrorKind {
        self.kind
    }

    fn retryable(&self) -> bool {
        self.retryable
    }
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        let kind = match error.kind() {
            io::ErrorKind::NotFound => ErrorKind::Unavailable,
            io::ErrorKind::PermissionDenied => ErrorKind::Rejected,
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => ErrorKind::Timeout,
            _ => ErrorKind::Io,
        };
        let retryable = matches!(
            kind,
            ErrorKind::Unavailable | ErrorKind::Timeout | ErrorKind::Io
        );
        SaveError {
            kind,
            message: error.to_string(),
            retryable,
        }
    }
}

#[derive(Clone)]
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buffered: 0,
            length: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    fn finalize(mut self) -> Digest {
        let bit_length = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_length.to_be_bytes());
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        Digest(out)
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

pub fn digest_bytes(bytes: &[u8]) -> Digest {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.finalize()
}

pub const CURRENT_IMAGE_VERSION: u16 = 1;
pub const CURRENT_MANIFEST_VERSION: u16 = 1;
pub const CURRENT_IMAGE_SCHEMA: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};
pub const MAX_PARTS: usize = 64;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

const IMAGE_MAGIC: &[u8; 8] = b"SHIMG001";
const MANIFEST_MAGIC: &[u8; 8] = b"SHMAN001";
const IMAGE_FILE: &str = "image.bin";
const MANIFEST_FILE: &str = "manifest.bin";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveImage {
    pub format: FormatId,
    pub schema: Version,
    pub revision: Revision,
    pub parts: BTreeMap<String, Arc<[u8]>>,
}

impl SaveImage {
    pub fn empty(format: FormatId, schema: Version, revision: Revision) -> Self {
        SaveImage {
            format,
            schema,
            revision,
            parts: BTreeMap::new(),
        }
    }

    pub fn single(
        format: FormatId,
        schema: Version,
        revision: Revision,
        name: impl Into<String>,
        body: impl Into<Arc<[u8]>>,
    ) -> Self {
        let mut parts = BTreeMap::new();
        parts.insert(name.into(), body.into());
        SaveImage {
            format,
            schema,
            revision,
            parts,
        }
    }

    pub fn total_len(&self) -> Result<u64, SaveError> {
        let mut total = 0u64;
        for body in self.parts.values() {
            total = total
                .checked_add(body.len() as u64)
                .ok_or_else(|| SaveError::overloaded("image size overflow"))?;
        }
        if total > MAX_IMAGE_BYTES {
            return Err(SaveError::overloaded("image exceeds hard size ceiling"));
        }
        Ok(total)
    }

    pub fn validate(&self) -> Result<(), SaveError> {
        if self.schema.major != CURRENT_IMAGE_SCHEMA.major {
            return Err(SaveError::unsupported("unsupported image schema"));
        }
        if self.parts.len() > MAX_PARTS {
            return Err(SaveError::rejected("too many image parts"));
        }
        let mut total = 0u64;
        for (name, body) in &self.parts {
            validate_part_name(name).map_err(|_| SaveError::rejected("invalid part name"))?;
            let size = body.len() as u64;
            if size > MAX_PART_BYTES {
                return Err(SaveError::overloaded("part exceeds size ceiling"));
            }
            total = total
                .checked_add(size)
                .ok_or_else(|| SaveError::overloaded("image size overflow"))?;
        }
        if total > MAX_IMAGE_BYTES {
            return Err(SaveError::overloaded("image exceeds size ceiling"));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<Digest, SaveError> {
        Ok(digest_bytes(&self.encode()?))
    }

    fn encode(&self) -> Result<Vec<u8>, SaveError> {
        self.validate()?;
        let mut out = Vec::new();
        out.extend_from_slice(IMAGE_MAGIC);
        push_u16(&mut out, CURRENT_IMAGE_VERSION);
        out.extend_from_slice(&self.format.0);
        push_version(&mut out, self.schema);
        push_u64(&mut out, self.revision);
        push_u32(&mut out, self.parts.len() as u32);
        for (name, body) in &self.parts {
            push_u16(&mut out, name.len() as u16);
            out.extend_from_slice(name.as_bytes());
            push_u64(&mut out, body.len() as u64);
            out.extend_from_slice(body);
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self, SaveError> {
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != IMAGE_MAGIC {
            return Err(SaveError::corrupt("bad image magic"));
        }
        let version = reader.u16()?;
        if version != CURRENT_IMAGE_VERSION {
            return Err(SaveError::unsupported("unsupported image version"));
        }
        let format = FormatId(reader.array::<8>()?);
        let schema = reader.version()?;
        if schema.major != CURRENT_IMAGE_SCHEMA.major {
            return Err(SaveError::unsupported("unsupported image schema"));
        }
        let revision = reader.u64()?;
        let count = reader.u32()? as usize;
        if count > MAX_PARTS {
            return Err(SaveError::corrupt("too many image parts"));
        }
        let mut parts = BTreeMap::new();
        let mut total = 0u64;
        for _ in 0..count {
            let name = reader.name()?;
            let size = reader.u64()?;
            if size > MAX_PART_BYTES {
                return Err(SaveError::corrupt("image part exceeds ceiling"));
            }
            let body = reader.take(size as usize)?;
            total = total
                .checked_add(size)
                .ok_or_else(|| SaveError::corrupt("image size overflow"))?;
            if total > MAX_IMAGE_BYTES {
                return Err(SaveError::corrupt("image exceeds ceiling"));
            }
            if parts.insert(name, Arc::from(body)).is_some() {
                return Err(SaveError::corrupt("duplicate image part"));
            }
        }
        reader.finish()?;
        Ok(SaveImage {
            format,
            schema,
            revision,
            parts,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveSnapshot {
    pub id: SnapshotId,
    pub world: WorldId,
    pub slot: SaveSlot,
    pub format: FormatId,
    pub schema: Version,
    pub revision: Revision,
    pub captured_at: Timestamp,
    pub digest: Digest,
    pub body: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveMode {
    Replace,
    FailIfExists,
    VerifyOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveTarget {
    pub slot: SaveSlot,
    pub mode: SaveMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility {
    Writable,
    ReadOnly,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotVerification {
    pub valid: bool,
    pub compatibility: Compatibility,
    pub digest_matches: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveCommit {
    pub slot: SaveSlot,
    pub previous: Revision,
    pub current: Revision,
    pub digest: Digest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreReceipt {
    pub previous: Revision,
    pub current: Revision,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SnapshotRecord {
    pub snapshot: SaveSnapshot,
    pub baseline: bool,
    pub epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct SnapshotEntry {
    pub id: SnapshotId,
    pub epoch: u64,
    pub sequence: u64,
    pub baseline: bool,
    pub captured_at: Timestamp,
    pub size_bytes: u64,
    pub valid: bool,
}

#[derive(Clone, Debug)]
struct PartEntry {
    name: String,
    size: u64,
    digest: Digest,
}

#[derive(Clone, Debug)]
struct Manifest {
    version: u16,
    baseline: bool,
    epoch: u64,
    sequence: u64,
    captured_at: Timestamp,
    world: WorldId,
    slot: SaveSlot,
    format: FormatId,
    schema: Version,
    revision: Revision,
    id: SnapshotId,
    image_digest: Digest,
    total_bytes: u64,
    parts: Vec<PartEntry>,
}

impl Manifest {
    fn encode(&self) -> Result<Vec<u8>, SaveError> {
        let mut out = Vec::new();
        out.extend_from_slice(MANIFEST_MAGIC);
        push_u16(&mut out, self.version);
        out.push(if self.baseline { 1 } else { 0 });
        push_u64(&mut out, self.epoch);
        push_u64(&mut out, self.sequence);
        push_i64(&mut out, self.captured_at.0);
        out.extend_from_slice(&self.world.0);
        push_u16(&mut out, self.slot.0);
        out.extend_from_slice(&self.format.0);
        push_version(&mut out, self.schema);
        push_u64(&mut out, self.revision);
        out.extend_from_slice(&self.id.0);
        out.extend_from_slice(&self.image_digest.0);
        push_u64(&mut out, self.total_bytes);
        push_u32(&mut out, self.parts.len() as u32);
        for part in &self.parts {
            push_u16(&mut out, part.name.len() as u16);
            out.extend_from_slice(part.name.as_bytes());
            push_u64(&mut out, part.size);
            out.extend_from_slice(&part.digest.0);
        }
        if out.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(SaveError::overloaded("manifest exceeds ceiling"));
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self, SaveError> {
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MANIFEST_MAGIC {
            return Err(SaveError::corrupt("bad manifest magic"));
        }
        let version = reader.u16()?;
        if version != CURRENT_MANIFEST_VERSION {
            return Err(SaveError::unsupported("unsupported manifest version"));
        }
        let flags = reader.u8()?;
        if flags & !1 != 0 {
            return Err(SaveError::corrupt("unknown manifest flags"));
        }
        let baseline = flags & 1 == 1;
        let epoch = reader.u64()?;
        let sequence = reader.u64()?;
        let captured_at = Timestamp(reader.i64()?);
        let world = WorldId(reader.array::<16>()?);
        let slot = SaveSlot(reader.u16()?);
        let format = FormatId(reader.array::<8>()?);
        let schema = reader.version()?;
        if schema.major != CURRENT_IMAGE_SCHEMA.major {
            return Err(SaveError::unsupported("unsupported manifest schema"));
        }
        let revision = reader.u64()?;
        let id = SnapshotId(reader.array::<16>()?);
        let image_digest = Digest(reader.array::<32>()?);
        let total_bytes = reader.u64()?;
        if total_bytes > MAX_IMAGE_BYTES {
            return Err(SaveError::corrupt("manifest image size exceeds ceiling"));
        }
        let count = reader.u32()? as usize;
        if count > MAX_PARTS {
            return Err(SaveError::corrupt("too many manifest parts"));
        }
        let mut parts = Vec::with_capacity(count);
        let mut seen = HashSet::new();
        let mut summed = 0u64;
        for _ in 0..count {
            let name = reader.name()?;
            if !seen.insert(name.clone()) {
                return Err(SaveError::corrupt("duplicate manifest part"));
            }
            let size = reader.u64()?;
            if size > MAX_PART_BYTES {
                return Err(SaveError::corrupt("manifest part exceeds ceiling"));
            }
            summed = summed
                .checked_add(size)
                .ok_or_else(|| SaveError::corrupt("manifest size overflow"))?;
            let digest = Digest(reader.array::<32>()?);
            parts.push(PartEntry { name, size, digest });
        }
        reader.finish()?;
        if summed != total_bytes {
            return Err(SaveError::corrupt("manifest size mismatch"));
        }
        Ok(Manifest {
            version,
            baseline,
            epoch,
            sequence,
            captured_at,
            world,
            slot,
            format,
            schema,
            revision,
            id,
            image_digest,
            total_bytes,
            parts,
        })
    }
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_version(out: &mut Vec<u8>, value: Version) {
    push_u16(out, value.major);
    push_u16(out, value.minor);
    push_u16(out, value.patch);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], SaveError> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| SaveError::corrupt("length overflow"))?;
        if end > self.data.len() {
            return Err(SaveError::corrupt("truncated container"));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SaveError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, SaveError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, SaveError> {
        let bytes = self.take(8)?;
        let mut out = [0u8; 8];
        out.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(out))
    }

    fn i64(&mut self) -> Result<i64, SaveError> {
        Ok(self.u64()? as i64)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SaveError> {
        let bytes = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn version(&mut self) -> Result<Version, SaveError> {
        Ok(Version {
            major: self.u16()?,
            minor: self.u16()?,
            patch: self.u16()?,
        })
    }

    fn name(&mut self) -> Result<String, SaveError> {
        let length = self.u16()? as usize;
        if length == 0 || length > MAX_NAME_BYTES {
            return Err(SaveError::corrupt("invalid part name length"));
        }
        let bytes = self.take(length)?;
        let name = std::str::from_utf8(bytes)
            .map_err(|_| SaveError::corrupt("part name is not UTF-8"))?
            .to_string();
        if !part_name_is_valid(&name) {
            return Err(SaveError::corrupt("invalid part name"));
        }
        Ok(name)
    }

    fn finish(&self) -> Result<(), SaveError> {
        if self.pos != self.data.len() {
            return Err(SaveError::corrupt("trailing container data"));
        }
        Ok(())
    }
}

fn part_name_is_valid(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return false;
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return false;
    }
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_part_name(name: &str) -> Result<(), SaveError> {
    if part_name_is_valid(name) {
        Ok(())
    } else {
        Err(SaveError::rejected("invalid part name"))
    }
}

fn hex16(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn parse_hex16(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for index in 0..16 {
        let high = hex_nibble(text.as_bytes()[index * 2])?;
        let low = hex_nibble(text.as_bytes()[index * 2 + 1])?;
        out[index] = (high << 4) | low;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn snapshot_id(world: WorldId, epoch: u64, sequence: u64, digest: &Digest) -> SnapshotId {
    let mut hash = Sha256::new();
    hash.update(b"snapshot-id");
    hash.update(&world.0);
    hash.update(&epoch.to_be_bytes());
    hash.update(&sequence.to_be_bytes());
    hash.update(&digest.0);
    let full = hash.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&full.0[..16]);
    SnapshotId(out)
}

fn invalid_verification(reason: &str) -> SnapshotVerification {
    SnapshotVerification {
        valid: false,
        compatibility: Compatibility::Unsupported,
        digest_matches: false,
        warnings: vec![reason.to_string()],
    }
}

pub fn confined_join(root: &Path, relative: &Path) -> Result<PathBuf, SaveError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(SaveError::rejected("path must be relative"));
    }
    let root = fs::canonicalize(root)?;
    let mut out = root.clone();
    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                out.push(name);
                match fs::symlink_metadata(&out) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(SaveError::rejected("symlink escapes confinement"));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            _ => return Err(SaveError::rejected("non-normal path component")),
        }
    }
    let mut probe = out.clone();
    loop {
        match fs::canonicalize(&probe) {
            Ok(canonical) => {
                if !canonical.starts_with(&root) {
                    return Err(SaveError::rejected("path escapes root"));
                }
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !probe.pop() {
                    return Err(SaveError::rejected("path escapes root"));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(out)
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<(), SaveError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<(), SaveError> {
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), SaveError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(source: Option<&Path>, allow_world_readable: bool) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = source
        .and_then(|path| fs::metadata(path).ok())
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0o600);
    if !allow_world_readable {
        mode &= !0o007;
    }
    mode |= 0o600;
    mode
}

#[cfg(not(unix))]
fn file_mode(_source: Option<&Path>, _allow_world_readable: bool) -> u32 {
    0
}

fn ensure_dir_no_symlink(path: &Path) -> Result<(), SaveError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(SaveError::rejected("symlink in generated path"));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(SaveError::rejected("generated path is not a directory"));
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(path)?;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(SaveError::rejected("generated path is not a directory"));
    }
    Ok(())
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, SaveError> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(SaveError::rejected("artifact is a symlink"));
    }
    if !meta.is_file() {
        return Err(SaveError::corrupt("artifact is not a regular file"));
    }
    if meta.len() > limit {
        return Err(SaveError::overloaded("artifact exceeds read ceiling"));
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > limit {
        return Err(SaveError::overloaded("artifact exceeds read ceiling"));
    }
    Ok(bytes)
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&child) {
                if meta.file_type().is_symlink() {
                    continue;
                }
                let size = if meta.is_dir() {
                    dir_size(&child)
                } else {
                    meta.len()
                };
                total = total.saturating_add(size);
            }
        }
    }
    total
}

fn sanitize(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if out.len() >= 240 {
            out.push_str("...");
            break;
        }
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            ch if ch.is_control() => out.push('?'),
            ch => out.push(ch),
        }
    }
    out
}

fn slot_label(slot: SaveSlot) -> String {
    format!("slot:{}", slot.0)
}

#[derive(Clone, Copy, Debug)]
pub struct CommitOptions {
    pub mode: SaveMode,
    pub allow_world_readable: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SnapshotBudget {
    pub max_total_bytes: u64,
    pub min_free_bytes: u64,
    pub allow_world_readable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotCause {
    Baseline,
    Rolling,
    Manual,
    PreRollback,
}

#[derive(Clone, Copy, Debug)]
pub struct SnapshotMetaRequest {
    pub epoch: u64,
    pub baseline: bool,
    pub cause: SnapshotCause,
    pub captured_at: Timestamp,
}

pub trait SpaceProbe: Send + Sync {
    fn available_bytes(&self, root: &Path) -> Result<u64, SaveError>;
}

pub struct UnboundedSpace;

impl SpaceProbe for UnboundedSpace {
    fn available_bytes(&self, _root: &Path) -> Result<u64, SaveError> {
        Ok(u64::MAX)
    }
}

pub struct FixedSpace(pub u64);

impl SpaceProbe for FixedSpace {
    fn available_bytes(&self, _root: &Path) -> Result<u64, SaveError> {
        Ok(self.0)
    }
}

pub trait SaveBackend: Send + Sync {
    fn load(&self, world: WorldId, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError>;
    fn commit(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        options: CommitOptions,
    ) -> Result<SaveCommit, SaveError>;
    fn capture(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        request: SnapshotMetaRequest,
        budget: SnapshotBudget,
    ) -> Result<SnapshotRecord, SaveError>;
    fn verify(&self, id: SnapshotId) -> Result<SnapshotVerification, SaveError>;
    fn info(&self, id: SnapshotId) -> Result<SnapshotRecord, SaveError>;
    fn restore(&self, id: SnapshotId, expected: Revision) -> Result<RestoreReceipt, SaveError>;
    fn scan(&self, world: WorldId) -> Result<Vec<SnapshotEntry>, SaveError>;
    fn remove(&self, id: SnapshotId) -> Result<(), SaveError>;
    fn available_bytes(&self) -> Result<u64, SaveError>;
}

pub struct FileBackend {
    root: PathBuf,
    live: PathBuf,
    backups: PathBuf,
    space: Arc<dyn SpaceProbe>,
    temp_counter: AtomicU64,
    capture_lock: Mutex<()>,
}

impl FileBackend {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SaveError> {
        Self::with_space(root, Arc::new(UnboundedSpace))
    }

    pub fn with_space(
        root: impl AsRef<Path>,
        space: Arc<dyn SpaceProbe>,
    ) -> Result<Self, SaveError> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root.as_ref())?;
        let live = ensure_subdir(&root, "live")?;
        let backups = ensure_subdir(&root, "backups")?;
        let backend = FileBackend {
            root,
            live,
            backups,
            space,
            temp_counter: AtomicU64::new(1),
            capture_lock: Mutex::new(()),
        };
        clean_temp_dir(&backend.live)?;
        clean_temp_dir(&backend.backups)?;
        Ok(backend)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn live_path(&self, world: WorldId, slot: SaveSlot) -> Result<PathBuf, SaveError> {
        let relative = PathBuf::from(format!("live/world-{}", hex16(&world.0)));
        let dir = confined_join(&self.root, &relative)?;
        Ok(dir.join(format!("slot-{:05}.img", slot.0)))
    }

    pub fn snapshot_dir(&self, id: SnapshotId) -> Option<PathBuf> {
        self.locate(id)
    }

    fn temp_path(&self, dir: &Path, prefix: &str) -> PathBuf {
        let counter = self.temp_counter.fetch_add(1, Ordering::SeqCst);
        dir.join(format!(
            ".tmp-{}-{}-{}",
            prefix,
            std::process::id(),
            counter
        ))
    }

    fn ensure_live_dir(&self, world: WorldId) -> Result<PathBuf, SaveError> {
        let dir = self.live.join(format!("world-{}", hex16(&world.0)));
        ensure_dir_no_symlink(&dir)?;
        Ok(dir)
    }

    fn locate(&self, id: SnapshotId) -> Option<PathBuf> {
        let target = hex16(&id.0);
        let worlds = fs::read_dir(&self.backups).ok()?;
        for world_entry in worlds.flatten() {
            if !world_entry.file_type().ok()?.is_dir() {
                continue;
            }
            let epochs = fs::read_dir(world_entry.path()).ok()?;
            for epoch_entry in epochs.flatten() {
                if !epoch_entry.file_type().ok()?.is_dir() {
                    continue;
                }
                let snapshots = fs::read_dir(epoch_entry.path()).ok()?;
                for snapshot_entry in snapshots.flatten() {
                    let name = snapshot_entry.file_name();
                    let name = name.to_string_lossy();
                    if name.ends_with(&target) {
                        return Some(snapshot_entry.path());
                    }
                }
            }
        }
        None
    }
}

fn ensure_subdir(root: &Path, name: &str) -> Result<PathBuf, SaveError> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(SaveError::rejected("configured directory is a symlink"));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(SaveError::rejected("configured path is not a directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&path)?;
        }
        Err(error) => return Err(error.into()),
    }
    let canonical = fs::canonicalize(&path)?;
    if !canonical.starts_with(root) {
        return Err(SaveError::rejected("configured directory escapes root"));
    }
    Ok(canonical)
}

fn clean_temp_dir(dir: &Path) -> Result<(), SaveError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".tmp-") {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

impl SaveBackend for FileBackend {
    fn load(&self, world: WorldId, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
        let path = self.live_path(world, slot)?;
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(SaveError::rejected("live save is a symlink"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let bytes = read_limited(&path, MAX_IMAGE_BYTES + 64 * 1024)?;
        SaveImage::decode(&bytes).map(Some)
    }

    fn commit(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        options: CommitOptions,
    ) -> Result<SaveCommit, SaveError> {
        image.validate()?;
        let encoded = image.encode()?;
        let digest = digest_bytes(&encoded);
        let dir = self.ensure_live_dir(world)?;
        let target = dir.join(format!("slot-{:05}.img", slot.0));

        let existing = match fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(SaveError::rejected("live save is a symlink"));
            }
            Ok(_) => Some(
                self.load(world, slot)?
                    .ok_or_else(|| SaveError::corrupt("live save vanished"))?,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let previous = existing.as_ref().map(|item| item.revision).unwrap_or(0);

        if options.mode == SaveMode::VerifyOnly {
            let live = existing.ok_or_else(|| SaveError::rejected("verify target missing"))?;
            if live.digest()? != digest {
                return Err(SaveError::corrupt("verify digest mismatch"));
            }
            return Ok(SaveCommit {
                slot,
                previous,
                current: previous,
                digest,
            });
        }
        if options.mode == SaveMode::FailIfExists && existing.is_some() {
            return Err(SaveError::rejected("target already exists"));
        }
        if image.revision < previous {
            return Err(SaveError::conflict("revision would decrease"));
        }

        let mode = file_mode(Some(&target), options.allow_world_readable);
        let temp = self.temp_path(&dir, "save");
        if let Err(error) = write_new_file(&temp, &encoded, mode) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        let on_disk = match fs::read(&temp) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error.into());
            }
        };
        if digest_bytes(&on_disk) != digest {
            let _ = fs::remove_file(&temp);
            return Err(SaveError::corrupt("write verification failed"));
        }
        fs::rename(&temp, &target)?;
        sync_dir(&dir)?;
        Ok(SaveCommit {
            slot,
            previous,
            current: image.revision,
            digest,
        })
    }

    fn capture(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        request: SnapshotMetaRequest,
        budget: SnapshotBudget,
    ) -> Result<SnapshotRecord, SaveError> {
        image.validate()?;
        let encoded = image.encode()?;
        let digest = digest_bytes(&encoded);
        let size = encoded.len() as u64;
        let part_total = image.total_len()?;

        let _sequence_guard = self
            .capture_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let entries = self.scan(world)?;
        let sequence = entries
            .iter()
            .map(|entry| entry.sequence)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| SaveError::overloaded("snapshot sequence exhausted"))?;
        let id = snapshot_id(world, request.epoch, sequence, &digest);

        let existing_total = entries
            .iter()
            .try_fold(0u64, |total, entry| total.checked_add(entry.size_bytes))
            .ok_or_else(|| SaveError::overloaded("backup size overflow"))?;
        let total = existing_total
            .checked_add(size)
            .ok_or_else(|| SaveError::overloaded("backup size overflow"))?;
        if total > budget.max_total_bytes {
            return Err(SaveError::overloaded("backup size budget exceeded"));
        }
        let available = self.space.available_bytes(&self.root)?;
        let required = size
            .checked_add(budget.min_free_bytes)
            .ok_or_else(|| SaveError::overloaded("free space requirement overflow"))?;
        if available < required {
            return Err(SaveError::overloaded("free space below threshold"));
        }

        let parts: Vec<PartEntry> = image
            .parts
            .iter()
            .map(|(name, body)| PartEntry {
                name: name.clone(),
                size: body.len() as u64,
                digest: digest_bytes(body),
            })
            .collect();
        let manifest = Manifest {
            version: CURRENT_MANIFEST_VERSION,
            baseline: request.baseline,
            epoch: request.epoch,
            sequence,
            captured_at: request.captured_at,
            world,
            slot,
            format: image.format,
            schema: image.schema,
            revision: image.revision,
            id,
            image_digest: digest,
            total_bytes: part_total,
            parts,
        };
        let manifest_bytes = manifest.encode()?;

        let world_dir = self.backups.join(format!("world-{}", hex16(&world.0)));
        ensure_dir_no_symlink(&world_dir)?;
        let epoch_dir = world_dir.join(format!("epoch-{:020}", request.epoch));
        ensure_dir_no_symlink(&epoch_dir)?;
        let final_dir = epoch_dir.join(format!("snap-{:020}-{}", sequence, hex16(&id.0)));
        if fs::symlink_metadata(&final_dir).is_ok() {
            return Err(SaveError::conflict("snapshot id collision"));
        }

        let live = self.live_path(world, slot)?;
        let mode = file_mode(Some(&live), budget.allow_world_readable);
        let temp = self.temp_path(&epoch_dir, "snap");
        fs::create_dir(&temp)?;
        let result = (|| -> Result<(), SaveError> {
            let image_path = temp.join(IMAGE_FILE);
            let manifest_path = temp.join(MANIFEST_FILE);
            write_new_file(&image_path, &encoded, mode)?;
            write_new_file(&manifest_path, &manifest_bytes, mode)?;
            let stored_image = read_limited(&image_path, MAX_IMAGE_BYTES + 64 * 1024)?;
            if digest_bytes(&stored_image) != digest {
                return Err(SaveError::corrupt("snapshot write verification failed"));
            }
            let stored_manifest = read_limited(&manifest_path, MAX_MANIFEST_BYTES)?;
            let decoded = Manifest::decode(&stored_manifest)?;
            if decoded.id != id || decoded.image_digest != digest {
                return Err(SaveError::corrupt("snapshot manifest verification failed"));
            }
            sync_dir(&temp)?;
            sync_dir(&epoch_dir)?;
            fs::rename(&temp, &final_dir)?;
            sync_dir(&epoch_dir)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }

        Ok(SnapshotRecord {
            snapshot: SaveSnapshot {
                id,
                world,
                slot,
                format: image.format,
                schema: image.schema,
                revision: image.revision,
                captured_at: request.captured_at,
                digest,
                body: Arc::from(encoded),
            },
            baseline: request.baseline,
            epoch: request.epoch,
            sequence,
        })
    }

    fn verify(&self, id: SnapshotId) -> Result<SnapshotVerification, SaveError> {
        let Some(dir) = self.locate(id) else {
            return Err(SaveError::corrupt("snapshot not found"));
        };
        let manifest_path = dir.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return Ok(invalid_verification("manifest missing"));
        }
        let manifest_bytes = match read_limited(&manifest_path, MAX_MANIFEST_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::UnsupportedVersion => return Err(error),
            Err(_) => return Ok(invalid_verification("manifest unreadable")),
        };
        let manifest = match Manifest::decode(&manifest_bytes) {
            Ok(manifest) => manifest,
            Err(error) if error.kind() == ErrorKind::UnsupportedVersion => return Err(error),
            Err(_) => return Ok(invalid_verification("manifest invalid")),
        };
        if manifest.id != id {
            return Ok(invalid_verification("manifest id mismatch"));
        }
        let image_path = dir.join(IMAGE_FILE);
        let image_bytes = match read_limited(&image_path, MAX_IMAGE_BYTES + 64 * 1024) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(invalid_verification("image missing or unreadable")),
        };
        let digest_matches = digest_bytes(&image_bytes) == manifest.image_digest;
        if !digest_matches {
            return Ok(SnapshotVerification {
                valid: false,
                compatibility: Compatibility::Unsupported,
                digest_matches: false,
                warnings: vec!["image digest mismatch".to_string()],
            });
        }
        let image = match SaveImage::decode(&image_bytes) {
            Ok(image) => image,
            Err(_) => return Ok(invalid_verification("image invalid")),
        };
        if image.format != manifest.format
            || image.schema != manifest.schema
            || image.revision != manifest.revision
            || image.total_len()? != manifest.total_bytes
            || image.parts.len() != manifest.parts.len()
        {
            return Ok(invalid_verification("image metadata mismatch"));
        }
        for part in &manifest.parts {
            let Some(body) = image.parts.get(&part.name) else {
                return Ok(invalid_verification("manifest part missing"));
            };
            if body.len() as u64 != part.size || digest_bytes(body) != part.digest {
                return Ok(invalid_verification("part digest mismatch"));
            }
        }
        Ok(SnapshotVerification {
            valid: true,
            compatibility: Compatibility::Writable,
            digest_matches: true,
            warnings: Vec::new(),
        })
    }

    fn info(&self, id: SnapshotId) -> Result<SnapshotRecord, SaveError> {
        let dir = self
            .locate(id)
            .ok_or_else(|| SaveError::corrupt("snapshot not found"))?;
        let manifest_bytes = read_limited(&dir.join(MANIFEST_FILE), MAX_MANIFEST_BYTES)?;
        let manifest = Manifest::decode(&manifest_bytes)?;
        if manifest.id != id {
            return Err(SaveError::corrupt("snapshot identity mismatch"));
        }
        let image_bytes = read_limited(&dir.join(IMAGE_FILE), MAX_IMAGE_BYTES + 64 * 1024)?;
        let image = SaveImage::decode(&image_bytes)?;
        if image.format != manifest.format
            || image.revision != manifest.revision
            || image.schema != manifest.schema
        {
            return Err(SaveError::corrupt("snapshot body mismatch"));
        }
        Ok(SnapshotRecord {
            snapshot: SaveSnapshot {
                id,
                world: manifest.world,
                slot: manifest.slot,
                format: manifest.format,
                schema: manifest.schema,
                revision: manifest.revision,
                captured_at: manifest.captured_at,
                digest: manifest.image_digest,
                body: Arc::from(image_bytes),
            },
            baseline: manifest.baseline,
            epoch: manifest.epoch,
            sequence: manifest.sequence,
        })
    }

    fn restore(&self, id: SnapshotId, expected: Revision) -> Result<RestoreReceipt, SaveError> {
        let record = self.info(id)?;
        let verification = self.verify(id)?;
        if !verification.valid {
            return Err(SaveError::corrupt("snapshot failed verification"));
        }
        let world = record.snapshot.world;
        let slot = record.snapshot.slot;
        let live = self.load(world, slot)?;
        let previous = live.as_ref().map(|image| image.revision).unwrap_or(0);
        if previous != expected {
            return Err(SaveError::conflict("stale live revision"));
        }
        let mut image = SaveImage::decode(&record.snapshot.body)?;
        let original = image.clone();
        image.revision = previous
            .checked_add(1)
            .ok_or_else(|| SaveError::conflict("revision exhausted"))?;
        let commit = self.commit(
            world,
            slot,
            &image,
            CommitOptions {
                mode: SaveMode::Replace,
                allow_world_readable: false,
            },
        )?;
        let restored = self
            .load(world, slot)?
            .ok_or_else(|| SaveError::corrupt("restored save missing"))?;
        if restored.format != original.format
            || restored.schema != original.schema
            || restored.parts != original.parts
        {
            return Err(SaveError::corrupt("restored state failed verification"));
        }
        let warnings = Vec::new();
        Ok(RestoreReceipt {
            previous,
            current: commit.current,
            warnings,
        })
    }

    fn scan(&self, world: WorldId) -> Result<Vec<SnapshotEntry>, SaveError> {
        let world_dir = self.backups.join(format!("world-{}", hex16(&world.0)));
        let mut entries = Vec::new();
        let epochs = match fs::read_dir(&world_dir) {
            Ok(epochs) => epochs,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(entries),
            Err(error) => return Err(error.into()),
        };
        for epoch_entry in epochs {
            let epoch_entry = epoch_entry?;
            if !epoch_entry.file_type()?.is_dir() {
                continue;
            }
            let epoch_name = epoch_entry.file_name();
            let epoch_name = epoch_name.to_string_lossy();
            let epoch_from_dir = epoch_name
                .strip_prefix("epoch-")
                .and_then(|text| text.parse::<u64>().ok());
            for snapshot_entry in fs::read_dir(epoch_entry.path())? {
                let snapshot_entry = snapshot_entry?;
                if !snapshot_entry.file_type()?.is_dir() {
                    continue;
                }
                let name = snapshot_entry.file_name();
                let name = name.to_string_lossy();
                let Some(rest) = name.strip_prefix("snap-") else {
                    continue;
                };
                let Some((sequence_text, id_text)) = rest.split_once('-') else {
                    continue;
                };
                let Some(sequence) = sequence_text.parse::<u64>().ok() else {
                    continue;
                };
                let Some(id_bytes) = parse_hex16(id_text) else {
                    continue;
                };
                let id = SnapshotId(id_bytes);
                let dir = snapshot_entry.path();
                let manifest = read_limited(&dir.join(MANIFEST_FILE), MAX_MANIFEST_BYTES)
                    .ok()
                    .and_then(|bytes| Manifest::decode(bytes.as_slice()).ok());
                let (epoch, baseline, captured_at) = match &manifest {
                    Some(manifest) => (manifest.epoch, manifest.baseline, manifest.captured_at),
                    None => (epoch_from_dir.unwrap_or(0), false, Timestamp(0)),
                };
                let image_present = fs::symlink_metadata(dir.join(IMAGE_FILE))
                    .map(|meta| meta.is_file() && !meta.file_type().is_symlink())
                    .unwrap_or(false);
                entries.push(SnapshotEntry {
                    id,
                    epoch,
                    sequence,
                    baseline,
                    captured_at,
                    size_bytes: dir_size(&dir),
                    valid: manifest.is_some() && image_present,
                });
            }
        }
        entries.sort_by_key(|entry| (entry.sequence, entry.id.0));
        Ok(entries)
    }

    fn remove(&self, id: SnapshotId) -> Result<(), SaveError> {
        let Some(dir) = self.locate(id) else {
            return Ok(());
        };
        let parent = match dir.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Err(SaveError::corrupt("snapshot path has no parent")),
        };
        fs::remove_dir_all(&dir)?;
        sync_dir(&parent)?;
        Ok(())
    }

    fn available_bytes(&self) -> Result<u64, SaveError> {
        self.space.available_bytes(&self.root)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveTrigger {
    Manual,
    Autosave,
    ChunkUnload,
    Shutdown,
    Explicit,
    Rollback,
}

#[derive(Clone, Debug)]
pub struct SaveRequest {
    pub world: WorldId,
    pub epoch: u64,
    pub trigger: SaveTrigger,
    pub slot: SaveSlot,
    pub revision: Revision,
    pub bytes: u64,
    pub correlation: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterceptDecision {
    Proceed,
    Skip,
    Redirect(SaveSlot),
}

pub trait SaveInterceptor: Send + Sync {
    fn name(&self) -> &str;
    fn before_save(&self, request: &SaveRequest) -> Result<InterceptDecision, SaveError>;
    fn after_save(
        &self,
        request: &SaveRequest,
        receipt: &SaveCommit,
    ) -> Result<SnapshotVerification, SaveError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InterceptorStatus {
    NotRun,
    Applied { interceptor: String },
    SkippedBy(String),
    Fallback { interceptor: String, error: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveResult {
    Committed,
    Skipped,
    Failed,
}

#[derive(Clone, Debug)]
pub struct SaveRecord {
    pub at: Timestamp,
    pub monotonic: u64,
    pub epoch: u64,
    pub world: WorldId,
    pub trigger: SaveTrigger,
    pub requested_slot: SaveSlot,
    pub destination: String,
    pub outcome: SaveResult,
    pub bytes: u64,
    pub digest: Digest,
    pub interceptor: InterceptorStatus,
    pub verification: Option<SnapshotVerification>,
    pub warnings: Vec<String>,
    pub reason: Option<String>,
    pub duration_us: u64,
    pub correlation: [u8; 16],
}

#[derive(Clone, Debug)]
pub struct SaveAttempt {
    pub record: SaveRecord,
    pub commit: Option<SaveCommit>,
    pub snapshot: Option<SnapshotRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotOutcome {
    Created,
    Failed,
}

#[derive(Clone, Debug)]
pub struct SnapshotAudit {
    pub at: Timestamp,
    pub monotonic: u64,
    pub world: WorldId,
    pub id: Option<SnapshotId>,
    pub epoch: u64,
    pub baseline: bool,
    pub cause: SnapshotCause,
    pub outcome: SnapshotOutcome,
    pub bytes: u64,
    pub reason: Option<String>,
    pub duration_us: u64,
}

#[derive(Clone, Debug)]
pub struct PruneAudit {
    pub at: Timestamp,
    pub monotonic: u64,
    pub world: WorldId,
    pub removed: Vec<SnapshotId>,
    pub kept: usize,
    pub total_bytes: u64,
    pub over_budget: bool,
    pub error: Option<String>,
    pub duration_us: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollbackTrigger {
    Manual,
    Automatic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollbackOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug)]
pub struct RollbackAudit {
    pub at: Timestamp,
    pub monotonic: u64,
    pub world: WorldId,
    pub source: SnapshotId,
    pub trigger: RollbackTrigger,
    pub reason: String,
    pub before_digest: Option<Digest>,
    pub after_digest: Option<Digest>,
    pub outcome: RollbackOutcome,
    pub error: Option<String>,
    pub epoch_after: Option<u64>,
    pub duration_us: u64,
}

#[derive(Clone, Debug)]
pub enum AuditRecord {
    Save(SaveRecord),
    Snapshot(SnapshotAudit),
    Prune(PruneAudit),
    Rollback(RollbackAudit),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counters {
    pub saves_attempted: u64,
    pub saves_committed: u64,
    pub saves_failed: u64,
    pub saves_skipped: u64,
    pub snapshots_created: u64,
    pub snapshots_pruned: u64,
    pub verify_failures: u64,
    pub rollbacks_triggered: u64,
    pub rollbacks_failed: u64,
}

const MAX_AUDIT_RECORDS: usize = 256;
static NEXT_AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn next_audit_sequence() -> u64 {
    NEXT_AUDIT_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Default)]
struct AuditState {
    records: VecDeque<AuditRecord>,
    counters: Counters,
}

pub struct AuditLog {
    inner: Mutex<AuditState>,
}

impl AuditLog {
    fn new() -> Self {
        AuditLog {
            inner: Mutex::new(AuditState::default()),
        }
    }

    fn push(&self, record: AuditRecord) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match &record {
            AuditRecord::Save(save) => match save.outcome {
                SaveResult::Committed => state.counters.saves_committed += 1,
                SaveResult::Skipped => state.counters.saves_skipped += 1,
                SaveResult::Failed => state.counters.saves_failed += 1,
            },
            AuditRecord::Snapshot(snapshot) => {
                if snapshot.outcome == SnapshotOutcome::Created {
                    state.counters.snapshots_created += 1;
                }
            }
            AuditRecord::Prune(prune) => {
                state.counters.snapshots_pruned += prune.removed.len() as u64;
            }
            AuditRecord::Rollback(rollback) => match rollback.outcome {
                RollbackOutcome::Succeeded => state.counters.rollbacks_triggered += 1,
                RollbackOutcome::Failed => state.counters.rollbacks_failed += 1,
            },
        }
        if state.records.len() >= MAX_AUDIT_RECORDS {
            state.records.pop_front();
        }
        state.records.push_back(record);
    }

    fn note_attempt(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.counters.saves_attempted += 1;
    }

    fn note_verify_failure(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.counters.verify_failures += 1;
    }

    pub fn counters(&self) -> Counters {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .counters
    }

    pub fn records(&self) -> Vec<AuditRecord> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .records
            .iter()
            .cloned()
            .collect()
    }
}

pub trait StateReset: Send + Sync {
    fn reset_after_rollback(&self, world: WorldId, epoch: u64);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollbackNotifyPolicy {
    Pause,
    Disconnect,
    Resynchronize,
}

pub trait PeerNotifier: Send + Sync {
    fn notify_rollback(
        &self,
        world: WorldId,
        epoch: u64,
        policy: RollbackNotifyPolicy,
        reason: &str,
    ) -> Result<(), SaveError>;
}

#[derive(Clone, Debug)]
pub struct SaveHookConfig {
    pub rolling_snapshots: usize,
    pub max_total_backup_bytes: u64,
    pub min_free_bytes: u64,
    pub allow_world_readable: bool,
    pub auto_rollback: bool,
    pub min_rollback_interval: Duration,
    pub notify_policy: RollbackNotifyPolicy,
}

impl Default for SaveHookConfig {
    fn default() -> Self {
        SaveHookConfig {
            rolling_snapshots: 3,
            max_total_backup_bytes: 512 * 1024 * 1024,
            min_free_bytes: 64 * 1024 * 1024,
            allow_world_readable: false,
            auto_rollback: false,
            min_rollback_interval: Duration::from_secs(60),
            notify_policy: RollbackNotifyPolicy::Pause,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RollbackReceipt {
    pub previous: Revision,
    pub current: Revision,
    pub epoch: u64,
    pub pre_rollback: Option<SnapshotId>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PruneReport {
    pub world: WorldId,
    pub removed: Vec<SnapshotId>,
    pub kept: usize,
    pub total_bytes: u64,
    pub over_budget: bool,
}

struct WorldState {
    epoch: u64,
    baseline_done: bool,
    last_auto_rollback: Option<Instant>,
}

enum SaveFlow {
    Committed(Option<SaveCommit>, Option<SnapshotRecord>),
    Skipped,
}

thread_local! {
    static ACTIVE_WORLDS: RefCell<HashSet<WorldId>> = RefCell::new(HashSet::new());
}

struct ReentryGuard {
    world: WorldId,
}

impl ReentryGuard {
    fn enter(world: WorldId) -> Option<Self> {
        ACTIVE_WORLDS.with(|active| {
            let mut active = active.borrow_mut();
            if active.contains(&world) {
                None
            } else {
                active.insert(world);
                Some(ReentryGuard { world })
            }
        })
    }
}

impl Drop for ReentryGuard {
    fn drop(&mut self) {
        ACTIVE_WORLDS.with(|active| {
            active.borrow_mut().remove(&self.world);
        });
    }
}

fn correlation_id(
    world: WorldId,
    epoch: u64,
    trigger: SaveTrigger,
    revision: Revision,
) -> [u8; 16] {
    let mut hash = Sha256::new();
    hash.update(b"save-correlation");
    hash.update(&world.0);
    hash.update(&epoch.to_be_bytes());
    hash.update(&[trigger as u8]);
    hash.update(&revision.to_be_bytes());
    hash.update(&next_audit_sequence().to_be_bytes());
    let full = hash.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&full.0[..16]);
    out
}

pub struct SaveHooks {
    backend: Arc<dyn SaveBackend>,
    config: SaveHookConfig,
    interceptors: Vec<Arc<dyn SaveInterceptor>>,
    resetters: Vec<Arc<dyn StateReset>>,
    notifier: Option<Arc<dyn PeerNotifier>>,
    audit: AuditLog,
    world_locks: Mutex<BTreeMap<WorldId, Arc<Mutex<()>>>>,
    world_states: Mutex<BTreeMap<WorldId, WorldState>>,
    next_epoch: AtomicU64,
}

impl SaveHooks {
    pub fn new(backend: Arc<dyn SaveBackend>, mut config: SaveHookConfig) -> Self {
        if config.rolling_snapshots == 0 {
            config.rolling_snapshots = 1;
        }
        SaveHooks {
            backend,
            config,
            interceptors: Vec::new(),
            resetters: Vec::new(),
            notifier: None,
            audit: AuditLog::new(),
            world_locks: Mutex::new(BTreeMap::new()),
            world_states: Mutex::new(BTreeMap::new()),
            next_epoch: AtomicU64::new(1),
        }
    }

    pub fn with_interceptor(mut self, interceptor: Arc<dyn SaveInterceptor>) -> Self {
        self.interceptors.push(interceptor);
        self
    }

    pub fn with_resetter(mut self, resetter: Arc<dyn StateReset>) -> Self {
        self.resetters.push(resetter);
        self
    }

    pub fn with_notifier(mut self, notifier: Arc<dyn PeerNotifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub fn backend(&self) -> &Arc<dyn SaveBackend> {
        &self.backend
    }

    pub fn config(&self) -> &SaveHookConfig {
        &self.config
    }

    pub fn counters(&self) -> Counters {
        self.audit.counters()
    }

    pub fn audit_records(&self) -> Vec<AuditRecord> {
        self.audit.records()
    }

    fn world_lock(&self, world: WorldId) -> Arc<Mutex<()>> {
        let mut locks = self
            .world_locks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        locks
            .entry(world)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn lock_world(&self, world: WorldId) -> Arc<Mutex<()>> {
        self.world_lock(world)
    }

    fn ensure_epoch(&self, world: WorldId) -> u64 {
        let mut states = self
            .world_states
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let state = states.entry(world).or_insert(WorldState {
            epoch: 0,
            baseline_done: false,
            last_auto_rollback: None,
        });
        if state.epoch == 0 {
            state.epoch = self.next_epoch.fetch_add(1, Ordering::SeqCst);
            state.baseline_done = false;
        }
        state.epoch
    }

    pub fn begin_session(&self, world: WorldId) -> u64 {
        let mut states = self
            .world_states
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let state = states.entry(world).or_insert(WorldState {
            epoch: 0,
            baseline_done: false,
            last_auto_rollback: None,
        });
        state.epoch = self.next_epoch.fetch_add(1, Ordering::SeqCst);
        state.baseline_done = false;
        state.epoch
    }

    pub fn session_epoch(&self, world: WorldId) -> Option<u64> {
        let states = self
            .world_states
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        states
            .get(&world)
            .map(|state| state.epoch)
            .filter(|epoch| *epoch != 0)
    }

    pub fn load(&self, world: WorldId, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
        self.backend.load(world, slot)
    }

    pub fn snapshots(&self, world: WorldId) -> Result<Vec<SnapshotEntry>, SaveError> {
        self.backend.scan(world)
    }

    pub fn snapshot_info(&self, id: SnapshotId) -> Result<SnapshotRecord, SaveError> {
        self.backend.info(id)
    }

    pub fn verify(&self, id: SnapshotId) -> Result<SnapshotVerification, SaveError> {
        let verification = self.backend.verify(id)?;
        if !verification.valid {
            self.audit.note_verify_failure();
        }
        Ok(verification)
    }

    pub fn save(
        &self,
        world: WorldId,
        target: SaveTarget,
        image: SaveImage,
        trigger: SaveTrigger,
    ) -> Result<SaveAttempt, SaveError> {
        let started = Instant::now();
        let epoch_hint = self.session_epoch(world).unwrap_or(0);
        let correlation = correlation_id(world, epoch_hint, trigger, image.revision);
        let mut record = SaveRecord {
            at: Timestamp::now(),
            monotonic: next_audit_sequence(),
            epoch: epoch_hint,
            world,
            trigger,
            requested_slot: target.slot,
            destination: slot_label(target.slot),
            outcome: SaveResult::Failed,
            bytes: image.total_len().unwrap_or(0),
            digest: Digest::ZERO,
            interceptor: InterceptorStatus::NotRun,
            verification: None,
            warnings: Vec::new(),
            reason: Some("not started".to_string()),
            duration_us: 0,
            correlation,
        };
        self.audit.note_attempt();
        let inner = self.save_inner(world, target, image, trigger, &mut record);
        record.duration_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        match inner {
            Ok(SaveFlow::Committed(commit, snapshot)) => {
                record.outcome = SaveResult::Committed;
                record.reason = None;
                self.audit.push(AuditRecord::Save(record.clone()));
                Ok(SaveAttempt {
                    record,
                    commit,
                    snapshot,
                })
            }
            Ok(SaveFlow::Skipped) => {
                record.outcome = SaveResult::Skipped;
                self.audit.push(AuditRecord::Save(record.clone()));
                Ok(SaveAttempt {
                    record,
                    commit: None,
                    snapshot: None,
                })
            }
            Err(error) => {
                record.outcome = SaveResult::Failed;
                record.reason = Some(sanitize(&error.to_string()));
                self.audit.push(AuditRecord::Save(record));
                Err(error)
            }
        }
    }

    fn save_inner(
        &self,
        world: WorldId,
        target: SaveTarget,
        image: SaveImage,
        trigger: SaveTrigger,
        record: &mut SaveRecord,
    ) -> Result<SaveFlow, SaveError> {
        let _reentry = match ReentryGuard::enter(world) {
            Some(guard) => guard,
            None => {
                record.reason = Some("reentrant save skipped".to_string());
                return Ok(SaveFlow::Skipped);
            }
        };
        let lock = self.lock_world(world);
        let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let epoch = self.ensure_epoch(world);
        record.epoch = epoch;
        image.validate()?;
        record.bytes = image.total_len()?;

        let needs_baseline = {
            let states = self
                .world_states
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            states
                .get(&world)
                .map(|state| !state.baseline_done)
                .unwrap_or(true)
        };
        if needs_baseline {
            let baseline_image = match self.backend.load(world, target.slot)? {
                Some(live) => live,
                None => SaveImage::empty(image.format, image.schema, 0),
            };
            match self.capture_with_retention(
                world,
                target.slot,
                &baseline_image,
                epoch,
                true,
                SnapshotCause::Baseline,
            ) {
                Ok(_) => {
                    let mut states = self
                        .world_states
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    if let Some(state) = states.get_mut(&world) {
                        state.baseline_done = true;
                    }
                }
                Err(error) => {
                    return Err(SaveError::new(
                        error.kind(),
                        format!("baseline snapshot failed: {}", error),
                    ));
                }
            }
        }

        let request = SaveRequest {
            world,
            epoch,
            trigger,
            slot: target.slot,
            revision: image.revision,
            bytes: record.bytes,
            correlation: record.correlation,
        };
        let mut destination = target.slot;
        let mut interceptor = InterceptorStatus::NotRun;
        for candidate in &self.interceptors {
            match candidate.before_save(&request) {
                Ok(InterceptDecision::Proceed) => {
                    if interceptor == InterceptorStatus::NotRun {
                        interceptor = InterceptorStatus::Applied {
                            interceptor: candidate.name().to_string(),
                        };
                    }
                }
                Ok(InterceptDecision::Skip) => {
                    interceptor = InterceptorStatus::SkippedBy(candidate.name().to_string());
                    record.interceptor = interceptor;
                    record.reason = Some("skipped by interceptor".to_string());
                    return Ok(SaveFlow::Skipped);
                }
                Ok(InterceptDecision::Redirect(slot)) => {
                    destination = slot;
                    interceptor = InterceptorStatus::Applied {
                        interceptor: candidate.name().to_string(),
                    };
                }
                Err(error) => {
                    interceptor = InterceptorStatus::Fallback {
                        interceptor: candidate.name().to_string(),
                        error: sanitize(&error.to_string()),
                    };
                    break;
                }
            }
        }
        record.interceptor = interceptor;
        record.destination = slot_label(destination);

        let commit = self.backend.commit(
            world,
            destination,
            &image,
            CommitOptions {
                mode: target.mode,
                allow_world_readable: self.config.allow_world_readable,
            },
        )?;
        record.digest = commit.digest;

        let mut verification = if target.mode == SaveMode::VerifyOnly {
            match self.backend.load(world, destination)? {
                Some(live) => {
                    let matches = live.digest()? == commit.digest;
                    SnapshotVerification {
                        valid: matches,
                        compatibility: if matches {
                            Compatibility::Writable
                        } else {
                            Compatibility::Unsupported
                        },
                        digest_matches: matches,
                        warnings: if matches {
                            Vec::new()
                        } else {
                            vec!["verify-only digest mismatch".to_string()]
                        },
                    }
                }
                None => invalid_verification("verify target missing"),
            }
        } else {
            match self.backend.load(world, destination)? {
                Some(live) => {
                    let matches = live.digest()? == commit.digest;
                    SnapshotVerification {
                        valid: matches,
                        compatibility: if matches {
                            Compatibility::Writable
                        } else {
                            Compatibility::Unsupported
                        },
                        digest_matches: matches,
                        warnings: if matches {
                            Vec::new()
                        } else {
                            vec!["committed digest mismatch".to_string()]
                        },
                    }
                }
                None => invalid_verification("committed save missing"),
            }
        };

        for candidate in &self.interceptors {
            match candidate.after_save(&request, &commit) {
                Ok(extra) => {
                    if !extra.valid {
                        verification.valid = false;
                        verification.digest_matches &= extra.digest_matches;
                    }
                    for warning in extra.warnings {
                        verification.warnings.push(sanitize(&warning));
                    }
                }
                Err(error) => {
                    record.warnings.push(format!(
                        "post-save interceptor failed: {}",
                        sanitize(&error.to_string())
                    ));
                }
            }
        }
        record.verification = Some(verification.clone());
        if !verification.valid {
            return Err(SaveError::corrupt("post-save verification failed"));
        }

        let mut snapshot = None;
        if target.mode != SaveMode::VerifyOnly {
            match self.capture_with_retention(
                world,
                destination,
                &image,
                epoch,
                false,
                SnapshotCause::Rolling,
            ) {
                Ok(captured) => snapshot = Some(captured),
                Err(error) => record.warnings.push(format!(
                    "rolling snapshot failed: {}",
                    sanitize(&error.to_string())
                )),
            }
            if let Err(error) = self.prune_locked(world, epoch) {
                record.warnings.push(format!(
                    "retention failed: {}",
                    sanitize(&error.to_string())
                ));
            }
        }

        Ok(SaveFlow::Committed(Some(commit), snapshot))
    }

    fn capture_locked(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        epoch: u64,
        baseline: bool,
        cause: SnapshotCause,
    ) -> Result<SnapshotRecord, SaveError> {
        let started = Instant::now();
        let request = SnapshotMetaRequest {
            epoch,
            baseline,
            cause,
            captured_at: Timestamp::now(),
        };
        let budget = SnapshotBudget {
            max_total_bytes: self.config.max_total_backup_bytes,
            min_free_bytes: self.config.min_free_bytes,
            allow_world_readable: self.config.allow_world_readable,
        };
        let result = self.backend.capture(world, slot, image, request, budget);
        let duration_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let audit = match &result {
            Ok(record) => SnapshotAudit {
                at: Timestamp::now(),
                monotonic: next_audit_sequence(),
                world,
                id: Some(record.snapshot.id),
                epoch,
                baseline,
                cause,
                outcome: SnapshotOutcome::Created,
                bytes: record.snapshot.body.len() as u64,
                reason: None,
                duration_us,
            },
            Err(error) => SnapshotAudit {
                at: Timestamp::now(),
                monotonic: next_audit_sequence(),
                world,
                id: None,
                epoch,
                baseline,
                cause,
                outcome: SnapshotOutcome::Failed,
                bytes: image.total_len().unwrap_or(0),
                reason: Some(sanitize(&error.to_string())),
                duration_us,
            },
        };
        self.audit.push(AuditRecord::Snapshot(audit));
        result
    }

    fn capture_with_retention(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        epoch: u64,
        baseline: bool,
        cause: SnapshotCause,
    ) -> Result<SnapshotRecord, SaveError> {
        let _ = self.prune_locked(world, epoch);
        match self.capture_locked(world, slot, image, epoch, baseline, cause) {
            Err(error) if error.kind() == ErrorKind::Overloaded => {
                let _ = self.prune_locked(world, epoch);
                self.capture_locked(world, slot, image, epoch, baseline, cause)
            }
            result => result,
        }
    }

    pub fn capture(
        &self,
        world: WorldId,
        slot: SaveSlot,
        cause: SnapshotCause,
    ) -> Result<SnapshotRecord, SaveError> {
        let _reentry = ReentryGuard::enter(world)
            .ok_or_else(|| SaveError::conflict("reentrant snapshot rejected"))?;
        let lock = self.lock_world(world);
        let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let epoch = self.ensure_epoch(world);
        let image = self
            .backend
            .load(world, slot)?
            .ok_or_else(|| SaveError::rejected("no live save to capture"))?;
        self.capture_with_retention(world, slot, &image, epoch, false, cause)
    }

    pub fn prune(&self, world: WorldId) -> Result<PruneReport, SaveError> {
        let _reentry = ReentryGuard::enter(world)
            .ok_or_else(|| SaveError::conflict("reentrant prune rejected"))?;
        let lock = self.lock_world(world);
        let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let epoch = self.session_epoch(world).unwrap_or(0);
        self.prune_locked(world, epoch)
    }

    fn prune_locked(&self, world: WorldId, current_epoch: u64) -> Result<PruneReport, SaveError> {
        let started = Instant::now();
        let mut audit = PruneAudit {
            at: Timestamp::now(),
            monotonic: next_audit_sequence(),
            world,
            removed: Vec::new(),
            kept: 0,
            total_bytes: 0,
            over_budget: false,
            error: None,
            duration_us: 0,
        };
        let result = self.prune_inner(world, current_epoch, &mut audit);
        audit.duration_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        if let Err(error) = &result {
            audit.error = Some(sanitize(&error.to_string()));
        }
        self.audit.push(AuditRecord::Prune(audit));
        result
    }

    fn prune_inner(
        &self,
        world: WorldId,
        current_epoch: u64,
        audit: &mut PruneAudit,
    ) -> Result<PruneReport, SaveError> {
        let entries = self.backend.scan(world)?;
        let mut valid = Vec::new();
        let mut removed = Vec::new();
        for entry in entries {
            match self.backend.verify(entry.id) {
                Ok(verification) if verification.valid => valid.push(entry),
                Ok(_) => {
                    self.backend.remove(entry.id)?;
                    removed.push(entry.id);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::Corrupt | ErrorKind::UnsupportedVersion
                    ) =>
                {
                    self.backend.remove(entry.id)?;
                    removed.push(entry.id);
                }
                Err(_) => valid.push(entry),
            }
        }
        valid.sort_by_key(|entry| (entry.sequence, entry.id.0));

        let newest = valid.last().map(|entry| entry.id);
        let baseline = valid
            .iter()
            .filter(|entry| entry.baseline && entry.epoch == current_epoch)
            .max_by_key(|entry| entry.sequence)
            .map(|entry| entry.id)
            .or_else(|| {
                valid
                    .iter()
                    .filter(|entry| entry.baseline)
                    .max_by_key(|entry| entry.sequence)
                    .map(|entry| entry.id)
            });

        let mut keep = HashSet::new();
        for entry in valid
            .iter()
            .rev()
            .take(self.config.rolling_snapshots.max(1))
        {
            keep.insert(entry.id);
        }
        if let Some(id) = baseline {
            keep.insert(id);
        }
        if let Some(id) = newest {
            keep.insert(id);
        }

        for entry in &valid {
            if !keep.contains(&entry.id) {
                self.backend.remove(entry.id)?;
                removed.push(entry.id);
            }
        }

        let protected: HashSet<SnapshotId> = [baseline, newest].into_iter().flatten().collect();
        let mut remaining: Vec<SnapshotEntry> = valid
            .iter()
            .filter(|entry| !removed.contains(&entry.id))
            .cloned()
            .collect();
        let mut total = remaining
            .iter()
            .try_fold(0u64, |sum, entry| sum.checked_add(entry.size_bytes))
            .unwrap_or(u64::MAX);
        let mut index = 0;
        while total > self.config.max_total_backup_bytes && index < remaining.len() {
            let entry = remaining[index].clone();
            if protected.contains(&entry.id) {
                index += 1;
                continue;
            }
            self.backend.remove(entry.id)?;
            total = total.saturating_sub(entry.size_bytes);
            removed.push(entry.id);
            remaining.remove(index);
        }

        let report = PruneReport {
            world,
            removed: removed.clone(),
            kept: remaining.len(),
            total_bytes: total,
            over_budget: total > self.config.max_total_backup_bytes,
        };
        audit.removed = removed;
        audit.kept = report.kept;
        audit.total_bytes = report.total_bytes;
        audit.over_budget = report.over_budget;
        Ok(report)
    }

    pub fn rollback(
        &self,
        world: WorldId,
        source: SnapshotId,
        expected: Revision,
        reason: &str,
        trigger: RollbackTrigger,
    ) -> Result<RollbackReceipt, SaveError> {
        let started = Instant::now();
        let mut audit = RollbackAudit {
            at: Timestamp::now(),
            monotonic: next_audit_sequence(),
            world,
            source,
            trigger,
            reason: sanitize(reason),
            before_digest: None,
            after_digest: None,
            outcome: RollbackOutcome::Failed,
            error: None,
            epoch_after: None,
            duration_us: 0,
        };
        let result = self.rollback_inner(world, source, expected, reason, trigger, &mut audit);
        audit.duration_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        match &result {
            Ok(receipt) => {
                audit.outcome = RollbackOutcome::Succeeded;
                audit.epoch_after = Some(receipt.epoch);
            }
            Err(error) => {
                audit.error = Some(sanitize(&error.to_string()));
            }
        }
        self.audit.push(AuditRecord::Rollback(audit));
        result
    }

    fn rollback_inner(
        &self,
        world: WorldId,
        source: SnapshotId,
        expected: Revision,
        reason: &str,
        trigger: RollbackTrigger,
        audit: &mut RollbackAudit,
    ) -> Result<RollbackReceipt, SaveError> {
        let _reentry = ReentryGuard::enter(world)
            .ok_or_else(|| SaveError::conflict("reentrant rollback rejected"))?;
        let lock = self.lock_world(world);
        let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());

        if trigger == RollbackTrigger::Automatic {
            if !self.config.auto_rollback {
                return Err(SaveError::rejected("automatic rollback is disabled"));
            }
            let states = self
                .world_states
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(last) = states
                .get(&world)
                .and_then(|state| state.last_auto_rollback)
            {
                if last.elapsed() < self.config.min_rollback_interval {
                    return Err(SaveError::rejected("automatic rollback rate limited"));
                }
            }
        }

        let record = self.backend.info(source)?;
        if record.snapshot.world != world {
            return Err(SaveError::rejected("snapshot belongs to another world"));
        }
        let verification = self.backend.verify(source)?;
        if !verification.valid {
            return Err(SaveError::corrupt("snapshot failed verification"));
        }
        let slot = record.snapshot.slot;
        let live = self.backend.load(world, slot)?;
        let previous = live.as_ref().map(|image| image.revision).unwrap_or(0);
        if previous != expected {
            return Err(SaveError::conflict("stale live revision"));
        }
        audit.before_digest = match &live {
            Some(image) => Some(image.digest()?),
            None => None,
        };

        let pre_rollback = match &live {
            Some(image) => Some(self.capture_with_retention(
                world,
                slot,
                image,
                self.session_epoch(world).unwrap_or(0),
                false,
                SnapshotCause::PreRollback,
            )?),
            None => None,
        };

        let restore = self.backend.restore(source, expected)?;
        let restored = self
            .backend
            .load(world, slot)?
            .ok_or_else(|| SaveError::corrupt("restored state missing"))?;
        let original = SaveImage::decode(&record.snapshot.body)?;
        if restored.parts != original.parts
            || restored.format != original.format
            || restored.schema != original.schema
        {
            return Err(SaveError::corrupt("restored state failed verification"));
        }
        audit.after_digest = Some(restored.digest()?);

        let new_epoch = {
            let mut states = self
                .world_states
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let state = states.entry(world).or_insert(WorldState {
                epoch: 0,
                baseline_done: false,
                last_auto_rollback: None,
            });
            state.epoch = self.next_epoch.fetch_add(1, Ordering::SeqCst);
            state.baseline_done = false;
            if trigger == RollbackTrigger::Automatic {
                state.last_auto_rollback = Some(Instant::now());
            }
            state.epoch
        };

        for resetter in &self.resetters {
            resetter.reset_after_rollback(world, new_epoch);
        }
        let mut warnings = restore.warnings;
        if let Some(notifier) = &self.notifier {
            if let Err(error) =
                notifier.notify_rollback(world, new_epoch, self.config.notify_policy, reason)
            {
                warnings.push(format!(
                    "peer notification failed: {}",
                    sanitize(&error.to_string())
                ));
            }
        }

        Ok(RollbackReceipt {
            previous,
            current: restore.current,
            epoch: new_epoch,
            pre_rollback: pre_rollback.map(|record| record.snapshot.id),
            warnings,
        })
    }

    pub fn maybe_auto_rollback(
        &self,
        world: WorldId,
        reason: &str,
    ) -> Result<RollbackReceipt, SaveError> {
        if !self.config.auto_rollback {
            return Err(SaveError::rejected("automatic rollback is disabled"));
        }
        let entries = self.backend.scan(world)?;
        let mut chosen = None;
        for entry in entries.iter().rev() {
            if let Ok(verification) = self.backend.verify(entry.id) {
                if verification.valid {
                    chosen = Some(entry.clone());
                    break;
                }
            }
        }
        let chosen = chosen.ok_or_else(|| {
            SaveError::new(ErrorKind::Unavailable, "no valid snapshot is available")
        })?;
        let record = self.backend.info(chosen.id)?;
        let live = self.backend.load(world, record.snapshot.slot)?;
        let expected = live.as_ref().map(|image| image.revision).unwrap_or(0);
        self.rollback(
            world,
            chosen.id,
            expected,
            reason,
            RollbackTrigger::Automatic,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "save-hooks-{}-{}-{}",
                std::process::id(),
                tag,
                TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const WORLD: WorldId = WorldId([7u8; 16]);
    const SLOT: SaveSlot = SaveSlot(1);
    const FORMAT: FormatId = FormatId(*b"TEST0001");

    fn image(revision: Revision, body: &[u8]) -> SaveImage {
        SaveImage::single(
            FORMAT,
            CURRENT_IMAGE_SCHEMA,
            revision,
            "state.dat",
            body.to_vec(),
        )
    }

    fn config() -> SaveHookConfig {
        SaveHookConfig {
            min_free_bytes: 0,
            ..SaveHookConfig::default()
        }
    }

    fn replace_target() -> SaveTarget {
        SaveTarget {
            slot: SLOT,
            mode: SaveMode::Replace,
        }
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            digest_bytes(b"").to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest_bytes(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn atomic_commit_keeps_the_previous_save_on_rejection() {
        let dir = TempDir::new("atomic");
        let backend = FileBackend::open(dir.path()).unwrap();
        backend
            .commit(
                WORLD,
                SLOT,
                &image(1, b"one"),
                CommitOptions {
                    mode: SaveMode::Replace,
                    allow_world_readable: false,
                },
            )
            .unwrap();
        let target = backend.live_path(WORLD, SLOT).unwrap();
        let before = fs::read(&target).unwrap();

        let error = backend
            .commit(
                WORLD,
                SLOT,
                &image(2, b"two"),
                CommitOptions {
                    mode: SaveMode::FailIfExists,
                    allow_world_readable: false,
                },
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Rejected);
        assert_eq!(fs::read(&target).unwrap(), before);

        let mut newer = image(2, b"two");
        newer.schema = Version {
            major: 2,
            minor: 0,
            patch: 0,
        };
        let error = backend
            .commit(
                WORLD,
                SLOT,
                &newer,
                CommitOptions {
                    mode: SaveMode::Replace,
                    allow_world_readable: false,
                },
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnsupportedVersion);
        assert_eq!(fs::read(&target).unwrap(), before);

        let mut escaping = image(1, b"bad");
        escaping
            .parts
            .insert("../escape".to_string(), Arc::from(vec![1u8, 2, 3]));
        let error = backend
            .commit(
                WORLD,
                SLOT,
                &escaping,
                CommitOptions {
                    mode: SaveMode::Replace,
                    allow_world_readable: false,
                },
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Rejected);
        assert_eq!(fs::read(&target).unwrap(), before);
    }

    #[test]
    fn snapshots_create_baseline_roll_and_protect_newest() {
        let dir = TempDir::new("retain");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let hooks = SaveHooks::new(
            backend.clone(),
            SaveHookConfig {
                rolling_snapshots: 2,
                ..config()
            },
        );
        hooks.begin_session(WORLD);
        for revision in 1..=5 {
            hooks
                .save(
                    WORLD,
                    replace_target(),
                    image(revision, &[revision as u8; 16]),
                    SaveTrigger::Manual,
                )
                .unwrap();
        }
        let entries = hooks.snapshots(WORLD).unwrap();
        assert!(entries.len() <= 3, "retained {}", entries.len());
        let epoch = hooks.session_epoch(WORLD).unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry.baseline && entry.epoch == epoch));
        let newest = entries
            .iter()
            .max_by_key(|entry| entry.sequence)
            .expect("snapshot");
        assert!(hooks.verify(newest.id).unwrap().valid);
        for entry in &entries {
            assert!(hooks.verify(entry.id).unwrap().valid, "{:?}", entry.id);
        }
        assert_eq!(hooks.counters().saves_committed, 5);
        let save_records = hooks
            .audit_records()
            .iter()
            .filter(|record| matches!(record, AuditRecord::Save(_)))
            .count();
        assert_eq!(save_records, 5);
        let second = hooks.prune(WORLD).unwrap();
        assert!(second.removed.is_empty());
    }

    #[test]
    fn snapshot_verification_detects_corruption_and_newer_versions() {
        let dir = TempDir::new("verify");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let hooks = SaveHooks::new(backend.clone(), config());
        hooks.begin_session(WORLD);
        hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"payload"),
                SaveTrigger::Manual,
            )
            .unwrap();
        let entries = hooks.snapshots(WORLD).unwrap();
        let target = entries
            .iter()
            .find(|entry| !entry.baseline)
            .expect("rolling snapshot");
        let location = backend.snapshot_dir(target.id).unwrap();
        let image_path = location.join(IMAGE_FILE);
        let mut bytes = fs::read(&image_path).unwrap();
        bytes[0] ^= 0xff;
        fs::write(&image_path, &bytes).unwrap();
        let verification = hooks.verify(target.id).unwrap();
        assert!(!verification.valid);
        assert!(!verification.digest_matches);

        let manifest_path = location.join(MANIFEST_FILE);
        let mut manifest = fs::read(&manifest_path).unwrap();
        manifest[8..10].copy_from_slice(&u16::MAX.to_be_bytes());
        fs::write(&manifest_path, &manifest).unwrap();
        let error = hooks.verify(target.id).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnsupportedVersion);
    }

    struct ScriptedInterceptor {
        name: &'static str,
        skip: Option<SaveTrigger>,
        fail: bool,
    }

    impl SaveInterceptor for ScriptedInterceptor {
        fn name(&self) -> &str {
            self.name
        }

        fn before_save(&self, request: &SaveRequest) -> Result<InterceptDecision, SaveError> {
            if self.fail {
                return Err(SaveError::new(ErrorKind::Internal, "scripted failure"));
            }
            if Some(request.trigger) == self.skip {
                Ok(InterceptDecision::Skip)
            } else {
                Ok(InterceptDecision::Proceed)
            }
        }

        fn after_save(
            &self,
            _request: &SaveRequest,
            _receipt: &SaveCommit,
        ) -> Result<SnapshotVerification, SaveError> {
            Ok(SnapshotVerification {
                valid: true,
                compatibility: Compatibility::Writable,
                digest_matches: true,
                warnings: Vec::new(),
            })
        }
    }

    #[test]
    fn interceptor_skip_is_recorded_and_failure_falls_back_to_direct_save() {
        let dir = TempDir::new("interceptor");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let skip_hooks = SaveHooks::new(backend.clone(), config()).with_interceptor(Arc::new(
            ScriptedInterceptor {
                name: "skipper",
                skip: Some(SaveTrigger::Autosave),
                fail: false,
            },
        ));
        skip_hooks.begin_session(WORLD);
        let attempt = skip_hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"skipped"),
                SaveTrigger::Autosave,
            )
            .unwrap();
        assert_eq!(attempt.record.outcome, SaveResult::Skipped);
        assert!(matches!(
            attempt.record.interceptor,
            InterceptorStatus::SkippedBy(_)
        ));
        assert!(backend.load(WORLD, SLOT).unwrap().is_none());

        let fallback_hooks = SaveHooks::new(backend.clone(), config()).with_interceptor(Arc::new(
            ScriptedInterceptor {
                name: "failing",
                skip: None,
                fail: true,
            },
        ));
        fallback_hooks.begin_session(WORLD);
        let attempt = fallback_hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"committed"),
                SaveTrigger::Manual,
            )
            .unwrap();
        assert_eq!(attempt.record.outcome, SaveResult::Committed);
        assert!(matches!(
            attempt.record.interceptor,
            InterceptorStatus::Fallback { .. }
        ));
        assert!(backend.load(WORLD, SLOT).unwrap().is_some());
    }

    struct RedirectInterceptor {
        to: SaveSlot,
    }

    impl SaveInterceptor for RedirectInterceptor {
        fn name(&self) -> &str {
            "redirector"
        }

        fn before_save(&self, _request: &SaveRequest) -> Result<InterceptDecision, SaveError> {
            Ok(InterceptDecision::Redirect(self.to))
        }

        fn after_save(
            &self,
            _request: &SaveRequest,
            _receipt: &SaveCommit,
        ) -> Result<SnapshotVerification, SaveError> {
            Ok(SnapshotVerification {
                valid: true,
                compatibility: Compatibility::Writable,
                digest_matches: true,
                warnings: Vec::new(),
            })
        }
    }

    #[test]
    fn redirect_commits_to_the_alternate_destination() {
        let dir = TempDir::new("redirect");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let alternate = SaveSlot(9);
        let hooks = SaveHooks::new(backend.clone(), config())
            .with_interceptor(Arc::new(RedirectInterceptor { to: alternate }));
        hooks.begin_session(WORLD);
        let attempt = hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"redirected"),
                SaveTrigger::Manual,
            )
            .unwrap();
        assert_eq!(attempt.record.destination, slot_label(alternate));
        assert!(backend.load(WORLD, SLOT).unwrap().is_none());
        assert!(backend.load(WORLD, alternate).unwrap().is_some());
    }

    struct ReentrantProbe {
        hooks: Mutex<Option<std::sync::Weak<SaveHooks>>>,
        inner: Mutex<Option<SaveResult>>,
        entered: AtomicU64,
    }

    impl SaveInterceptor for ReentrantProbe {
        fn name(&self) -> &str {
            "reentrant-probe"
        }

        fn before_save(&self, _request: &SaveRequest) -> Result<InterceptDecision, SaveError> {
            if self.entered.fetch_add(1, Ordering::SeqCst) == 0 {
                let guarded = self
                    .hooks
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .as_ref()
                    .and_then(|weak| weak.upgrade());
                if let Some(hooks) = guarded {
                    let attempt = hooks
                        .save(
                            WORLD,
                            replace_target(),
                            image(9, b"inner"),
                            SaveTrigger::Explicit,
                        )
                        .unwrap();
                    *self
                        .inner
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner()) =
                        Some(attempt.record.outcome);
                }
            }
            Ok(InterceptDecision::Proceed)
        }

        fn after_save(
            &self,
            _request: &SaveRequest,
            _receipt: &SaveCommit,
        ) -> Result<SnapshotVerification, SaveError> {
            Ok(SnapshotVerification {
                valid: true,
                compatibility: Compatibility::Writable,
                digest_matches: true,
                warnings: Vec::new(),
            })
        }
    }

    #[test]
    fn reentrant_save_is_skipped_without_deadlock() {
        let dir = TempDir::new("reentrant");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let probe = Arc::new(ReentrantProbe {
            hooks: Mutex::new(None),
            inner: Mutex::new(None),
            entered: AtomicU64::new(0),
        });
        let hooks = Arc::new(SaveHooks::new(backend, config()).with_interceptor(probe.clone()));
        *probe
            .hooks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(Arc::downgrade(&hooks));
        let attempt = hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"outer"),
                SaveTrigger::Manual,
            )
            .unwrap();
        assert_eq!(attempt.record.outcome, SaveResult::Committed);
        assert_eq!(
            *probe
                .inner
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            Some(SaveResult::Skipped)
        );
    }

    struct ResetCount(AtomicU64);

    impl StateReset for ResetCount {
        fn reset_after_rollback(&self, _world: WorldId, _epoch: u64) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct NotifyCount(AtomicU64);

    impl PeerNotifier for NotifyCount {
        fn notify_rollback(
            &self,
            _world: WorldId,
            _epoch: u64,
            _policy: RollbackNotifyPolicy,
            _reason: &str,
        ) -> Result<(), SaveError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn rollback_restores_content_and_bumps_epoch_atomically() {
        let dir = TempDir::new("rollback");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let resets = Arc::new(ResetCount(AtomicU64::new(0)));
        let notifications = Arc::new(NotifyCount(AtomicU64::new(0)));
        let hooks = SaveHooks::new(backend.clone(), config())
            .with_resetter(resets.clone())
            .with_notifier(notifications.clone());
        hooks.begin_session(WORLD);
        hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"one"),
                SaveTrigger::Manual,
            )
            .unwrap();
        hooks
            .save(
                WORLD,
                replace_target(),
                image(2, b"two"),
                SaveTrigger::Manual,
            )
            .unwrap();
        let target = hooks
            .snapshots(WORLD)
            .unwrap()
            .into_iter()
            .filter(|entry| !entry.baseline)
            .find(|entry| {
                hooks
                    .snapshot_info(entry.id)
                    .map(|record| record.snapshot.revision == 1)
                    .unwrap_or(false)
            })
            .expect("revision one snapshot");
        let epoch_before = hooks.session_epoch(WORLD).unwrap();
        let receipt = hooks
            .rollback(WORLD, target.id, 2, "restore test", RollbackTrigger::Manual)
            .unwrap();
        assert_eq!(receipt.previous, 2);
        assert_eq!(receipt.current, 3);
        assert_eq!(receipt.epoch, epoch_before + 1);
        assert!(receipt.pre_rollback.is_some());
        let live = hooks.load(WORLD, SLOT).unwrap().unwrap();
        assert_eq!(live.revision, 3);
        assert_eq!(&live.parts["state.dat"][..], b"one");
        assert_eq!(resets.0.load(Ordering::SeqCst), 1);
        assert_eq!(notifications.0.load(Ordering::SeqCst), 1);

        let error = hooks
            .rollback(WORLD, target.id, 2, "stale", RollbackTrigger::Manual)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Conflict);
        assert_eq!(hooks.load(WORLD, SLOT).unwrap().unwrap().revision, 3);
    }

    #[test]
    fn corrupted_snapshot_rollback_leaves_live_state_intact() {
        let dir = TempDir::new("rollback-corrupt");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let hooks = SaveHooks::new(backend.clone(), config());
        hooks.begin_session(WORLD);
        hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"one"),
                SaveTrigger::Manual,
            )
            .unwrap();
        let target = hooks
            .snapshots(WORLD)
            .unwrap()
            .into_iter()
            .find(|entry| !entry.baseline)
            .expect("snapshot");
        hooks
            .save(
                WORLD,
                replace_target(),
                image(2, b"two"),
                SaveTrigger::Manual,
            )
            .unwrap();
        let epoch_before = hooks.session_epoch(WORLD).unwrap();
        let location = backend.snapshot_dir(target.id).unwrap();
        let image_path = location.join(IMAGE_FILE);
        let mut bytes = fs::read(&image_path).unwrap();
        bytes[0] ^= 0xff;
        fs::write(&image_path, &bytes).unwrap();
        let error = hooks
            .rollback(
                WORLD,
                target.id,
                2,
                "corrupt source",
                RollbackTrigger::Manual,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ErrorKind::Corrupt | ErrorKind::UnsupportedVersion
        ));
        let live = hooks.load(WORLD, SLOT).unwrap().unwrap();
        assert_eq!(live.revision, 2);
        assert_eq!(&live.parts["state.dat"][..], b"two");
        assert_eq!(hooks.session_epoch(WORLD).unwrap(), epoch_before);
        let failed = hooks
            .audit_records()
            .iter()
            .filter(|record| matches!(record, AuditRecord::Rollback(_)))
            .count();
        assert_eq!(failed, 1);
    }

    #[test]
    fn snapshot_budget_and_free_space_failures_do_not_touch_live_data() {
        let dir = TempDir::new("budget");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        backend
            .commit(
                WORLD,
                SLOT,
                &image(1, &[3u8; 64]),
                CommitOptions {
                    mode: SaveMode::Replace,
                    allow_world_readable: false,
                },
            )
            .unwrap();
        let hooks = SaveHooks::new(
            backend.clone(),
            SaveHookConfig {
                max_total_backup_bytes: 64,
                ..config()
            },
        );
        let error = hooks
            .capture(WORLD, SLOT, SnapshotCause::Manual)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Overloaded);
        assert_eq!(backend.load(WORLD, SLOT).unwrap().unwrap().revision, 1);

        let dir = TempDir::new("space");
        let backend =
            Arc::new(FileBackend::with_space(dir.path(), Arc::new(FixedSpace(4))).unwrap());
        backend
            .commit(
                WORLD,
                SLOT,
                &image(1, &[3u8; 64]),
                CommitOptions {
                    mode: SaveMode::Replace,
                    allow_world_readable: false,
                },
            )
            .unwrap();
        let hooks = SaveHooks::new(
            backend.clone(),
            SaveHookConfig {
                min_free_bytes: 1024,
                ..config()
            },
        );
        let error = hooks
            .capture(WORLD, SLOT, SnapshotCause::Manual)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Overloaded);
        assert_eq!(backend.load(WORLD, SLOT).unwrap().unwrap().revision, 1);
    }

    #[test]
    fn baseline_failure_aborts_the_first_mutating_save() {
        let dir = TempDir::new("baseline");
        let backend = Arc::new(FileBackend::open(dir.path()).unwrap());
        let hooks = SaveHooks::new(
            backend.clone(),
            SaveHookConfig {
                max_total_backup_bytes: 1,
                ..config()
            },
        );
        hooks.begin_session(WORLD);
        let error = hooks
            .save(
                WORLD,
                replace_target(),
                image(1, b"first"),
                SaveTrigger::Manual,
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Overloaded);
        assert!(backend.load(WORLD, SLOT).unwrap().is_none());
    }

    struct ConcurrencyProbe {
        inner: Arc<dyn SaveBackend>,
        active: AtomicU64,
        overlap: std::sync::atomic::AtomicBool,
    }

    impl SaveBackend for ConcurrencyProbe {
        fn load(&self, world: WorldId, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
            self.inner.load(world, slot)
        }

        fn commit(
            &self,
            world: WorldId,
            slot: SaveSlot,
            image: &SaveImage,
            options: CommitOptions,
        ) -> Result<SaveCommit, SaveError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            if active > 1 {
                self.overlap.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(5));
            let result = self.inner.commit(world, slot, image, options);
            self.active.fetch_sub(1, Ordering::SeqCst);
            result
        }

        fn capture(
            &self,
            world: WorldId,
            slot: SaveSlot,
            image: &SaveImage,
            request: SnapshotMetaRequest,
            budget: SnapshotBudget,
        ) -> Result<SnapshotRecord, SaveError> {
            self.inner.capture(world, slot, image, request, budget)
        }

        fn verify(&self, id: SnapshotId) -> Result<SnapshotVerification, SaveError> {
            self.inner.verify(id)
        }

        fn info(&self, id: SnapshotId) -> Result<SnapshotRecord, SaveError> {
            self.inner.info(id)
        }

        fn restore(&self, id: SnapshotId, expected: Revision) -> Result<RestoreReceipt, SaveError> {
            self.inner.restore(id, expected)
        }

        fn scan(&self, world: WorldId) -> Result<Vec<SnapshotEntry>, SaveError> {
            self.inner.scan(world)
        }

        fn remove(&self, id: SnapshotId) -> Result<(), SaveError> {
            self.inner.remove(id)
        }

        fn available_bytes(&self) -> Result<u64, SaveError> {
            self.inner.available_bytes()
        }
    }

    #[test]
    fn saves_for_one_world_are_serialized_across_threads() {
        let dir = TempDir::new("concurrent");
        let backend = Arc::new(ConcurrencyProbe {
            inner: Arc::new(FileBackend::open(dir.path()).unwrap()),
            active: AtomicU64::new(0),
            overlap: std::sync::atomic::AtomicBool::new(false),
        });
        let hooks = Arc::new(SaveHooks::new(
            backend.clone(),
            SaveHookConfig {
                rolling_snapshots: 1,
                ..config()
            },
        ));
        hooks.begin_session(WORLD);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let hooks = hooks.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                hooks
                    .save(
                        WORLD,
                        replace_target(),
                        image(1, b"same"),
                        SaveTrigger::Manual,
                    )
                    .map(|attempt| attempt.record.outcome)
            }));
        }
        for handle in handles {
            assert_eq!(handle.join().unwrap().unwrap(), SaveResult::Committed);
        }
        assert!(!backend.overlap.load(Ordering::SeqCst));
        assert_eq!(hooks.counters().saves_committed, 2);
    }

    #[test]
    fn path_confinement_rejects_traversal_and_symlink_escapes() {
        let dir = TempDir::new("paths");
        let root = dir.path();
        fs::create_dir_all(root.join("nested")).unwrap();
        assert!(confined_join(root, Path::new("../escape")).is_err());
        assert!(confined_join(root, Path::new("/etc/passwd")).is_err());
        assert!(confined_join(root, Path::new("nested/../../escape")).is_err());
        assert!(confined_join(root, Path::new("nested/file")).is_ok());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let outside = TempDir::new("outside");
            symlink(outside.path(), root.join("link")).unwrap();
            assert!(confined_join(root, Path::new("link/file")).is_err());

            let backend = FileBackend::open(root).unwrap();
            backend
                .commit(
                    WORLD,
                    SLOT,
                    &image(1, b"private"),
                    CommitOptions {
                        mode: SaveMode::Replace,
                        allow_world_readable: false,
                    },
                )
                .unwrap();
            let mode = fs::metadata(backend.live_path(WORLD, SLOT).unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o007, 0);
        }
    }
}
