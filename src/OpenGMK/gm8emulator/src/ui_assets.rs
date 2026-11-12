//! Exports the art the desktop launcher shows on its save-slot cards.
//!
//! The launcher reads the game's `.nfo` summaries and needs two pictures per slot: the player
//! character (`prota_<sprite>.png`) and the current Pokémon (`pkmn_<id>.png`). Both are exported
//! as plain PNGs into `data/cache/ui/`, either on demand (`--ui-assets DIR`) or once when the game
//! starts and the files are missing or stale.
//!
//! Sprites embedded in the `.exe` store their pixels as BGRA (the same order `.gmspr` files use),
//! while `image::RgbaImage` expects RGBA, so the red and blue channels are swapped before saving.

use std::{
    fs,
    io::{self, BufReader},
    path::{Path, PathBuf},
    time::SystemTime,
};

use image::{codecs::gif::GifDecoder, AnimationDecoder, ImageFormat, RgbaImage};

/// Where the running game leaves the exported art, relative to the game directory.
pub const CACHE_DIR: &str = "data/cache/ui";

/// A save slot and the two asset ids its `.nfo` points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotArt {
    /// Slot number from `reloaded<N>.nfo`.
    pub slot: u32,
    /// The `.nfo` this slot was read from.
    pub nfo: PathBuf,
    /// Sprite id of the player character (`sprite_prota`, 5th non-empty line).
    pub sprite_id: i32,
    /// Pokémon id of the current party lead (`pkmn_actual`, 6th non-empty line).
    pub pkmn_id: i32,
}

/// Parses a `reloaded<N>.nfo` summary and returns `(sprite_id, pkmn_id)`.
///
/// The files use CRLF, occasionally have blank lines, and are not guaranteed to be UTF-8 (the
/// name line may contain Shift-JIS), so only the two numeric lines we need are interpreted. The
/// first four non-empty lines are name, seen, caught and place; lines 5 and 6 are the ids.
pub fn parse_slot_nfo(text: &str) -> Option<(i32, i32)> {
    let mut lines = text
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty());
    // name, seen, caught, place
    lines.next()?;
    lines.next()?;
    lines.next()?;
    lines.next()?;
    let sprite_id = lines.next()?.parse::<i32>().ok()?;
    let pkmn_id = lines.next()?.parse::<i32>().ok()?;
    Some((sprite_id, pkmn_id))
}

/// Lists the `data/partidas/reloaded<N>.nfo` summaries of a game directory, sorted by slot.
///
/// Backup names such as `reloaded1_bk.nfo` are skipped because their suffix is not numeric.
pub fn collect_slots(game_dir: &Path) -> io::Result<Vec<SlotArt>> {
    let dir = game_dir.join("data").join("partidas");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut slots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        let Some(stem) = file_name.strip_suffix(".nfo") else { continue };
        let Some(number) = stem.strip_prefix("reloaded") else { continue };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue
        }
        let Ok(slot) = number.parse::<u32>() else { continue };

        let text = fs::read(&path)?;
        let text = String::from_utf8_lossy(&text);
        let Some((sprite_id, pkmn_id)) = parse_slot_nfo(&text) else {
            eprintln!("[ui-assets] {}: no se pudieron leer los ids de sprite/pokemon", path.display());
            continue
        };
        slots.push(SlotArt { slot, nfo: path, sprite_id, pkmn_id });
    }
    slots.sort_by_key(|slot| slot.slot);
    Ok(slots)
}

/// Exports every slot's PNGs, parsing the sprites out of the `.exe`, and returns the number of
/// files written (fresh files are skipped). `game_dir` is the directory containing `data/`.
pub fn export_slot_art(game_dir: &Path, exe_path: &Path, out_dir: &Path) -> io::Result<usize> {
    let mut bytes = fs::read(exe_path)?;
    let assets = gm8exe::reader::from_exe(&mut bytes, None::<fn(&str)>, false, true)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("failed to parse exe: {error}")))?;
    let mut sprite_frame = |id: i32| sprite_rgba(&assets.sprites, id);
    export_slots(game_dir, exe_path, out_dir, &mut sprite_frame)
}

/// Same as [`export_slot_art`] but returns no count; used by the `--ui-assets` command line flag.
pub fn export_all(game_dir: &Path, exe_path: &Path, out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let written = export_slot_art(game_dir, exe_path, out_dir)?;
    println!("ui-assets: {} PNG(s) exportados a {}", written, out_dir.display());
    Ok(())
}

/// Exports every slot using sprites from an already parsed `.exe`, avoiding a second parse at
/// startup. `assets` is left untouched. A game without slot summaries is left alone.
pub fn export_from_assets(
    assets: &gm8exe::GameAssets,
    game_dir: &Path,
    exe_path: &Path,
    out_dir: &Path,
) -> io::Result<usize> {
    let mut sprite_frame = |id: i32| sprite_rgba(&assets.sprites, id);
    export_slots(game_dir, exe_path, out_dir, &mut sprite_frame)
}

