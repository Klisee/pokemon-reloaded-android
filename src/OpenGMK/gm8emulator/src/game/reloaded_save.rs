//! Host adapter for the clean-room save hooks.
//!
//! The clean-room module owns snapshot, retention, audit, and rollback policy.
//! This module maps the host slot layout (`data/partidas/reloaded{slot}` with
//! `.savc`, `.nfo`, and `.data` companions) to opaque save images and keeps the
//! live files at their canonical paths.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use reloaded_cleanroom::save::{
    self, CURRENT_IMAGE_SCHEMA, CommitOptions, FileBackend, FormatId, MAX_PART_BYTES, Revision, SaveBackend,
    SaveCommit, SaveError, SaveHookConfig, SaveHooks, SaveImage, SaveMode, SaveSlot, SaveTarget, SaveTrigger,
    SnapshotBudget, SnapshotCause, SnapshotEntry, SnapshotId, SnapshotMetaRequest, SnapshotRecord,
    SnapshotVerification, WorldId,
};

pub(crate) const HOST_FORMAT: FormatId = FormatId(*b"RELOADD1");
pub(crate) const SAVC: &str = "savc";
pub(crate) const NFO: &str = "nfo";
pub(crate) const DATA: &str = "data";

const LIVE_PARTS: [&str; 3] = [SAVC, NFO, DATA];
const BACKUP_DIR: &str = ".reloaded-save";
const STAGING_DIR: &str = "staging";
const MAX_HOST_FILE_BYTES: u64 = MAX_PART_BYTES;
static NEXT_STAGING: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostSavePath {
    pub program_dir: PathBuf,
    pub slot: SaveSlot,
    pub part: &'static str,
}

fn part_name(extension: &str) -> Option<&'static str> {
    match extension {
        "savc" => Some(SAVC),
        "nfo" => Some(NFO),
        "data" => Some(DATA),
        _ => None,
    }
}

fn part_extension(name: &str) -> Option<&'static str> {
    match name {
        SAVC => Some("savc"),
        NFO => Some("nfo"),
        DATA => Some("data"),
        _ => None,
    }
}

fn name_is(parent: &Path, expected: &str) -> bool {
    parent.file_name().and_then(|name| name.to_str()).map(|name| name.eq_ignore_ascii_case(expected)).unwrap_or(false)
}

/// Recognizes the canonical host save paths. Anything else stays under the
/// existing atomic write path, and `.online` guard files are never adopted.
pub(crate) fn parse_save_path(path: &Path) -> Option<HostSavePath> {
    let file_name = path.file_name()?.to_str()?;
    let (stem, extension) = file_name.rsplit_once('.')?;
    let part = part_name(&extension.to_ascii_lowercase())?;
    let slot_text = stem.strip_prefix("reloaded")?;
    if slot_text.is_empty() || !slot_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let slot = slot_text.parse::<u16>().ok()?;
    let partidas = path.parent()?;
    if !name_is(partidas, "partidas") {
        return None;
    }
    let data = partidas.parent()?;
    if !name_is(data, "data") {
        return None;
    }
    let program_dir = data.parent()?.to_path_buf();
    Some(HostSavePath { program_dir, slot: SaveSlot(slot), part })
}

fn clean_staging(root: &Path) {
    let staging = root.join(BACKUP_DIR).join(STAGING_DIR);
    let Ok(entries) = fs::read_dir(&staging) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("journal") {
            continue
        }
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let _ = fs::remove_file(path);
            },
            Ok(meta) if meta.is_dir() => {
                let _ = fs::remove_dir_all(path);
            },
            Ok(_) => {
                let _ = fs::remove_file(path);
            },
            Err(_) => {},
        }
    }
}

/// Removes write temps left beside a live slot by a crash between the write and
/// its rename. Atomic commits make them stale on the next start.
fn clean_save_temps(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("reloaded") {
            continue
        }
        let Some((_, suffix)) = name.split_once(".tmp") else { continue };
        if !suffix.is_empty() && !suffix.starts_with('-') {
            continue
        }
        match fs::symlink_metadata(entry.path()) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                let _ = fs::remove_file(entry.path());
            },
            _ => {},
        }
    }
}

fn revisions_path(root: &Path) -> PathBuf {
    root.join(BACKUP_DIR).join("revisions")
}

fn read_revisions(path: &Path) -> BTreeMap<u16, Revision> {
    let mut revisions = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else { return revisions };
    for line in text.lines() {
        let Some((slot, revision)) = line.split_once(' ') else { continue };
        let (Ok(slot), Ok(revision)) = (slot.parse::<u16>(), revision.parse::<Revision>()) else { continue };
        revisions.insert(slot, revision);
    }
    revisions
}

fn persist_revision(path: &Path, slot: u16, revision: Revision) {
    let mut revisions = read_revisions(path);
    revisions.insert(slot, revision);
    let mut text = String::new();
    for (slot, revision) in revisions {
        text.push_str(&format!("{slot} {revision}\n"));
    }
    let _ = super::save_guard::write_file_atomic(path, text.as_bytes());
}

/// Combines the revision sidecar with the snapshot manifests once at startup, so
/// a lost or stale sidecar cannot make revisions go backwards after a restart.
fn reconcile_revisions(root: &Path, inner: &FileBackend, world: WorldId) -> BTreeMap<u16, Revision> {
    let mut revisions = read_revisions(&revisions_path(root));
    let Ok(entries) = inner.scan(world) else { return revisions };
    for entry in entries {
        if let Ok(record) = inner.info(entry.id) {
            let slot = record.snapshot.slot.0;
            let revision = record.snapshot.revision;
            let known = revisions.entry(slot).or_insert(0);
            if revision > *known {
                *known = revision;
            }
        }
    }
    revisions
}

pub(crate) struct HostSaveBackend {
    root: PathBuf,
    world: WorldId,
    inner: FileBackend,
    revisions: Mutex<BTreeMap<u16, Revision>>,
    commit_lock: Mutex<()>,
}

impl HostSaveBackend {
    fn open(root: &Path) -> Result<Self, SaveError> {
        fs::create_dir_all(root)?;
        let root = fs::canonicalize(root)?;
        let recovered = recover_journals(&root);
        clean_staging(&root);
        if recovered {
            clean_save_temps(&root);
        }
        let inner = FileBackend::open(root.join(BACKUP_DIR))?;
        let world = world_id(&root);
        let revisions = reconcile_revisions(&root, &inner, world);
        Ok(HostSaveBackend { root, world, inner, revisions: Mutex::new(revisions), commit_lock: Mutex::new(()) })
    }

    pub(crate) fn world(&self) -> WorldId {
        self.world
    }

    pub(crate) fn staging_dir(&self) -> PathBuf {
        self.root.join(BACKUP_DIR).join(STAGING_DIR)
    }

    pub(crate) fn live_path(&self, slot: SaveSlot, part: &str) -> Option<PathBuf> {
        let extension = part_extension(part)?;
        Some(self.root.join(format!("reloaded{}.{}", slot.0, extension)))
    }

