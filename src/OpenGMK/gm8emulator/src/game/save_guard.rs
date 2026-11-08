//! Backups and atomic writes for save files.

use crate::compat::f3r;
use reloaded_cleanroom::save::{RollbackReceipt, SaveError, SnapshotId, SnapshotRecord};
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SAVE_EXTENSIONS: [&str; 3] = f3r::save::EXTENSIONS;
const MAX_TEMP_ATTEMPTS: u32 = 10_000;

static ONLINE_SAVE_GUARD: AtomicBool = AtomicBool::new(false);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
pub(crate) static GUARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// While an online battle is active, writes to the current save slot are
/// redirected to `<name>.online` instead of touching the real save. The game
/// keeps running; the pre-battle snapshot is the source of truth.
pub(crate) fn set_online_save_guard(active: bool) {
    ONLINE_SAVE_GUARD.store(active, Ordering::Relaxed);
}

pub(crate) fn redirect_online_write(path: &str) -> Option<String> {
    if !ONLINE_SAVE_GUARD.load(Ordering::Relaxed) {
        return None
    }

    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if f3r::save::is_online_path(&normalized) {
        return None
    }
    if f3r::save::is_save_path(&normalized) {
        Some(format!("{path}{}", f3r::save::ONLINE_SUFFIX))
    } else {
        None
    }
}

pub(crate) fn cleanup_online_saves(program_dir: &Path) -> io::Result<()> {
    let partidas = program_dir.join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
    let entries = match fs::read_dir(partidas) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|extension| extension == f3r::save::ONLINE_EXT)
        {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

/// Copies the save slot to the pre-online backup directory before an online session.
///
/// Returns the timestamped backup directory, or `None` when the primary save file
/// does not exist. Missing sidecar files are skipped.
pub(crate) fn backup_before_online(program_dir: &Path, slot: i32) -> io::Result<Option<PathBuf>> {
    let partidas = program_dir.join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
    let stem = f3r::save::slot_stem(slot);
    let savc = partidas.join(f3r::save::slot_file_name(slot, f3r::save::EXT_SAVC));

    if !savc.is_file() {
        return Ok(None);
    }

    let preonline = partidas.join(f3r::save::PREONLINE);
    fs::create_dir_all(&preonline)?;

    let backup_dir = create_unique_dir(
        &preonline,
        &format!("{stem}{}{}", f3r::save::PREONLINE_INFIX, timestamp()),
    )?;

    let result = (|| {
        for extension in SAVE_EXTENSIONS {
            let source = partidas.join(format!("{stem}.{extension}"));
            if !source.is_file() {
                continue;
            }

            let destination = backup_dir.join(format!("{stem}.{extension}"));
            match atomic_copy(&source, &destination) {
                Ok(()) => {},
                Err(error)
                    if extension != f3r::save::EXT_SAVC && error.kind() == ErrorKind::NotFound =>
                {
                    continue;
                },
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&backup_dir);
        return Err(error);
    }

    if let Err(error) = super::reloaded_save::begin_online_session(program_dir, slot) {
        eprintln!("[save] no se pudo crear el snapshot previo a online: {error}");
    }

    Ok(Some(backup_dir))
}

/// Captures the live slot before a mutating save that must be rollback-able.
pub(crate) fn capture_before_save(program_dir: &Path, slot: i32) -> Result<Option<SnapshotRecord>, SaveError> {
    super::reloaded_save::capture_slot(program_dir, slot)
}

/// Restores the slot captured by [`capture_before_save`] after a failed save.
pub(crate) fn rollback_after_failure(
    program_dir: &Path,
    slot: i32,
    source: SnapshotId,
    reason: &str,
) -> Result<Option<RollbackReceipt>, SaveError> {
    super::reloaded_save::rollback_slot(program_dir, slot, source, reason)
}

/// Runs one mutating save under a rollback checkpoint.
///
/// The checkpoint is captured before `save` runs; if it cannot be captured the
/// save is not attempted, so a failure cannot leave an untracked partial save.
/// When `save` fails the slot is restored, and a failed restore is appended to
/// the returned reason instead of being hidden.
pub(crate) fn transactional_save<F>(
    program_dir: &Path,
    slot: i32,
    reason: &str,
    save: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    let baseline = if slot > 0 {
        match capture_before_save(program_dir, slot) {
            Ok(record) => record,
            Err(error) => {
                return Err(format!("no se pudo preparar el guardado: {error}"))
            },
        }
    } else {
        None
    };

    match save() {
        Ok(()) => Ok(()),
        Err(error) => match baseline {
            Some(record) => {
                match rollback_after_failure(program_dir, slot, record.snapshot.id, reason) {
                    Ok(Some(receipt)) => {
                        eprintln!(
                            "[save] guardado fallido revertido a la revision {}",
                            receipt.current
                        );
                        Err(error)
                    },
                    Ok(None) => Err(error),
                    Err(rollback_error) => {
                        let combined = format!("{error}; rollback failed: {rollback_error}");
                        eprintln!("[save] no se pudo revertir el guardado: {combined}");
                        Err(combined)
                    },
                }
            },
            None => Err(error),
        },
    }
}

/// Writes `bytes` to `path` through a temporary file, syncs it and renames it
/// into place. Canonical save-slot files also go through the clean-room save
/// hooks so backups and rollback stay available.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(result) = super::reloaded_save::save_part_atomic(path, bytes) {
        return result
    }
    write_file_atomic(path, bytes)
}

/// Raw atomic write used by the save adapter itself.
pub(crate) fn write_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = prepare_file_atomic(path, bytes)?;
    commit_prepared_file(&temporary, path)
}

