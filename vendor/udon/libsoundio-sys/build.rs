// Mostly stolen from libgit2-sys

use std::env;
use std::ffi::OsString;
use std::fs;
use std::process::Command;

// This is like try!() but panics instead of returning an error.
macro_rules! t {
    ($e:expr) => {
        match $e {
            Ok(e) => e,
            Err(e) => panic!("{} failed with {}", stringify!($e), e),
        }
    };
}

fn main() {
    // First check whether a pkg-config command is installed in the path.
    let has_pkgconfig = Command::new("pkg-config").output().is_ok();

    // If the environment variable LIBSOUNDIO_SYS_USE_PKG_CONFIG exists
    // and there is a libsoundio system library use that. I wouldn't recommend
    // this option.
    if env::var("LIBSOUNDIO_SYS_USE_PKG_CONFIG").is_ok() {
        assert!(has_pkgconfig, "pkg-config required");
        assert!(
            pkg_config::find_library("libsoundio").is_ok(),
            "Can't run pkg-config"
        );
        return;
    }

    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();

    // Is the target Windows?
    let windows = target.contains("windows");

    // Create a new cmake config.
    let mut cfg = cmake::Config::new("libsoundio");

    // When cross-compiling, we're pretty unlikely to find a `dlltool` binary
    // lying around, so try to find another if it exists
    // What is dlltool?

    // If we're cross-compiling to Windows from a non-Windows platform...
    if windows && !host.contains("windows") {
        // Get the name of the C compiler.
        let c_compiler = cc::Build::new().cargo_metadata(false).get_compiler();
        let exe = c_compiler.path();

        // Then see if we can find it in the PATH.
        let path = env::var_os("PATH").unwrap_or(OsString::new());
        let exe = env::split_paths(&path)
            .map(|p| p.join(&exe))
            .find(|p| p.exists());

        // If we found it...
        if let Some(exe) = exe {
            if let Some(name) = exe.file_name().and_then(|e| e.to_str()) {
                // Then replace gcc with dlltool and set that in the cmake config.
                let name = name.replace("gcc", "dlltool");
                let dlltool = exe.with_file_name(name);
                cfg.define("DLLTOOL", &dlltool);
            }
        }
    }

    if target.contains("linux") {
        let alsa = pkg_config::probe_library("alsa").unwrap();
        let libpulse = pkg_config::probe_library("libpulse").unwrap();
        let jack = pkg_config::probe_library("jack").unwrap();
        for lib in alsa.libs.iter()
            .chain(libpulse.libs.iter())
            .chain(jack.libs.iter())
        {
            println!("cargo:rustc-link-lib={}", lib);
        }
        println!("cargo:rustc-link-lib=stdc++");
    }

    // Clear the output directory.
    let _ = fs::remove_dir_all(env::var("OUT_DIR").unwrap());
    // And then create an empty output directory.
    t!(fs::create_dir_all(env::var("OUT_DIR").unwrap()));

    // Don't bother with shared libs.
    let dst = cfg
        .define("BUILD_DYNAMIC_LIBS", "OFF")
        .define("BUILD_STATIC_LIBS", "ON")
        .define("BUILD_EXAMPLE_PROGRAMS", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .build();

    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("lib64").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("lib").display()
    );

    // Windows...
    if target.contains("windows") {
        // We need to link ole32 on Windows.
        println!("cargo:rustc-link-lib=avrt");
        println!("cargo:rustc-link-lib=ole32");
    }

    // Link soundio.
    println!("cargo:rustc-link-lib=static=soundio");

    // OSX
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
