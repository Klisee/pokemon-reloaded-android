use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

static SCRIPT_OVERRIDES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn overrides() -> &'static RwLock<HashMap<String, String>> {
    SCRIPT_OVERRIDES.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn script_override(name: &str) -> Option<String> {
    overrides().read().ok()?.get(name).cloned()
}

struct ModEntry {
    id: String,
    name: String,
    enabled: bool,
    path: PathBuf,
}

pub fn prepare(game_directory: &str) {
    if let Err(err) = try_prepare(Path::new(game_directory)) {
        eprintln!("[mods] error: {}", err);
    }
}

fn try_prepare(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mods_dir = root.join("mods");
    if !mods_dir.is_dir() {
        return Ok(());
    }

    let backup_root = mods_dir.join(".backup");
    let mut mods = Vec::new();
    for entry in fs::read_dir(&mods_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let manifest = path.join("mod.ini");
        if !manifest.is_file() {
            continue;
        }
        let (name, enabled) = parse_manifest(&manifest);
        mods.push(ModEntry {
            id: entry.file_name().to_string_lossy().to_string(),
            name,
            enabled,
            path,
        });
    }
    mods.sort_by(|a, b| a.id.cmp(&b.id));

    {
        let mut map = overrides().write().map_err(|_| "script lock poisoned")?;
        map.clear();
    }

    for entry in &mods {
        if entry.enabled {
            let backup = backup_root.join(&entry.id);
            let installed = apply_overlay(&entry.path, root, &backup)?;
            let scripts = load_scripts(&entry.path)?;
            if !scripts.is_empty() {
                let mut map = overrides().write().map_err(|_| "script lock poisoned")?;
                for (name, source) in scripts {
                    map.insert(name, source);
                }
            }
            eprintln!("[mods] {} ({}) activo: {} archivos", entry.name, entry.id, installed);
        } else {
            restore_overlay(root, &backup_root.join(&entry.id))?;
            eprintln!("[mods] {} ({}) desactivado", entry.name, entry.id);
        }
    }
    Ok(())
}

fn parse_manifest(path: &Path) -> (String, bool) {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut name = None;
    let mut enabled = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim().to_ascii_lowercase().as_str() {
            "name" => name = Some(value.trim().to_string()),
            "enabled" => {
                enabled = !matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")
            },
            _ => (),
        }
    }
    (
        name.unwrap_or_else(|| {
            path.parent()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        enabled,
    )
}

fn load_scripts(mod_dir: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let dir = mod_dir.join("scripts");
    let mut out = HashMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("gml") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        out.insert(name.to_string(), fs::read_to_string(path)?);
    }
    Ok(out)
}

fn apply_overlay(mod_dir: &Path, game_root: &Path, backup: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let src = mod_dir.join("data");
    if !src.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    copy_tree(&src, &game_root.join("data"), &backup.join("data"), &mut count)?;
    Ok(count)
}

fn copy_tree(src: &Path, dst: &Path, backup: &Path, count: &mut usize) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to, &backup.join(entry.file_name()), count)?;
        } else if from.is_file() {
            if to.exists() {
                fs::create_dir_all(backup.parent().unwrap_or(backup))?;
                if !backup.exists() {
                    fs::copy(&to, backup)?;
                }
            }
            fs::copy(&from, &to)?;
            *count += 1;
        }
    }
    Ok(())
}

fn restore_overlay(game_root: &Path, backup: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let src = backup.join("data");
    if !src.is_dir() {
        return Ok(());
    }
    restore_tree(&src, &game_root.join("data"))?;
    Ok(())
}

fn restore_tree(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            restore_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