    fn revision(&self, slot: SaveSlot) -> Revision {
        let persisted = read_revisions(&revisions_path(&self.root)).get(&slot.0).copied();
        let memory = self
            .revisions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&slot.0)
            .copied();
        persisted.unwrap_or(0).max(memory.unwrap_or(0))
    }

    fn next_revision(&self, slot: SaveSlot) -> Result<Revision, SaveError> {
        self.revision(slot).checked_add(1).ok_or_else(|| SaveError::conflict("save revision exhausted"))
    }

    fn read_part(&self, slot: SaveSlot, part: &str) -> Result<Option<Arc<[u8]>>, SaveError> {
        let path = self.live_path(slot, part).ok_or_else(|| SaveError::rejected("unknown save part"))?;
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => Err(SaveError::rejected("save part is a symlink")),
            Ok(meta) if !meta.is_file() => Err(SaveError::corrupt("save part is not a file")),
            Ok(meta) => {
                if meta.len() > MAX_HOST_FILE_BYTES {
                    return Err(SaveError::overloaded("save part exceeds size ceiling"));
                }
                let bytes = fs::read(&path)?;
                if bytes.len() as u64 > MAX_HOST_FILE_BYTES {
                    return Err(SaveError::overloaded("save part exceeds size ceiling"));
                }
                Ok(Some(Arc::from(bytes)))
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn prepare_journal(
        &self,
        slot: SaveSlot,
        revision: Revision,
        mode: &'static str,
        writes: Vec<(&'static str, Arc<[u8]>)>,
        removes: Vec<String>,
    ) -> Result<SaveJournal, SaveError> {
        let mut prepared = Vec::new();
        for (part, body) in writes {
            let path = match self.live_path(slot, part) {
                Some(path) => path,
                None => {
                    for (_, temporary) in &prepared {
                        let _ = fs::remove_file(temporary);
                    }
                    return Err(SaveError::rejected("unknown save part"))
                },
            };
            match super::save_guard::prepare_file_atomic(&path, &body) {
                Ok(temporary) => prepared.push((part.to_string(), temporary)),
                Err(error) => {
                    for (_, temporary) in &prepared {
                        let _ = fs::remove_file(temporary);
                    }
                    return Err(error.into())
                },
            }
        }
        Ok(SaveJournal {
            slot: slot.0,
            revision,
            mode,
            writes: prepared,
            removes,
            path: journal_path(&self.root, slot.0),
        })
    }

    fn abort_journal(
        &self,
        slot: SaveSlot,
        image: &SaveImage,
        old_parts: &BTreeMap<String, Arc<[u8]>>,
    ) -> Result<SaveJournal, SaveError> {
        let mut writes = Vec::new();
        let mut removes = Vec::new();
        for part in LIVE_PARTS {
            let new = image.parts.get(part);
            let old = old_parts.get(part);
            if new == old {
                continue
            }
            match old {
                Some(body) => writes.push((part, body.clone())),
                None => removes.push(part.to_string()),
            }
        }
        self.prepare_journal(slot, image.revision, JOURNAL_ABORT, writes, removes)
    }
}

fn world_id(root: &Path) -> WorldId {
    let digest = save::digest_bytes(root.as_os_str().to_string_lossy().as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.0[..16]);
    WorldId(bytes)
}

const JOURNAL_VERSION: &str = "v1";
const JOURNAL_COMMIT: &str = "commit";
const JOURNAL_ABORT: &str = "abort";

struct SaveJournal {
    slot: u16,
    revision: Revision,
    mode: &'static str,
    writes: Vec<(String, PathBuf)>,
    removes: Vec<String>,
    path: PathBuf,
}

impl SaveJournal {
    fn encode(&self) -> String {
        let mut text = String::new();
        text.push_str(JOURNAL_VERSION);
        text.push('\n');
        text.push_str("mode ");
        text.push_str(self.mode);
        text.push('\n');
        text.push_str(&format!("slot {}\n", self.slot));
        text.push_str(&format!("revision {}\n", self.revision));
        for (part, temporary) in &self.writes {
            let name = temporary.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            text.push_str(&format!("write {part} {name}\n"));
        }
        for part in &self.removes {
            text.push_str(&format!("remove {part}\n"));
        }
        text
    }

    fn write(&self) -> Result<(), SaveError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        super::save_guard::write_file_atomic(&self.path, self.encode().as_bytes())?;
        Ok(())
    }

    fn apply(&self, root: &Path) -> Result<(), SaveError> {
        for (part, temporary) in &self.writes {
            let extension = part_extension(part).ok_or_else(|| SaveError::rejected("unknown save part"))?;
            let destination = root.join(format!("reloaded{}.{}", self.slot, extension));
            if fs::symlink_metadata(temporary).is_ok() {
                super::save_guard::commit_prepared_file(temporary, &destination)?;
            }
        }
        for part in &self.removes {
            let extension = part_extension(part).ok_or_else(|| SaveError::rejected("unknown save part"))?;
            let destination = root.join(format!("reloaded{}.{}", self.slot, extension));
            match fs::remove_file(&destination) {
                Ok(()) => super::save_guard::sync_parent_dir(&destination),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn finish(&self) {
        let _ = fs::remove_file(&self.path);
        super::save_guard::sync_parent_dir(&self.path);
    }

    fn discard_temps(&self) {
        for (_, temporary) in &self.writes {
            let _ = fs::remove_file(temporary);
        }
    }
}

fn journal_path(root: &Path, slot: u16) -> PathBuf {
    root.join(BACKUP_DIR).join(STAGING_DIR).join(format!("slot-{slot}.journal"))
}

fn valid_journal_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && name.starts_with("reloaded")
        && name.contains(".tmp")
}

fn parse_journal(root: &Path, path: &Path, text: &str) -> Option<SaveJournal> {
    let mut lines = text.lines();
    if lines.next()? != JOURNAL_VERSION {
        return None
    }
    let mode = match lines.next()?.strip_prefix("mode ")? {
        JOURNAL_COMMIT => JOURNAL_COMMIT,
        JOURNAL_ABORT => JOURNAL_ABORT,
        _ => return None,
    };
    let slot = lines.next()?.strip_prefix("slot ")?.parse::<u16>().ok()?;
    let revision = lines.next()?.strip_prefix("revision ")?.parse::<Revision>().ok()?;
    let mut writes = Vec::new();
    let mut removes = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("write ") {
            let (part, name) = rest.split_once(' ')?;
            if part_extension(part).is_none() || !valid_journal_name(name) {
                return None
            }
            writes.push((part.to_string(), root.join(name)));
        } else if let Some(part) = line.strip_prefix("remove ") {
            if part_extension(part).is_none() {
                return None
            }
            removes.push(part.to_string());
        } else if !line.is_empty() {
            return None
        }
    }
    Some(SaveJournal { slot, revision, mode, writes, removes, path: path.to_path_buf() })
}

fn recover_journals(root: &Path) -> bool {
    let staging = root.join(BACKUP_DIR).join(STAGING_DIR);
    let Ok(entries) = fs::read_dir(&staging) else { return true };
    let mut journals: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("journal"))
        .collect();
    journals.sort();
    let mut clean = true;
    for path in journals {
        let Ok(text) = fs::read_to_string(&path) else {
            clean = false;
            continue
        };
        let Some(journal) = parse_journal(root, &path, &text) else {
            let _ = fs::remove_file(&path);
            continue
        };
        match journal.apply(root) {
            Ok(()) => journal.finish(),
            Err(error) => {
                clean = false;
                eprintln!("[save] no se pudo recuperar {}: {error}", path.display());
            },
        }
    }
    clean
}

impl SaveBackend for HostSaveBackend {
    fn load(&self, world: WorldId, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
        if world != self.world {
            return Err(SaveError::rejected("save world mismatch"));
        }
        let mut parts = BTreeMap::new();
        for part in LIVE_PARTS {
            if let Some(body) = self.read_part(slot, part)? {
                parts.insert(part.to_string(), body);
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(SaveImage { format: HOST_FORMAT, schema: CURRENT_IMAGE_SCHEMA, revision: self.revision(slot), parts }))
    }

    fn commit(
        &self,
        world: WorldId,
        slot: SaveSlot,
        image: &SaveImage,
        options: CommitOptions,
    ) -> Result<SaveCommit, SaveError> {
        if world != self.world {
            return Err(SaveError::rejected("save world mismatch"));
        }
        image.validate()?;
        for name in image.parts.keys() {
            if part_extension(name).is_none() {
                return Err(SaveError::rejected("unknown save part"));
            }
        }

        let _guard = self.commit_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let previous_image = self.load(world, slot)?;
        let previous = previous_image.as_ref().map(|image| image.revision).unwrap_or(0);
        let digest = image.digest()?;

        if options.mode == SaveMode::VerifyOnly {
            let live = previous_image.ok_or_else(|| SaveError::rejected("verify target missing"))?;
            if live.digest()? != digest {
                return Err(SaveError::corrupt("verify digest mismatch"));
            }
            return Ok(SaveCommit { slot, previous, current: previous, digest });
        }
        if options.mode == SaveMode::FailIfExists && previous_image.is_some() {
            return Err(SaveError::rejected("target already exists"));
        }
        if image.revision < previous {
            return Err(SaveError::conflict("revision would decrease"));
        }

        let old_parts = previous_image.as_ref().map(|image| image.parts.clone()).unwrap_or_default();
        let mut writes = Vec::new();
        let mut removes = Vec::new();
        for part in LIVE_PARTS {
            match image.parts.get(part) {
                Some(body) => {
                    if old_parts.get(part).map(|old| old == body).unwrap_or(false) {
                        continue
                    }
                    writes.push((part, body.clone()));
                },
                None if old_parts.contains_key(part) => removes.push(part.to_string()),
                None => {},
            }
        }

        // All changed parts are fully written and fsynced before the journal is
        // published. The journal then renames them in canonical order
        // (`.savc`, `.nfo`, `.data`) and removes absent companions last; a
        // crash at any point is completed on the next open.
        let journal = self.prepare_journal(slot, image.revision, JOURNAL_COMMIT, writes, removes)?;
        if let Err(error) = journal.write() {
            journal.discard_temps();
            return Err(error)
        }
        persist_revision(&revisions_path(&self.root), slot.0, image.revision);
        if let Err(error) = journal.apply(&self.root) {
            let aborted = match self.abort_journal(slot, image, &old_parts) {
                Ok(abort) => match abort.write() {
                    Ok(()) => match abort.apply(&self.root) {
                        Ok(()) => {
                            abort.finish();
                            true
                        },
                        Err(abort_error) => {
                            eprintln!("[save] no se pudo revertir el commit parcial: {abort_error}");
                            false
                        },
                    },
                    Err(abort_error) => {
                        eprintln!("[save] no se pudo publicar la reversion: {abort_error}");
                        false
                    },
                },
                Err(abort_error) => {
                    eprintln!("[save] no se pudo preparar la reversion: {abort_error}");
                    false
                },
            };
            if aborted {
                journal.discard_temps();
            }
            return Err(error)
        }
        journal.finish();

        persist_revision(&revisions_path(&self.root), slot.0, image.revision);
        self.revisions.lock().unwrap_or_else(|poison| poison.into_inner()).insert(slot.0, image.revision);
        Ok(SaveCommit { slot, previous, current: image.revision, digest })
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

    fn restore(&self, id: SnapshotId, expected: Revision) -> Result<save::RestoreReceipt, SaveError> {
        let record = self.info(id)?;
        if record.snapshot.world != self.world {
            return Err(SaveError::rejected("snapshot belongs to another world"));
        }
        let verification = self.verify(id)?;
        if !verification.valid {
            return Err(SaveError::corrupt("snapshot failed verification"));
        }
        let original = SaveImage::decode(&record.snapshot.body)?;
        let world = record.snapshot.world;
        let slot = record.snapshot.slot;
        let live = self.load(world, slot)?;
        let previous = live.as_ref().map(|image| image.revision).unwrap_or(0);
        if previous != expected {
            return Err(SaveError::conflict("stale live revision"));
        }

        let mut image = original.clone();
        image.revision = previous.checked_add(1).ok_or_else(|| SaveError::conflict("revision exhausted"))?;
        let commit =
            self.commit(world, slot, &image, CommitOptions { mode: SaveMode::Replace, allow_world_readable: false })?;

        let restored = self.load(world, slot)?;
        let matches = match &restored {
            Some(restored) => {
                restored.format == original.format
                    && restored.schema == original.schema
                    && restored.parts == original.parts
            }
            None => original.parts.is_empty(),
        };
        if !matches {
            let revision = restored.as_ref().map(|image| image.revision).unwrap_or(commit.current);
            if let Some(mut previous_image) = live {
                previous_image.revision = revision.max(previous_image.revision);
                let _ = self.commit(
                    world,
                    slot,
                    &previous_image,
                    CommitOptions { mode: SaveMode::Replace, allow_world_readable: false },
                );
            }
            return Err(SaveError::corrupt("restored state failed verification"));
        }
        Ok(save::RestoreReceipt { previous, current: commit.current, warnings: Vec::new() })
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

pub(crate) struct HostSaveService {
    backend: Arc<HostSaveBackend>,
    hooks: SaveHooks,
    save_lock: Mutex<()>,
}

impl HostSaveService {
    fn open_with_config(program_dir: &Path, config: SaveHookConfig) -> Result<Self, SaveError> {
        let root = program_dir.join("data").join("partidas");
        let backend = Arc::new(HostSaveBackend::open(&root)?);
        let hooks = SaveHooks::new(backend.clone(), config);
        Ok(HostSaveService { backend, hooks, save_lock: Mutex::new(()) })
    }

    pub(crate) fn world(&self) -> WorldId {
        self.backend.world()
    }

    pub(crate) fn staging_dir(&self) -> PathBuf {
        self.backend.staging_dir()
    }

    fn load_unlocked(&self, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
        self.hooks.load(self.world(), slot)
    }

    pub(crate) fn load(&self, slot: SaveSlot) -> Result<Option<SaveImage>, SaveError> {
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        self.load_unlocked(slot)
    }

    pub(crate) fn load_part(&self, slot: SaveSlot, part: &str) -> Result<Option<Arc<[u8]>>, SaveError> {
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(self.load_unlocked(slot)?.and_then(|image| image.parts.get(part).cloned()))
    }

    pub(crate) fn capture(&self, slot: SaveSlot) -> Result<SnapshotRecord, SaveError> {
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        self.hooks.capture(self.world(), slot, SnapshotCause::Manual)
    }

    pub(crate) fn save_part(
        &self,
        slot: SaveSlot,
        part: &str,
        bytes: &[u8],
        trigger: SaveTrigger,
    ) -> Result<save::SaveAttempt, SaveError> {
        if part_extension(part).is_none() {
            return Err(SaveError::rejected("unknown save part"));
        }
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut image = match self.load_unlocked(slot)? {
            Some(image) => image,
            None => SaveImage::empty(HOST_FORMAT, CURRENT_IMAGE_SCHEMA, 0),
        };
        image.parts.insert(part.to_string(), Arc::from(bytes));
        self.save_image_locked(slot, image, trigger)
    }

    fn save_image_locked(
        &self,
        slot: SaveSlot,
        mut image: SaveImage,
        trigger: SaveTrigger,
    ) -> Result<save::SaveAttempt, SaveError> {
        image.revision = self.backend.next_revision(slot)?;
        self.hooks.save(self.world(), SaveTarget { slot, mode: SaveMode::Replace }, image, trigger)
    }

    /// Removes one part, committing an empty image when it was the last one so
    /// the revision and snapshot trail still record the deletion.
    pub(crate) fn remove_part(
        &self,
        slot: SaveSlot,
        part: &str,
        trigger: SaveTrigger,
    ) -> Result<(), SaveError> {
        if part_extension(part).is_none() {
            return Err(SaveError::rejected("unknown save part"))
        }
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(mut image) = self.load_unlocked(slot)? else { return Ok(()) };
        if image.parts.remove(part).is_none() {
            return Ok(())
        }
        self.save_image_locked(slot, image, trigger).map(|_| ())
    }

    pub(crate) fn copy_part(
        &self,
        from_slot: SaveSlot,
        from_part: &str,
        to_slot: SaveSlot,
        to_part: &str,
        trigger: SaveTrigger,
    ) -> Result<(), SaveError> {
        if part_extension(from_part).is_none() || part_extension(to_part).is_none() {
            return Err(SaveError::rejected("unknown save part"))
        }
        if from_slot == to_slot && from_part == to_part {
            return Ok(())
        }
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let source = self.load_unlocked(from_slot)?.ok_or_else(|| SaveError::rejected("source save missing"))?;
        let body = source
            .parts
            .get(from_part)
            .cloned()
            .ok_or_else(|| SaveError::rejected("source save part missing"))?;
        let mut image = match self.load_unlocked(to_slot)? {
            Some(image) => image,
            None => SaveImage::empty(HOST_FORMAT, CURRENT_IMAGE_SCHEMA, 0),
        };
        image.parts.insert(to_part.to_string(), body);
        self.save_image_locked(to_slot, image, trigger).map(|_| ())
    }

    pub(crate) fn rename_part(
        &self,
        from_slot: SaveSlot,
        from_part: &str,
        to_slot: SaveSlot,
        to_part: &str,
        trigger: SaveTrigger,
    ) -> Result<(), SaveError> {
        if part_extension(from_part).is_none() || part_extension(to_part).is_none() {
            return Err(SaveError::rejected("unknown save part"))
        }
        if from_slot == to_slot && from_part == to_part {
            return Ok(())
        }
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let source = self.load_unlocked(from_slot)?.ok_or_else(|| SaveError::rejected("source save missing"))?;
        let body = source
            .parts
            .get(from_part)
            .cloned()
            .ok_or_else(|| SaveError::rejected("source save part missing"))?;
        if from_slot == to_slot {
            let mut image = source;
            image.parts.remove(from_part);
            image.parts.insert(to_part.to_string(), body);
            return self.save_image_locked(to_slot, image, trigger).map(|_| ())
        }

        let mut destination = match self.load_unlocked(to_slot)? {
            Some(image) => image,
            None => SaveImage::empty(HOST_FORMAT, CURRENT_IMAGE_SCHEMA, 0),
        };
        destination.parts.insert(to_part.to_string(), body);
        self.save_image_locked(to_slot, destination, trigger)?;

        let mut image = source;
        image.parts.remove(from_part);
        self.save_image_locked(from_slot, image, trigger).map(|_| ())
    }

    pub(crate) fn snapshots(&self, slot: SaveSlot) -> Result<Vec<SnapshotEntry>, SaveError> {
        self.hooks
            .snapshots(self.world())
            .map(|entries| entries.into_iter().filter(|entry| self.slot_of(entry.id) == Some(slot)).collect())
    }

    fn slot_of(&self, id: SnapshotId) -> Option<SaveSlot> {
        self.hooks.snapshot_info(id).ok().map(|record| record.snapshot.slot)
    }

    pub(crate) fn snapshot_info(&self, id: SnapshotId) -> Result<SnapshotRecord, SaveError> {
        self.hooks.snapshot_info(id)
    }

    pub(crate) fn verify(&self, id: SnapshotId) -> Result<SnapshotVerification, SaveError> {
        self.hooks.verify(id)
    }

    pub(crate) fn rollback(
        &self,
        slot: SaveSlot,
        source: SnapshotId,
        expected: Revision,
        reason: &str,
    ) -> Result<save::RollbackReceipt, SaveError> {
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = self.hooks.snapshot_info(source)?;
        if record.snapshot.slot != slot {
            return Err(SaveError::rejected("snapshot belongs to another slot"));
        }
        self.hooks.rollback(self.world(), source, expected, reason, save::RollbackTrigger::Manual)
    }

    pub(crate) fn begin_session(&self, slot: SaveSlot) -> Result<Option<SnapshotRecord>, SaveError> {
        let _guard = self.save_lock.lock().unwrap_or_else(|poison| poison.into_inner());
        if self.load_unlocked(slot)?.is_none() {
            return Ok(None);
        }
        self.hooks.begin_session(self.world());
        match self.hooks.capture(self.world(), slot, SnapshotCause::Baseline) {
            Ok(record) => Ok(Some(record)),
            Err(error) if matches!(error.kind(), save::ErrorKind::Unavailable | save::ErrorKind::Rejected) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

static SERVICES: OnceLock<Mutex<HashMap<PathBuf, Arc<HostSaveService>>>> = OnceLock::new();

fn services() -> &'static Mutex<HashMap<PathBuf, Arc<HostSaveService>>> {
    SERVICES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn service_for(program_dir: &Path) -> Result<Arc<HostSaveService>, SaveError> {
    let key = fs::canonicalize(program_dir).unwrap_or_else(|_| program_dir.to_path_buf());
    let mut map = services().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(service) = map.get(&key) {
        return Ok(service.clone());
    }
    let service = Arc::new(HostSaveService::open_with_config(&key, SaveHookConfig::default())?);
    map.insert(key, service.clone());
    Ok(service)
}

fn save_error_io(error: SaveError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
}

/// Backup/snapshot failures must not make an otherwise valid game save fail.
/// Hard contract failures (corrupt data, conflicts, unsafe paths) still do.
fn backup_failure(error: &SaveError) -> bool {
    matches!(
        error.kind(),
        save::ErrorKind::Unavailable
            | save::ErrorKind::Timeout
            | save::ErrorKind::Overloaded
            | save::ErrorKind::Io
            | save::ErrorKind::Internal
    )
}

fn finish_save(path: &Path, bytes: &[u8], result: Result<(), SaveError>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if backup_failure(&error) => {
            eprintln!(
                "[save] la copia de seguridad no esta disponible ({error}); escritura directa de {}",
                path.display()
            );
            super::save_guard::write_file_atomic(path, bytes)
        },
        Err(error) => Err(save_error_io(error)),
    }
}

fn save_part_checked(path: &Path, bytes: &[u8]) -> Option<Result<(), SaveError>> {
    let parsed = parse_save_path(path)?;
    if !parsed.program_dir.join("data").join("partidas").is_dir() {
        return None
    }
    Some(
        service_for(&parsed.program_dir)
            .and_then(|service| {
                service
                    .save_part(parsed.slot, parsed.part, bytes, SaveTrigger::Manual)
                    .map(|_| ())
            }),
    )
}

/// A staging file for a GML text/binary write to a canonical save part. The
/// canonical file is only replaced when the handle flushes, through the same
/// atomic commit and snapshot path as `game_save`.
#[derive(Debug)]
pub(crate) struct StagedSave {
    canonical: PathBuf,
    staging: PathBuf,
    dirty: bool,
}

impl StagedSave {
    fn new(
        service: &HostSaveService,
        canonical: &Path,
        copy_existing: bool,
        force_dirty: bool,
    ) -> io::Result<Self> {
        let dir = service.staging_dir();
        fs::create_dir_all(&dir)?;
        let staging = loop {
            let counter = NEXT_STAGING.fetch_add(1, Ordering::Relaxed);
            let candidate = dir.join(format!(".tmp-part-{}-{counter}", std::process::id()));
            match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
                Ok(file) => {
                    drop(file);
                    break candidate
                },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };

        let mut dirty = force_dirty;
        if copy_existing {
            match fs::symlink_metadata(canonical) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    let _ = fs::remove_file(&staging);
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "save part is a symlink"))
                },
                Ok(meta) if !meta.is_file() => {
                    let _ = fs::remove_file(&staging);
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "save part is not a file"))
                },
                Ok(_) => {
                    fs::copy(canonical, &staging)?;
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    dirty = true;
                },
                Err(error) => {
                    let _ = fs::remove_file(&staging);
                    return Err(error)
                },
            }
        } else {
            dirty = true;
        }
        Ok(StagedSave { canonical: canonical.to_path_buf(), staging, dirty })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.staging
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn commit(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(())
        }
        let bytes = fs::read(&self.staging)?;
        match save_part_checked(&self.canonical, &bytes) {
            Some(result) => finish_save(&self.canonical, &bytes, result)?,
            None => return Err(io::Error::new(io::ErrorKind::Other, "save part is no longer recognized")),
        }
        self.dirty = false;
        Ok(())
    }
}

impl Drop for StagedSave {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.staging);
    }
}