/// Writes `bytes` to a unique temporary beside `path` and syncs it. The
/// destination is untouched until [`commit_prepared_file`] renames it.
pub(crate) fn prepare_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let mut last_error = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let temporary = unique_temporary_path(path);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                last_error = Some(error);
                continue
            },
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            file.flush()?;
            file.sync_all()
        })();
        drop(file);
        match result {
            Ok(()) => return Ok(temporary),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error)
            },
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::new(ErrorKind::AlreadyExists, "could not create a unique temp file")))
}

/// Renames a file prepared by [`prepare_file_atomic`] into place and syncs the
/// parent directory. On failure the prepared file is removed.
pub(crate) fn commit_prepared_file(temporary: &Path, path: &Path) -> io::Result<()> {
    match replace_file(temporary, path) {
        Ok(()) => {
            sync_parent_dir(path);
            Ok(())
        },
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(error)
        },
    }
}

/// Copies `src` to `dst` atomically.
pub(crate) fn atomic_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let temporary = unique_temporary_path(dst);

    let result = (|| {
        let mut input = File::open(src)?;
        let mut output = File::create(&temporary)?;
        io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        drop(output);
        replace_file(&temporary, dst)
    })();

    match result {
        Ok(()) => {
            sync_parent_dir(dst);
            Ok(())
        },
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        },
    }
}

fn create_unique_dir(parent: &Path, stem: &str) -> io::Result<PathBuf> {
    for attempt in 0..10_000u32 {
        let name = if attempt == 0 { stem.to_string() } else { format!("{stem}_{attempt}") };
        let candidate = parent.join(name);

        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(ErrorKind::AlreadyExists, "could not create a unique backup directory"))
}

fn unique_temporary_path(path: &Path) -> PathBuf {
    let counter = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    parent_dir(path).join(format!(
        "{file_name}{}{}-{counter}",
        f3r::save::TEMP_SUFFIX,
        std::process::id()
    ))
}

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

pub(crate) fn sync_parent_dir(path: &Path) {
    if let Ok(directory) = File::open(parent_dir(path)) {
        let _ = directory.sync_all();
    }
}

fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}_{:09}", now.as_secs(), now.subsec_nanos())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(first_error)
            if destination.exists()
                && matches!(first_error.kind(), ErrorKind::AlreadyExists | ErrorKind::PermissionDenied) =>
        {
            let backup = parent_dir(destination).join(format!(
                "{}{}{}",
                destination.file_name().map(|name| name.to_string_lossy()).unwrap_or_default(),
                f3r::save::WINDOWS_BACKUP_INFIX,
                timestamp()
            ));

            fs::rename(destination, &backup)?;

            match fs::rename(source, destination) {
                Ok(()) => {
                    let _ = fs::remove_file(&backup);
                    Ok(())
                },
                Err(error) => {
                    let _ = fs::rename(&backup, destination);
                    Err(error)
                },
            }
        },
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let path = std::env::temp_dir().join(format!("gm8-save-guard-{}-{label}-{stamp}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_fixture(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("write test fixture");
    }

    fn native_path(name: &str) -> String {
        format!("{}\\{}\\{name}", f3r::save::ROOT, f3r::save::PARTIDAS)
    }

    fn slot_name(slot: i32, extension: &str) -> String {
        f3r::save::slot_file_name(slot, extension)
    }

    #[test]
    fn online_guard_redirects_save_writes_and_cleanup() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        set_online_save_guard(true);
        let savc = native_path(&slot_name(1, f3r::save::EXT_SAVC));
        let nfo = native_path(&slot_name(1, f3r::save::EXT_NFO));
        let online = format!("{savc}{}", f3r::save::ONLINE_SUFFIX);
        assert_eq!(redirect_online_write(&savc).as_deref(), Some(online.as_str()));
        assert_eq!(
            redirect_online_write(&nfo).as_deref(),
            Some(format!("{nfo}{}", f3r::save::ONLINE_SUFFIX).as_str())
        );
        assert_eq!(redirect_online_write(&online), None);
        assert_eq!(redirect_online_write(&format!("{savc}.txt")), None);

        let temp = TestDir::new("online-guard");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).expect("create partidas");
        let live = partidas.join(slot_name(1, f3r::save::EXT_SAVC));
        let overlay = partidas.join(format!(
            "{}{}",
            slot_name(1, f3r::save::EXT_SAVC),
            f3r::save::ONLINE_SUFFIX
        ));
        write_fixture(&overlay, b"online");
        write_fixture(&live, b"real");
        cleanup_online_saves(temp.path()).expect("cleanup");
        assert!(!overlay.exists());
        assert!(live.exists());

        set_online_save_guard(false);
        assert_eq!(redirect_online_write(&savc), None);
    }

    #[test]
    fn online_guard_redirects_save_mutations() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let temp = TestDir::new("online-mutations");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).expect("create partidas");
        let live = partidas.join(slot_name(5, f3r::save::EXT_SAVC));
        let overlay = partidas.join(format!(
            "{}{}",
            slot_name(5, f3r::save::EXT_SAVC),
            f3r::save::ONLINE_SUFFIX
        ));
        write_fixture(&live, b"real");
        write_fixture(&overlay, b"overlay");

        set_online_save_guard(true);
        crate::gml::file::delete(live.to_str().unwrap()).unwrap();
        assert!(!overlay.exists());
        assert_eq!(fs::read(&live).unwrap(), b"real");

        write_fixture(&overlay, b"second");
        let nfo = partidas.join(slot_name(5, f3r::save::EXT_NFO));
        let nfo_overlay = partidas.join(format!(
            "{}{}",
            slot_name(5, f3r::save::EXT_NFO),
            f3r::save::ONLINE_SUFFIX
        ));
        crate::gml::file::copy(live.to_str().unwrap(), nfo.to_str().unwrap()).unwrap();
        assert!(!nfo.exists());
        assert_eq!(fs::read(&nfo_overlay).unwrap(), b"second");

        let data = partidas.join(slot_name(5, f3r::save::EXT_DATA));
        let data_overlay = partidas.join(format!(
            "{}{}",
            slot_name(5, f3r::save::EXT_DATA),
            f3r::save::ONLINE_SUFFIX
        ));
        crate::gml::file::rename(nfo.to_str().unwrap(), data.to_str().unwrap()).unwrap();
        assert!(!nfo_overlay.exists());
        assert!(!data.exists());
        assert_eq!(fs::read(&data_overlay).unwrap(), b"second");
        assert_eq!(fs::read(&live).unwrap(), b"real");
        set_online_save_guard(false);
    }

    #[test]
    fn atomic_write_replaces_existing_contents() {
        let temp = TestDir::new("atomic-write");
        let path = temp.path().join("save.dat");
        write_fixture(&path, b"old");

        atomic_write(&path, b"new").expect("atomic write");

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!temp.path().join(format!("save.dat{}", f3r::save::TEMP_SUFFIX)).exists());
    }

    #[test]
    fn atomic_copy_replaces_existing_contents() {
        let temp = TestDir::new("atomic-copy");
        let source = temp.path().join("source.dat");
        let destination = temp.path().join("destination.dat");
        write_fixture(&source, b"source bytes");
        write_fixture(&destination, b"old destination");

        atomic_copy(&source, &destination).expect("atomic copy");

        assert_eq!(fs::read(&destination).unwrap(), b"source bytes");
        assert!(!temp.path().join(format!("destination.dat{}", f3r::save::TEMP_SUFFIX)).exists());
    }

    fn live_slot_path(temp: &TestDir, slot: i32) -> PathBuf {
        temp.path()
            .join(f3r::save::ROOT)
            .join(f3r::save::PARTIDAS)
            .join(slot_name(slot, f3r::save::EXT_SAVC))
    }

    fn prepare_live_slot(temp: &TestDir, slot: i32, bytes: &[u8]) -> PathBuf {
        let path = live_slot_path(temp, slot);
        fs::create_dir_all(path.parent().unwrap()).expect("create partidas");
        write_fixture(&path, bytes);
        path
    }

    #[test]
    fn transactional_save_commits_when_the_save_succeeds() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        set_online_save_guard(false);
        let temp = TestDir::new("transactional-ok");
        let path = prepare_live_slot(&temp, 31, b"before");
        let mut called = false;

        transactional_save(temp.path(), 31, "trade save", || {
            called = true;
            atomic_write(&path, b"after").map_err(|error| error.to_string())
        })
        .expect("transactional save");

        assert!(called);
        assert_eq!(fs::read(&path).unwrap(), b"after");
    }

    #[test]
    fn transactional_save_restores_the_slot_after_a_partial_save_failure() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        set_online_save_guard(false);
        let temp = TestDir::new("transactional-failure");
        let path = prepare_live_slot(&temp, 32, b"before");

        let outcome = transactional_save(temp.path(), 32, "trade save", || {
            atomic_write(&path, b"partial").map_err(|error| error.to_string())?;
            Err("disk full".into())
        });

        assert_eq!(outcome, Err("disk full".into()));
        assert_eq!(fs::read(&path).unwrap(), b"before");
    }

    #[test]
    fn transactional_save_removes_a_new_slot_after_a_failed_save() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        set_online_save_guard(false);
        let temp = TestDir::new("transactional-new-slot");
        let path = live_slot_path(&temp, 33);
        fs::create_dir_all(path.parent().unwrap()).expect("create partidas");
        assert!(!path.exists());

        let outcome = transactional_save(temp.path(), 33, "trade save", || {
            atomic_write(&path, b"partial").map_err(|error| error.to_string())?;
            Err("disk full".into())
        });

        assert_eq!(outcome, Err("disk full".into()));
        assert!(!path.exists());
    }

    #[test]
    fn transactional_save_does_not_run_without_a_checkpoint() {
        let temp = TestDir::new("transactional-prepare");
        let not_a_directory = temp.path().join("save-root");
        write_fixture(&not_a_directory, b"not a directory");
        let mut called = false;

        let outcome = transactional_save(&not_a_directory, 34, "trade save", || {
            called = true;
            Ok(())
        });

        assert!(!called);
        assert!(outcome
            .unwrap_err()
            .starts_with("no se pudo preparar el guardado:"));
    }

    #[test]
    fn transactional_save_reports_a_failed_rollback() {
        let _guard = GUARD_TEST_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        set_online_save_guard(false);
        let temp = TestDir::new("transactional-rollback-failure");
        let path = prepare_live_slot(&temp, 35, b"before");
        let backup_root = temp
            .path()
            .join(f3r::save::ROOT)
            .join(f3r::save::PARTIDAS)
            .join(".reloaded-save");

        let outcome = transactional_save(temp.path(), 35, "trade save", || {
            fs::write(&path, b"after").expect("write partial save");
            fs::remove_dir_all(&backup_root).expect("remove rollback checkpoint");
            Err("disk full".into())
        });

        let error = outcome.unwrap_err();
        assert!(error.starts_with("disk full; rollback failed:"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"after");
    }

    #[test]
    fn backup_returns_none_when_primary_file_is_missing() {
        let temp = TestDir::new("missing-savc");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).unwrap();
        write_fixture(&partidas.join(slot_name(3, f3r::save::EXT_NFO)), b"nfo");

        let backup = backup_before_online(temp.path(), 3).expect("backup");

        assert!(backup.is_none());
        assert!(!partidas.join(f3r::save::PREONLINE).exists());
    }

    #[test]
    fn backup_copies_all_present_files() {
        let temp = TestDir::new("backup-all");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).unwrap();
        for extension in f3r::save::EXTENSIONS {
            write_fixture(&partidas.join(slot_name(9, extension)), extension.as_bytes());
        }

        let backup = backup_before_online(temp.path(), 9).expect("backup").expect("backup directory");
        let name = backup.file_name().unwrap().to_string_lossy();

        assert!(name.starts_with(&format!(
            "{}{}",
            f3r::save::slot_stem(9),
            f3r::save::PREONLINE_INFIX
        )));
        assert!(backup.starts_with(partidas.join(f3r::save::PREONLINE)));
        for extension in f3r::save::EXTENSIONS {
            assert_eq!(
                fs::read(backup.join(slot_name(9, extension))).unwrap(),
                extension.as_bytes()
            );
        }
        assert!(!backup
            .join(format!("{}{}", slot_name(9, f3r::save::EXT_SAVC), f3r::save::TEMP_SUFFIX))
            .exists());
    }

    #[test]
    fn backup_skips_missing_optional_files() {
        let temp = TestDir::new("backup-savc-only");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).unwrap();
        let savc = slot_name(2, f3r::save::EXT_SAVC);
        write_fixture(&partidas.join(&savc), b"only savc");

        let backup = backup_before_online(temp.path(), 2).expect("backup").expect("backup directory");

        assert_eq!(fs::read(backup.join(&savc)).unwrap(), b"only savc");
        assert!(!backup.join(slot_name(2, f3r::save::EXT_NFO)).exists());
        assert!(!backup.join(slot_name(2, f3r::save::EXT_DATA)).exists());
    }

    #[test]
    fn repeated_backups_do_not_overwrite_each_other() {
        let temp = TestDir::new("backup-twice");
        let partidas = temp.path().join(f3r::save::ROOT).join(f3r::save::PARTIDAS);
        fs::create_dir_all(&partidas).unwrap();
        let savc = slot_name(4, f3r::save::EXT_SAVC);
        write_fixture(&partidas.join(&savc), b"first");

        let first = backup_before_online(temp.path(), 4).expect("first backup").expect("first backup directory");
        write_fixture(&partidas.join(&savc), b"second");
        let second = backup_before_online(temp.path(), 4).expect("second backup").expect("second backup directory");

        assert_ne!(first, second);
        assert_eq!(fs::read(first.join(&savc)).unwrap(), b"first");
        assert_eq!(fs::read(second.join(&savc)).unwrap(), b"second");
    }

}
