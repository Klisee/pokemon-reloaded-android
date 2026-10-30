use gm8exe::reader::from_exe;
use std::fs;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let file = fs::read(path).unwrap();
    let assets = from_exe(file, None::<fn(&str)>, false, false).unwrap();
    for (idx, room) in assets.rooms.iter().enumerate() {
        if let Some(room) = room {
            let name = room.name.to_string();
            if idx == 30 || idx == 43 || (455..=465).contains(&idx) || name.to_ascii_lowercase().contains("multi") {
                println!("ROOM {} {:?} {}x{} instances={} creation={:?}", idx, name, room.width, room.height, room.instances.len(), room.creation_code.to_string());
                for inst in &room.instances {
                    let obj = assets.objects.get(inst.object as usize).and_then(|o| o.as_ref()).map(|o| o.name.to_string()).unwrap_or_else(|| format!("#{}", inst.object));
                    println!("  obj {obj} at {},{} id {}", inst.x, inst.y, inst.id);
                }
            }
        }
    }
}