/// Starts a staged write for a canonical save part, or `None` when the path is
/// not a save part or the save service is unavailable (the caller then writes
/// directly, preserving the legacy behavior).
pub(crate) fn begin_staged_write(
    canonical: &Path,
    copy_existing: bool,
    force_dirty: bool,
) -> Option<io::Result<StagedSave>> {
    let parsed = parse_save_path(canonical)?;
    let service = service_for(&parsed.program_dir).ok()?;
    Some(StagedSave::new(&service, canonical, copy_existing, force_dirty))
}

/// Reads a canonical save part through the hooks so a load never observes a
/// half-committed slot. Returns `None` when the path is not that save part.
pub(crate) fn load_part_atomic(path: &Path, part: &str) -> Option<io::Result<Option<Arc<[u8]>>>> {
    let parsed = parse_save_path(path)?;
    if parsed.part != part {
        return None
    }
    let service = service_for(&parsed.program_dir).ok()?;
    Some(service.load_part(parsed.slot, part).map_err(save_error_io))
}

/// Routes a canonical host save write through the clean-room save hooks.
/// Returns `None` when the path is not a host save part. Backup failures fall
/// back to the raw atomic write so a save is never blocked by snapshots.
pub(crate) fn save_part_atomic(path: &Path, bytes: &[u8]) -> Option<io::Result<()>> {
    let result = save_part_checked(path, bytes)?;
    Some(finish_save(path, bytes, result))
}

