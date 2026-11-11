//! Desktop entry point. The shared logic lives in the library so the Android entry point
//! can call exactly the same `run_game`.

#[cfg(target_os = "windows")]
extern "system" {
    fn SetConsoleTitleW(lpConsoleTitle: *const u16) -> i32;
}

fn main() {
    #[cfg(target_os = "windows")]
    unsafe {
        let title: Vec<u16> = "Reloaded OpenGMK\0".encode_utf16().collect();
        SetConsoleTitleW(title.as_ptr());
    }
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(gm8emulator::run_game(args));
}
