use crate::game::audio::GbAudioSettings;
use crate::render::{CrtSettings, GbSettings};
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

/// The master switch is opt-in: no `enabled.flag` means no mods are loaded. `disabled.flag` is
/// still honoured so an old "disabled" choice keeps winning if both files happen to exist.
fn master_enabled(root: &Path) -> bool {
    let mods = root.join("mods");
    mods.join("enabled.flag").is_file() && !mods.join("disabled.flag").is_file()
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

    let master_enabled = master_enabled(root);
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
        if master_enabled && entry.enabled {
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

    let active = if master_enabled { mods.iter().filter(|m| m.enabled).count() } else { 0 };
    let menu = mods_dir.join("menu.txt");
    let marker = mods_dir.join("active.flag");
    if active > 0 {
        let mut text = String::new();
        for entry in &mods {
            text.push_str(&format!("{}  [{}]\n", entry.name, if entry.enabled { "ON" } else { "OFF" }));
        }
        fs::write(&menu, text)?;
        fs::write(&marker, b"1")?;
        eprintln!("[mods] menu in-game disponible ({} activos)", active);
    } else {
        let _ = fs::remove_file(&menu);
        let _ = fs::remove_file(&marker);
    }
    Ok(())
}


/// Reads the settings of the first enabled mod whose manifest says `effect=crt`.
///
/// The effect itself lives in the renderer; this only turns the mod's text file into the small
/// parameter block the renderer understands. Missing keys keep their defaults, so a mod can tune
/// just the values it cares about.
pub fn crt_settings(root: &Path) -> Option<CrtSettings> {
    if !master_enabled(root) {
        return None;
    }
    let mods_dir = root.join("mods");
    let entries = fs::read_dir(&mods_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("mod.ini");
        if !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).ok()?;
        let mut settings = CrtSettings::default();
        let mut enabled = true;
        let mut is_crt = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            let value_lower = value.to_ascii_lowercase();
            match key.as_str() {
                "enabled" => enabled = !matches!(value_lower.as_str(), "0" | "false" | "no" | "off"),
                "effect" => is_crt = value_lower == "crt",
                "curvature" => settings.curvature = parse_f32(value, settings.curvature),
                "scanline" => settings.scanline = parse_f32(value, settings.scanline),
                "scanline_count" => settings.scanline_count = parse_f32(value, settings.scanline_count),
                "mask" => settings.mask = parse_f32(value, settings.mask),
                "aberration" => settings.aberration = parse_f32(value, settings.aberration),
                "vignette" => settings.vignette = parse_f32(value, settings.vignette),
                "blur" => settings.blur = parse_f32(value, settings.blur),
                "noise" => settings.noise = parse_f32(value, settings.noise),
                "brightness" => settings.brightness = parse_f32(value, settings.brightness),
                "saturation" => settings.saturation = parse_f32(value, settings.saturation),
                "glow" => settings.glow = parse_f32(value, settings.glow),
                "corner" => settings.corner = parse_f32(value, settings.corner),
                _ => (),
            }
        }
        if enabled && is_crt {
            eprintln!("[crt] configuracion encontrada en {}", path.display());
            return Some(settings);
        }
    }
    None
}

fn parse_f32(value: &str, fallback: f32) -> f32 {
    value.parse().unwrap_or(fallback)
}


/// Enables or disables a mod from inside the game. Only the manifest and the in-game menu are
/// touched; data overlays keep their startup behavior until the next run.
pub fn set_enabled(root: &Path, id: &str, enabled: bool) -> bool {
    let manifest = root.join("mods").join(id).join("mod.ini");
    let Ok(text) = fs::read_to_string(&manifest) else { return false };
    let mut out = String::new();
    let mut found = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !found && trimmed.split_once('=').is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("enabled")) {
            out.push_str(&format!("enabled={}\n", if enabled { 1 } else { 0 }));
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str(&format!("enabled={}\n", if enabled { 1 } else { 0 }));
    }
    if fs::write(&manifest, out).is_err() {
        return false;
    }
    refresh_menu(root);
    true
}

/// Rewrites `mods/menu.txt` from the manifests, so the in-game list reflects a live toggle.
pub fn refresh_menu(root: &Path) {
    let mods_dir = root.join("mods");
    let Ok(entries) = fs::read_dir(&mods_dir) else { return };
    let mut mods = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("mod.ini");
        if !manifest.is_file() {
            continue;
        }
        let (name, enabled) = parse_manifest(&manifest);
        mods.push((name, enabled));
    }
    mods.sort();
    let mut text = String::new();
    for (name, enabled) in &mods {
        text.push_str(&format!("{}  [{}]\n", name, if *enabled { "ON" } else { "OFF" }));
    }
    let _ = fs::write(mods_dir.join("menu.txt"), text);
    let marker = mods_dir.join("active.flag");
    if mods.iter().any(|(_, enabled)| *enabled) {
        let _ = fs::write(marker, b"1");
    } else {
        let _ = fs::remove_file(marker);
    }
}


/// Same as [`crt_settings`], but for a mod whose manifest says `effect=gb`.
pub fn gb_settings(root: &Path) -> Option<GbSettings> {
    if !master_enabled(root) {
        return None;
    }
    let mods_dir = root.join("mods");
    let entries = fs::read_dir(&mods_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("mod.ini");
        if !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).ok()?;
        let mut settings = GbSettings::default();
        let mut enabled = true;
        let mut is_gb = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "enabled" => {
                    enabled = !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")
                },
                "effect" => is_gb = value.eq_ignore_ascii_case("gb"),
                "strength" => settings.strength = parse_f32(value, settings.strength),
                "dither" => settings.dither = parse_f32(value, settings.dither),
                "contrast" => settings.contrast = parse_f32(value, settings.contrast),
                "brightness" => settings.brightness = parse_f32(value, settings.brightness),
                _ => (),
            }
        }
        if enabled && is_gb {
            eprintln!("[gb] configuracion encontrada en {}", path.display());
            return Some(settings);
        }
    }
    None
}

/// Whether a mod is currently enabled in its manifest.
pub fn is_enabled(root: &Path, id: &str) -> bool {
    if !master_enabled(root) {
        return false;
    }
    parse_manifest(&root.join("mods").join(id).join("mod.ini")).1
}


/// Reads the optional lo-fi audio settings of an enabled Game Boy mod.
pub fn gb_audio_settings(root: &Path) -> Option<GbAudioSettings> {
    if !master_enabled(root) {
        return None;
    }
    let mods_dir = root.join("mods");
    let entries = fs::read_dir(&mods_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("mod.ini");
        if !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).ok()?;
        let mut enabled = true;
        let mut audio = false;
        let mut settings = GbAudioSettings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "enabled" => {
                    enabled = !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")
                },
                "audio" => {
                    audio = matches!(value.to_ascii_lowercase().as_str(), "gb" | "gameboy" | "game boy" | "1" | "on" | "yes")
                },
                "audio_bits" => {
                    if let Ok(bits) = value.parse::<u32>() {
                        settings.bits = bits;
                    }
                },
                "audio_rate" => {
                    if let Ok(rate) = value.parse::<u32>() {
                        settings.sample_rate = rate;
                    }
                },
                "audio_mono" => {
                    settings.mono = !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")
                },
                _ => (),
            }
        }
        if enabled && audio {
            eprintln!("[gb-audio] configuracion encontrada en {}", path.display());
            return Some(settings);
        }
    }
    None
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