fn service_for_parsed(parsed: &HostSavePath) -> Option<Arc<HostSaveService>> {
    if !parsed.program_dir.join("data").join("partidas").is_dir() {
        return None
    }
    service_for(&parsed.program_dir).ok()
}

fn canonical_service(path: &Path) -> Option<(Arc<HostSaveService>, HostSavePath)> {
    let parsed = parse_save_path(path)?;
    let service = service_for_parsed(&parsed)?;
    Some((service, parsed))
}

/// Deletes a canonical save part through the hooks so snapshots and revisions
/// stay consistent. Returns `None` for non-save paths and for backup failures,
/// letting the caller perform the raw deletion.
pub(crate) fn delete_save_part(path: &Path) -> Option<io::Result<()>> {
    let (service, parsed) = canonical_service(path)?;
    match service.remove_part(parsed.slot, parsed.part, SaveTrigger::Manual) {
        Ok(()) => Some(Ok(())),
        Err(error) if backup_failure(&error) => None,
        Err(error) => Some(Err(save_error_io(error))),
    }
}

/// Copies one canonical save part to another, or materializes a raw source
/// file as a canonical part. Returns `None` when the destination does not need
/// interception, or when the backup layer is unavailable.
pub(crate) fn copy_save_path(from: &Path, to: &Path) -> Option<io::Result<()>> {
    let from_parsed = parse_save_path(from);
    let to_parsed = parse_save_path(to)?;
    let service = service_for_parsed(&to_parsed)?;
    let to = to_parsed;
    match from_parsed {
        Some(from) if from.program_dir == to.program_dir => {
            match service.copy_part(from.slot, from.part, to.slot, to.part, SaveTrigger::Manual) {
                Ok(()) => Some(Ok(())),
                Err(error) if backup_failure(&error) => None,
                Err(error) => Some(Err(save_error_io(error))),
            }
        },
        _ => {
            let bytes = match fs::read(from) {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            match service.save_part(to.slot, to.part, &bytes, SaveTrigger::Manual) {
                Ok(_) => Some(Ok(())),
                Err(error) if backup_failure(&error) => None,
                Err(error) => Some(Err(save_error_io(error))),
            }
        },
    }
}

/// Renames a canonical save part, applying both ends in one commit when they
/// share a slot. Returns `None` when the caller should perform a raw rename.
pub(crate) fn rename_save_path(from: &Path, to: &Path) -> Option<io::Result<()>> {
    let from_parsed = parse_save_path(from);
    let to_parsed = parse_save_path(to);
    match (from_parsed, to_parsed) {
        (Some(from), Some(to)) => {
            let service = service_for_parsed(&to)?;
            if from.program_dir != to.program_dir {
                return None
            }
            match service.rename_part(from.slot, from.part, to.slot, to.part, SaveTrigger::Manual) {
                Ok(()) => Some(Ok(())),
                Err(error) if backup_failure(&error) => None,
                Err(error) => Some(Err(save_error_io(error))),
            }
        },
        (Some(from), None) => {
            let service = service_for_parsed(&from)?;
            let bytes = match service.load_part(from.slot, from.part) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    return Some(Err(io::Error::new(io::ErrorKind::NotFound, "save part not found")))
                },
                Err(error) => return Some(Err(save_error_io(error))),
            };
            if let Err(error) = super::save_guard::write_file_atomic(to, &bytes) {
                return Some(Err(error))
            }
            match service.remove_part(from.slot, from.part, SaveTrigger::Manual) {
                Ok(()) => Some(Ok(())),
                Err(error) if backup_failure(&error) => {
                    if let Some(path) = service.backend.live_path(from.slot, from.part) {
                        let _ = fs::remove_file(path);
                    }
                    Some(Ok(()))
                },
                Err(error) => Some(Err(save_error_io(error))),
            }
        },
        (None, Some(to)) => {
            let service = service_for_parsed(&to)?;
            let bytes = match fs::read(from) {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            match service.save_part(to.slot, to.part, &bytes, SaveTrigger::Manual) {
                Ok(_) => {
                    let _ = fs::remove_file(from);
                    Some(Ok(()))
                },
                Err(error) if backup_failure(&error) => None,
                Err(error) => Some(Err(save_error_io(error))),
            }
        },
        (None, None) => None,
    }
}



