//! Temporary tool: extracts the game's icon and any logo/title art from the .exe.
//!
//! `cargo run --bin dumpassets --release -- <game.exe> <out-dir>`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read exe");
    let out = &args[2];
    std::fs::create_dir_all(out).ok();

    let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, false, true).expect("parse exe");

    if let Some(ico) = assets.ico_file_raw {
        std::fs::write(format!("{out}/reloaded.ico"), &ico).ok();
        if let Ok(image) = image::load_from_memory(&ico) {
            image.save(format!("{out}/reloaded-icon.png")).ok();
        }
        println!("icon: {} bytes", ico.len());
    } else {
        println!("icon: none");
    }

    let mut names = String::new();
    for (index, sprite) in assets.sprites.iter().enumerate() {
        let Some(sprite) = sprite.as_ref() else { continue };
        let name = String::from_utf8_lossy(&sprite.name.0).to_string();
        names.push_str(&format!("sprite\t{index}\t{name}\n"));
        let lower = name.to_lowercase();
        if lower.contains("logo") || lower.contains("titulo") || lower.contains("title") {
            if let Some(frame) = sprite.frames.first() {
                if let Some(image) = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.to_vec()) {
                    image.save(format!("{out}/sprite_{index}_{name}.png")).ok();
                    println!("sprite {index} {name} -> {}x{}", frame.width, frame.height);
                }
            }
        }
    }

    for (index, background) in assets.backgrounds.iter().enumerate() {
        let Some(background) = background.as_ref() else { continue };
        let name = String::from_utf8_lossy(&background.name.0).to_string();
        names.push_str(&format!("background\t{index}\t{name}\n"));
        let lower = name.to_lowercase();
        if lower.contains("logo") || lower.contains("titulo") || lower.contains("title") || lower.contains("menu") {
            if let Some(data) = background.data.as_ref() {
                let mut rgba = data.to_vec();
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                if let Some(image) = image::RgbaImage::from_raw(background.width, background.height, rgba) {
                    image.save(format!("{out}/bg_{index}_{name}.png")).ok();
                    println!("background {index} {name} -> {}x{}", background.width, background.height);
                }
            }
        }
    }

    let mut objects = String::new();
    for (index, object) in assets.objects.iter().enumerate() {
        let Some(object) = object.as_ref() else { continue };
        let name = String::from_utf8_lossy(&object.name.0).to_string();
        objects.push_str(&format!("{index}\t{name}\n"));
    }
    std::fs::write(format!("{out}/objects.txt"), objects).ok();

    let mut rooms = String::new();
    for (index, room) in assets.rooms.iter().enumerate() {
        let Some(room) = room.as_ref() else { continue };
        let name = String::from_utf8_lossy(&room.name.0).to_string();
        let caption = String::from_utf8_lossy(&room.caption.0).to_string();
        rooms.push_str(&format!("{index}\t{name}\t{caption}\n"));
    }
    std::fs::write(format!("{out}/rooms.txt"), rooms).ok();

    std::fs::write(format!("{out}/assets.txt"), names).ok();
    println!("done");
}