fn export_slots(
    game_dir: &Path,
    exe_path: &Path,
    out_dir: &Path,
    sprite_frame: &mut dyn FnMut(i32) -> Option<(u32, u32, Vec<u8>)>,
) -> io::Result<usize> {
    let slots = collect_slots(game_dir)?;
    if slots.is_empty() {
        return Ok(0)
    }
    fs::create_dir_all(out_dir)?;
    let exe_modified = modified(exe_path);
    let mut written = 0;

    for slot in &slots {
        let prota_path = out_dir.join(format!("prota_{}.png", slot.sprite_id));
        let nfo_modified = modified(&slot.nfo);
        if slot.sprite_id >= 0 && !is_fresh(&prota_path, &[nfo_modified, exe_modified]) {
            match sprite_frame(slot.sprite_id) {
                Some((width, height, rgba)) => match save_png(&prota_path, width, height, rgba) {
                    Ok(()) => written += 1,
                    Err(error) => {
                        eprintln!("[ui-assets] {}: no se pudo escribir: {}", prota_path.display(), error)
                    },
                },
                None => eprintln!(
                    "[ui-assets] partida {}: el sprite {} no existe en el .exe",
                    slot.slot, slot.sprite_id
                ),
            }
        }

        let pkmn_path = out_dir.join(format!("pkmn_{}.png", slot.pkmn_id));
        let Some((gif_path, gif_modified)) = find_pokemon(game_dir, slot.pkmn_id) else {
            eprintln!("[ui-assets] partida {}: no hay GIF para el pokemon {}", slot.slot, slot.pkmn_id);
            continue
        };
        if !is_fresh(&pkmn_path, &[Some(gif_modified)]) {
            match first_gif_frame(&gif_path) {
                Ok(image) => match save_image(&pkmn_path, image) {
                    Ok(()) => written += 1,
                    Err(error) => {
                        eprintln!("[ui-assets] {}: no se pudo escribir: {}", pkmn_path.display(), error)
                    },
                },
                Err(error) => eprintln!("[ui-assets] {}: {}", gif_path.display(), error),
            }
        }
    }

    Ok(written)
}

/// The first frame of an exe sprite as RGBA, or `None` when the id has no sprite/frame.
fn sprite_rgba(sprites: &[Option<Box<gm8exe::asset::sprite::Sprite>>], id: i32) -> Option<(u32, u32, Vec<u8>)> {
    if id < 0 {
        return None
    }
    let sprite = sprites.get(id as usize)?.as_ref()?;
    let frame = sprite.frames.first()?;
    let mut data = frame.data.to_vec();
    // Exe frames are BGRA; PNG/RgbaImage want RGBA.
    crate::util::bgra2rgba(&mut data);
    Some((frame.width, frame.height, data))
}

/// Finds `data/pkmn/<id>.gif`, falling back to `data/pkmn_shiny/<id>.gif`.
fn find_pokemon(game_dir: &Path, id: i32) -> Option<(PathBuf, SystemTime)> {
    if id < 0 {
        return None
    }
    let name = format!("{id}.gif");
    for folder in ["pkmn", "pkmn_shiny"] {
        let path = game_dir.join("data").join(folder).join(&name);
        if let Some(modified) = modified(&path) {
            return Some((path, modified))
        }
    }
    None
}

/// The first frame of a GIF as RGBA. Only the first frame is needed for a card thumbnail.
fn first_gif_frame(path: &Path) -> io::Result<RgbaImage> {
    let file = fs::File::open(path)?;
    let decoder = GifDecoder::new(BufReader::new(file)).map_err(image_error)?;
    let frame = decoder
        .into_frames()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GIF has no frames"))?
        .map_err(image_error)?;
    Ok(frame.into_buffer())
}

/// Writes a PNG atomically through a temporary file, so a killed process leaves no half image.
fn save_png(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> io::Result<()> {
    let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, format!("pixel buffer does not match {width}x{height}"))
    })?;
    save_image(path, image)
}

fn save_image(path: &Path, image: RgbaImage) -> io::Result<()> {
    let tmp = path.with_extension("png.tmp");
    image.save_with_format(&tmp, ImageFormat::Png).map_err(image_error)?;
    if let Err(error) = fs::rename(&tmp, path) {
        // Windows refuses to rename over an existing file, so replace it explicitly.
        if fs::remove_file(path).is_ok() {
            if fs::rename(&tmp, path).is_ok() {
                return Ok(())
            }
        }
        fs::remove_file(&tmp).ok();
        return Err(error)
    }
    Ok(())
}

/// Whether `path` exists and is at least as new as every source it was generated from.
fn is_fresh(path: &Path, sources: &[Option<SystemTime>]) -> bool {
    let Ok(metadata) = fs::metadata(path) else { return false };
    if !metadata.is_file() {
        return false
    }
    let Ok(written) = metadata.modified() else { return false };
    !sources.is_empty() && sources.iter().all(|source| source.map(|time| written >= time).unwrap_or(false))
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn image_error(error: image::ImageError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::parse_slot_nfo;

    #[test]
    fn parses_crlf_nfo_with_known_ids() {
        let nfo = "Blaze\r\n929\r\n570\r\nCentro Pok&{e}mon\r\n1860\r\n448\r\n20260802";
        assert_eq!(parse_slot_nfo(nfo), Some((1860, 448)));
    }

    #[test]
    fn parses_crlf_nfo_with_blank_lines_and_sprite_1383() {
        let nfo = "Mark\r\n\r\n6\r\n1\r\n\r\nRuta 1\r\n\r\n1383\r\n\r\n448\r\n\r\n20260802\r\n";
        assert_eq!(parse_slot_nfo(nfo), Some((1383, 448)));
    }

    #[test]
    fn parses_lf_nfo() {
        let nfo = "Saul\n493\n66\nCiudad Antares\n42\n445\n20260802";
        assert_eq!(parse_slot_nfo(nfo), Some((42, 445)));
    }

    #[test]
    fn rejects_incomplete_or_non_numeric_nfos() {
        assert_eq!(parse_slot_nfo(""), None);
        assert_eq!(parse_slot_nfo("Name\r\n1\r\n2\r\nPlace\r\n"), None);
        assert_eq!(parse_slot_nfo("Name\r\n1\r\n2\r\nPlace\r\nsprite\r\n448\r\n20260802"), None);
        assert_eq!(parse_slot_nfo("Name\r\n1\r\n2\r\nPlace\r\n1383\r\npkmn\r\n20260802"), None);
    }
}