fn checked_slot(slot: i32) -> Result<SaveSlot, SaveError> {
    if (1..=u16::MAX as i32).contains(&slot) {
        Ok(SaveSlot(slot as u16))
    } else {
        Err(SaveError::rejected("invalid save slot"))
    }
}

/// Captures the live slot before a mutating operation (trade save, explicit
/// rollback checkpoints) without changing the selected save file.
pub(crate) fn capture_slot(program_dir: &Path, slot: i32) -> Result<Option<SnapshotRecord>, SaveError> {
    let slot = checked_slot(slot)?;
    let service = service_for(program_dir)?;
    if service.load(slot)?.is_none() {
        return Ok(None)
    }
    match service.capture(slot) {
        Ok(record) => Ok(Some(record)),
        Err(error) if matches!(error.kind(), save::ErrorKind::Unavailable | save::ErrorKind::Rejected) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Restores a slot after a failed mutation. Returns `None` when no save was
/// committed since the snapshot (nothing to roll back).
pub(crate) fn rollback_slot(
    program_dir: &Path,
    slot: i32,
    source: SnapshotId,
    reason: &str,
) -> Result<Option<save::RollbackReceipt>, SaveError> {
    let slot = checked_slot(slot)?;
    let service = service_for(program_dir)?;
    let record = service.snapshot_info(source)?;
    if record.snapshot.slot != slot {
        return Err(SaveError::rejected("snapshot belongs to another slot"))
    }
    let current = service.load(slot)?;
    let expected = current.as_ref().map(|image| image.revision).unwrap_or(0);
    if expected == record.snapshot.revision {
        return Ok(None)
    }
    service.rollback(slot, source, expected, reason).map(Some)
}

/// Captures the selected slot before an online session without mutating it.
pub(crate) fn begin_online_session(program_dir: &Path, slot: i32) -> Result<Option<SnapshotRecord>, SaveError> {
    let slot = checked_slot(slot)?;
    let service = service_for(program_dir)?;
    service.begin_session(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let path = std::env::temp_dir().join(format!(
                "reloaded-save-adapter-{}-{}-{}",
                std::process::id(),
                stamp,
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(path.join("data").join("partidas")).unwrap();
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

    fn config() -> SaveHookConfig {
        SaveHookConfig { rolling_snapshots: 10, min_free_bytes: 0, ..SaveHookConfig::default() }
    }

    fn service(dir: &TempDir) -> HostSaveService {
        HostSaveService::open_with_config(dir.path(), config()).unwrap()
    }

    fn live(dir: &TempDir, slot: u16, extension: &str) -> PathBuf {
        dir.path().join("data").join("partidas").join(format!("reloaded{slot}.{extension}"))
    }

    #[test]
    fn save_round_trip_preserves_bytes_and_rolls_back() {
        let dir = TempDir::new();
        let service = service(&dir);
        let slot = SaveSlot(4);
        let first = service.save_part(slot, SAVC, b"first", SaveTrigger::Manual).unwrap();
        assert_eq!(fs::read(live(&dir, 4, "savc")).unwrap(), b"first");

        let first_revision = first.commit.expect("commit").current;
        let mut target = None;
        for entry in service.snapshots(slot).unwrap() {
            if service.snapshot_info(entry.id).map(|record| record.snapshot.revision == first_revision).unwrap_or(false)
            {
                target = Some(entry.id);
            }
        }
        let target = target.expect("snapshot of the first save");

        service.save_part(slot, SAVC, b"second", SaveTrigger::Manual).unwrap();
        assert_eq!(fs::read(live(&dir, 4, "savc")).unwrap(), b"second");

        let receipt = service.rollback(slot, target, first_revision + 1, "host test").unwrap();
        assert_eq!(receipt.previous, first_revision + 1);
        assert_eq!(fs::read(live(&dir, 4, "savc")).unwrap(), b"first");
    }

    #[test]
    fn save_companions_are_restored_together() {
        let dir = TempDir::new();
        let service = service(&dir);
        let slot = SaveSlot(2);
        service.save_part(slot, SAVC, b"savc-one", SaveTrigger::Manual).unwrap();
        service.save_part(slot, NFO, b"nfo-one", SaveTrigger::Manual).unwrap();
        let last = service.save_part(slot, DATA, b"data-one", SaveTrigger::Manual).unwrap();
        let revision = last.commit.expect("commit").current;
        let target = service
            .snapshots(slot)
            .unwrap()
            .into_iter()
            .find(|entry| {
                service.snapshot_info(entry.id).map(|record| record.snapshot.revision == revision).unwrap_or(false)
            })
            .expect("snapshot");

        service.save_part(slot, SAVC, b"savc-two", SaveTrigger::Manual).unwrap();
        service.save_part(slot, NFO, b"nfo-two", SaveTrigger::Manual).unwrap();
        service.save_part(slot, DATA, b"data-two", SaveTrigger::Manual).unwrap();

        service.rollback(slot, target.id, revision + 3, "companions").unwrap();
        assert_eq!(fs::read(live(&dir, 2, "savc")).unwrap(), b"savc-one");
        assert_eq!(fs::read(live(&dir, 2, "nfo")).unwrap(), b"nfo-one");
        assert_eq!(fs::read(live(&dir, 2, "data")).unwrap(), b"data-one");
    }

    #[test]
    fn save_backups_stay_outside_the_live_slot() {
        let dir = TempDir::new();
        let service = service(&dir);
        service.save_part(SaveSlot(1), SAVC, b"payload", SaveTrigger::Manual).unwrap();
        let snapshots = service.snapshots(SaveSlot(1)).unwrap();
        assert!(!snapshots.is_empty());
        let root = dir.path().join("data").join("partidas");
        assert!(root.join(BACKUP_DIR).join("backups").exists());
        assert!(!live(&dir, 1, "savc").starts_with(root.join(BACKUP_DIR)));
    }

    #[test]
    fn save_path_parser_accepts_only_live_slot_parts() {
        let dir = TempDir::new();
        let parsed = parse_save_path(&live(&dir, 7, "savc")).expect("savc path");
        assert_eq!(parsed.slot, SaveSlot(7));
        assert_eq!(parsed.part, SAVC);
        assert!(parse_save_path(&live(&dir, 7, "savc.online")).is_none());
        assert!(parse_save_path(&live(&dir, 7, "txt")).is_none());
        assert!(parse_save_path(&dir.path().join("reloaded7.savc")).is_none());
        assert!(parse_save_path(&dir.path().join("data").join("partidas").join("other7.savc")).is_none());
    }

    #[test]
    fn save_guard_atomic_write_keeps_the_canonical_bytes() {
        let dir = TempDir::new();
        let path = live(&dir, 3, "savc");
        super::super::save_guard::atomic_write(&path, b"one").unwrap();
        super::super::save_guard::atomic_write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert!(!dir.path().join("data").join("partidas").join("reloaded3.savc.tmp").exists());
    }

    #[test]
    fn save_online_baseline_is_captured_without_touching_the_slot() {
        let dir = TempDir::new();
        let path = live(&dir, 5, "savc");
        fs::write(&path, b"before-online").unwrap();
        let snapshot = begin_online_session(dir.path(), 5).unwrap().expect("baseline snapshot");
        assert_eq!(fs::read(&path).unwrap(), b"before-online");
        assert!(snapshot.baseline);
        assert!(service_for(dir.path()).unwrap().verify(snapshot.snapshot.id).unwrap().valid);
    }

    #[test]
    fn save_rollback_to_empty_baseline_removes_the_slot() {
        let dir = TempDir::new();
        let service = service(&dir);
        let slot = SaveSlot(6);
        service.save_part(slot, SAVC, b"new-save", SaveTrigger::Manual).unwrap();
        let baseline = service
            .snapshots(slot)
            .unwrap()
            .into_iter()
            .find(|entry| entry.baseline)
            .expect("baseline");
        let receipt = service.rollback(slot, baseline.id, 1, "empty baseline").unwrap();
        assert_eq!(receipt.previous, 1);
        assert!(!live(&dir, 6, "savc").exists());
    }

    #[test]
    fn staged_text_write_commits_through_the_hooks() {
        let dir = TempDir::new();
        let service = service(&dir);
        let path = live(&dir, 9, "nfo");
        let mut handle =
            crate::gml::file::TextHandle::open(path.to_str().unwrap(), crate::gml::file::AccessMode::Write)
                .unwrap();
        handle.write_string(b"summary").unwrap();
        handle.write_newline().unwrap();
        handle.flush().unwrap();
        drop(handle);

        assert_eq!(fs::read(&path).unwrap(), b"summary\r\n");
        let image = service.load(SaveSlot(9)).unwrap().expect("committed image");
        assert_eq!(&image.parts[NFO][..], b"summary\r\n");
        assert!(!service.snapshots(SaveSlot(9)).unwrap().is_empty());
    }

    #[test]
    fn staged_text_append_keeps_the_existing_prefix_until_flush() {
        let dir = TempDir::new();
        let path = live(&dir, 10, "nfo");
        fs::write(&path, b"old").unwrap();
        let mut handle =
            crate::gml::file::TextHandle::open(path.to_str().unwrap(), crate::gml::file::AccessMode::Special)
                .unwrap();
        handle.write_string(b"-new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"old");
        handle.flush().unwrap();
        drop(handle);

        assert_eq!(fs::read(&path).unwrap(), b"old-new");
    }

    #[test]
    fn staged_binary_read_write_preserves_untouched_bytes() {
        let dir = TempDir::new();
        let path = live(&dir, 11, "data");
        fs::write(&path, b"abc").unwrap();
        let mut handle =
            crate::gml::file::BinaryHandle::open(path.to_str().unwrap(), crate::gml::file::AccessMode::Special)
                .unwrap();
        handle.seek(1).unwrap();
        handle.write_byte(b'Z').unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        handle.flush().unwrap();
        drop(handle);

        assert_eq!(fs::read(&path).unwrap(), b"aZc");
    }

    #[test]
    fn atomic_load_reads_the_committed_part() {
        let dir = TempDir::new();
        let service = service(&dir);
        service.save_part(SaveSlot(12), SAVC, b"payload", SaveTrigger::Manual).unwrap();
        let path = live(&dir, 12, "savc");
        let loaded = load_part_atomic(&path, SAVC).unwrap().unwrap().unwrap();
        assert_eq!(&loaded[..], b"payload");
        assert!(load_part_atomic(&path, NFO).is_none());
    }

    #[test]
    fn captured_slot_rolls_back_after_a_failed_sequence() {
        let dir = TempDir::new();
        let service = service(&dir);
        let slot = SaveSlot(13);
        service.save_part(slot, SAVC, b"one", SaveTrigger::Manual).unwrap();
        let record = capture_slot(dir.path(), 13).unwrap().expect("checkpoint");
        service.save_part(slot, SAVC, b"two", SaveTrigger::Manual).unwrap();

        let receipt =
            rollback_slot(dir.path(), 13, record.snapshot.id, "failed save").unwrap().expect("rollback");
        assert_eq!(receipt.previous, record.snapshot.revision + 1);
        assert_eq!(fs::read(live(&dir, 13, "savc")).unwrap(), b"one");
    }

    #[test]
    fn save_unknown_slot_is_rejected() {
        let dir = TempDir::new();
        assert!(begin_online_session(dir.path(), 0).is_err());
        assert!(begin_online_session(dir.path(), -1).is_err());
    }

    #[test]
    fn revisions_survive_restart_and_sidecar_loss() {
        let dir = TempDir::new();
        let root = dir.path().join("data").join("partidas");
        let slot = SaveSlot(15);
        {
            let first_service = service(&dir);
            let first = first_service.save_part(slot, SAVC, b"one", SaveTrigger::Manual).unwrap();
            let second = first_service.save_part(slot, SAVC, b"two", SaveTrigger::Manual).unwrap();
            assert_eq!(first.commit.expect("first commit").current, 1);
            assert_eq!(second.commit.expect("second commit").current, 2);
        }

        let reopened = service(&dir);
        let image = reopened.load(slot).unwrap().expect("persisted slot");
        assert_eq!(image.revision, 2);
        assert_eq!(&image.parts[SAVC][..], b"two");
        drop(reopened);

        fs::remove_file(root.join(BACKUP_DIR).join("revisions")).unwrap();
        let recovered = service(&dir);
        let image = recovered.load(slot).unwrap().expect("snapshot-derived slot");
        assert_eq!(image.revision, 2);
        let third = recovered.save_part(slot, SAVC, b"three", SaveTrigger::Manual).unwrap();
        assert_eq!(third.commit.expect("third commit").current, 3);
        assert_eq!(fs::read(live(&dir, 15, "savc")).unwrap(), b"three");
        drop(recovered);

        fs::write(root.join(BACKUP_DIR).join("revisions"), b"15 1\n").unwrap();
        let stale = service(&dir);
        assert_eq!(stale.load(slot).unwrap().expect("slot").revision, 3);
    }

    #[test]
    fn crash_leftovers_are_cleaned_on_reopen() {
        let dir = TempDir::new();
        let first_service = service(&dir);
        let staging = first_service.staging_dir();
        let nested = staging.join(".tmp-part-crash");
        fs::create_dir_all(nested.join("inner")).unwrap();
        fs::write(nested.join("inner").join("partial"), b"x").unwrap();
        fs::write(staging.join(".tmp-part-file"), b"y").unwrap();
        let stale = dir
            .path()
            .join("data")
            .join("partidas")
            .join("reloaded14.savc.tmp-999-1");
        fs::write(&stale, b"z").unwrap();
        drop(first_service);

        let reopened = service(&dir);
        assert!(!nested.exists());
        assert!(!staging.join(".tmp-part-file").exists());
        assert!(!stale.exists());
        assert!(fs::read_dir(&reopened.staging_dir()).unwrap().next().is_none());
    }

    #[test]
    fn interrupted_multi_part_commit_is_completed_on_reopen() {
        let dir = TempDir::new();
        let root = dir.path().join("data").join("partidas");
        let backend = HostSaveBackend::open(&root).unwrap();
        let slot = SaveSlot(16);
        let world = backend.world();
        let mut old = SaveImage::empty(HOST_FORMAT, CURRENT_IMAGE_SCHEMA, 1);
        old.parts.insert(SAVC.to_string(), Arc::from(&b"old-savc"[..]));
        old.parts.insert(DATA.to_string(), Arc::from(&b"old-data"[..]));
        backend
            .commit(world, slot, &old, CommitOptions { mode: SaveMode::Replace, allow_world_readable: false })
            .unwrap();

        let journal = backend
            .prepare_journal(
                slot,
                2,
                JOURNAL_COMMIT,
                vec![
                    (SAVC, Arc::from(&b"new-savc"[..])),
                    (NFO, Arc::from(&b"new-nfo"[..])),
                ],
                vec![DATA.to_string()],
            )
            .unwrap();
        journal.write().unwrap();
        let (part, temporary) = &journal.writes[0];
        let destination = backend.live_path(slot, part).unwrap();
        super::super::save_guard::commit_prepared_file(temporary, &destination).unwrap();
        drop(journal);
        drop(backend);

        let reopened = HostSaveBackend::open(&root).unwrap();
        let image = reopened.load(world, slot).unwrap().expect("recovered slot");
        assert_eq!(&image.parts[SAVC][..], b"new-savc");
        assert_eq!(&image.parts[NFO][..], b"new-nfo");
        assert!(!image.parts.contains_key(DATA));
        assert_eq!(fs::read(live(&dir, 16, "savc")).unwrap(), b"new-savc");
        assert!(!live(&dir, 16, "data").exists());
        assert!(!journal_path(&root, 16).exists());
    }

    #[test]
    fn backup_failure_falls_back_to_a_direct_write() {
        let dir = TempDir::new();
        let path = live(&dir, 17, "savc");
        let service = HostSaveService::open_with_config(
            dir.path(),
            SaveHookConfig { max_total_backup_bytes: 1, min_free_bytes: 0, ..SaveHookConfig::default() },
        )
        .unwrap();
        let error = service.save_part(SaveSlot(17), SAVC, b"payload", SaveTrigger::Manual).unwrap_err();
        assert_eq!(error.kind(), save::ErrorKind::Overloaded);
        assert!(!path.exists());

        finish_save(&path, b"payload", Err(error)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"payload");

        let hard = finish_save(&path, b"other", Err(SaveError::conflict("hard failure"))).unwrap_err();
        assert_eq!(hard.kind(), io::ErrorKind::Other);
        assert_eq!(fs::read(&path).unwrap(), b"payload");
    }

    #[test]
    fn file_copy_rename_and_delete_intercept_canonical_parts() {
        let _guard = crate::game::save_guard::GUARD_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        crate::game::save_guard::set_online_save_guard(false);
        let dir = TempDir::new();
        let service = service_for(dir.path()).unwrap();
        let slot = SaveSlot(18);
        service.save_part(slot, SAVC, b"savc", SaveTrigger::Manual).unwrap();
        service.save_part(slot, NFO, b"nfo", SaveTrigger::Manual).unwrap();
        let before = service.load(slot).unwrap().expect("slot").revision;

        let savc = live(&dir, 18, "savc");
        let nfo = live(&dir, 18, "nfo");
        let data = live(&dir, 18, "data");
        crate::gml::file::copy(savc.to_str().unwrap(), nfo.to_str().unwrap()).unwrap();
        assert_eq!(fs::read(&nfo).unwrap(), b"savc");
        let after_copy = service.load(slot).unwrap().expect("slot");
        assert_eq!(after_copy.revision, before + 1);

        crate::gml::file::rename(nfo.to_str().unwrap(), data.to_str().unwrap()).unwrap();
        assert!(!nfo.exists());
        assert_eq!(fs::read(&data).unwrap(), b"savc");
        let after_rename = service.load(slot).unwrap().expect("slot");
        assert_eq!(after_rename.revision, before + 2);
        assert!(!after_rename.parts.contains_key(NFO));
        assert_eq!(&after_rename.parts[DATA][..], b"savc");

        crate::gml::file::delete(data.to_str().unwrap()).unwrap();
        assert!(!data.exists());
        let after_delete = service.load(slot).unwrap().expect("slot");
        assert_eq!(after_delete.revision, before + 3);
        assert!(!after_delete.parts.contains_key(DATA));

        crate::gml::file::delete(savc.to_str().unwrap()).unwrap();
        assert!(!savc.exists());
        assert!(service.load(slot).unwrap().is_none());
    }

    #[test]
    fn legacy_rotation_through_non_canonical_names_is_captured() {
        let _guard = crate::game::save_guard::GUARD_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        crate::game::save_guard::set_online_save_guard(false);
        let dir = TempDir::new();
        let service = service_for(dir.path()).unwrap();
        let slot = SaveSlot(20);
        service.save_part(slot, SAVC, b"slot-bytes", SaveTrigger::Manual).unwrap();
        service.save_part(slot, NFO, b"nfo", SaveTrigger::Manual).unwrap();
        let before = service.load(slot).unwrap().expect("slot").revision;

        let savc = live(&dir, 20, "savc");
        let backup = dir
            .path()
            .join("data")
            .join("partidas")
            .join("reloaded20_bk.savc");
        crate::gml::file::rename(savc.to_str().unwrap(), backup.to_str().unwrap()).unwrap();
        assert!(!savc.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"slot-bytes");
        let rotated = service.load(slot).unwrap().expect("slot");
        assert_eq!(rotated.revision, before + 1);
        assert!(!rotated.parts.contains_key(SAVC));

        crate::gml::file::rename(backup.to_str().unwrap(), savc.to_str().unwrap()).unwrap();
        assert!(!backup.exists());
        assert_eq!(fs::read(&savc).unwrap(), b"slot-bytes");
        let restored = service.load(slot).unwrap().expect("slot");
        assert_eq!(restored.revision, before + 2);
        assert_eq!(&restored.parts[SAVC][..], b"slot-bytes");
    }

    #[test]
    fn non_canonical_copy_into_a_slot_is_captured() {
        let dir = TempDir::new();
        let service = service_for(dir.path()).unwrap();
        let slot = SaveSlot(19);
        let source = dir.path().join("incoming.bin");
        fs::write(&source, b"incoming").unwrap();
        let target = live(&dir, 19, "savc");
        crate::gml::file::copy(source.to_str().unwrap(), target.to_str().unwrap()).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"incoming");
        let image = service.load(slot).unwrap().expect("slot");
        assert_eq!(&image.parts[SAVC][..], b"incoming");
    }
}
