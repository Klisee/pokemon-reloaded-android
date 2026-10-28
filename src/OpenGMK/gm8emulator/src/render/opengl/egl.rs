//! EGL context creation for Android.
//!
//! Mirrors `glx.rs`, but against `libEGL` rather than GLX: the surface is created from the
//! `ANativeWindow` handed out by ramen, and functions are loaded with `eglGetProcAddress`.

use std::{
    ffi::{c_void, CString},
    os::raw::{c_char, c_int, c_uint},
    ptr,
};

use ramen::{connection::Connection, window::Window};

pub type EGLDisplay = *mut c_void;
pub type EGLConfig = *mut c_void;
pub type EGLContext = *mut c_void;
pub type EGLSurface = *mut c_void;
pub type EGLNativeWindow = *mut c_void;
pub type EGLClientBuffer = *mut c_void;
pub type EGLImage = *mut c_void;
pub type EGLSync = *mut c_void;
pub type __eglMustCastToProperFunctionPointerType = *const c_void;

pub const EGL_NO_DISPLAY: EGLDisplay = ptr::null_mut();
pub const EGL_NO_CONTEXT: EGLContext = ptr::null_mut();
pub const EGL_NO_SURFACE: EGLSurface = ptr::null_mut();

pub const EGL_BUFFER_SIZE: c_int = 0x3020;
pub const EGL_RED_SIZE: c_int = 0x3024;
pub const EGL_GREEN_SIZE: c_int = 0x3023;
pub const EGL_BLUE_SIZE: c_int = 0x3022;
pub const EGL_ALPHA_SIZE: c_int = 0x3021;
pub const EGL_DEPTH_SIZE: c_int = 0x3025;
pub const EGL_STENCIL_SIZE: c_int = 0x3026;
pub const EGL_RENDERABLE_TYPE: c_int = 0x3040;
pub const EGL_OPENGL_ES2_BIT: c_int = 0x0004;
pub const EGL_OPENGL_ES3_BIT: c_int = 0x0040;
pub const EGL_CONTEXT_CLIENT_VERSION: c_int = 0x3098;
pub const EGL_SURFACE_TYPE: c_int = 0x3033;
pub const EGL_WINDOW_BIT: c_int = 0x0004;
pub const EGL_NONE: c_int = 0x3038;
/// Reported by `eglGetError` after a call that found the context gone (EGL 1.5).
pub const EGL_CONTEXT_LOST: c_uint = 0x300E;

extern "C" {
    fn eglGetDisplay(display_id: *mut c_void) -> EGLDisplay;
    fn eglInitialize(display: EGLDisplay, major: *mut c_int, minor: *mut c_int) -> c_uint;
    fn eglChooseConfig(
        display: EGLDisplay,
        attrib_list: *const c_int,
        configs: *mut EGLConfig,
        config_size: c_int,
        num_config: *mut c_int,
    ) -> c_uint;
    fn eglCreateContext(display: EGLDisplay, config: EGLConfig, share: EGLContext, attrib_list: *const c_int)
        -> EGLContext;
    fn eglCreateWindowSurface(
        display: EGLDisplay,
        config: EGLConfig,
        window: EGLNativeWindow,
        attrib_list: *const c_int,
    ) -> EGLSurface;
    fn eglMakeCurrent(display: EGLDisplay, draw: EGLSurface, read: EGLSurface, context: EGLContext) -> c_uint;
    fn eglSwapBuffers(display: EGLDisplay, surface: EGLSurface) -> c_uint;
    fn eglSwapInterval(display: EGLDisplay, interval: c_int) -> c_uint;
    fn eglGetProcAddress(procname: *const c_char) -> __eglMustCastToProperFunctionPointerType;
    fn eglDestroySurface(display: EGLDisplay, surface: EGLSurface) -> c_uint;
    fn eglDestroyContext(display: EGLDisplay, context: EGLContext) -> c_uint;
    fn eglTerminate(display: EGLDisplay) -> c_uint;
    fn eglGetError() -> c_uint;
}

thread_local! {
    /// Reused so the loader's C strings outlive the call that made them.
    static NAME_BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn no_op_fn() {}

/// Entry points stubbed out because GLES doesn't have them.
static MISSING_FNS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

/// The stubbed names, for diagnostics.
pub fn stubbed_functions() -> Vec<String> {
    MISSING_FNS.lock().unwrap().iter().map(|s| s.to_string()).collect()
}

/// Prints the stubbed entry points once, so they show up in the log without flooding it.
pub fn log_stubbed_once() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let list = stubbed_functions();
        if !list.is_empty() {
            eprintln!("[gl] ausentes, sustituidas por no-ops: {}", list.join(", "));
        }
    });
}

/// Everything EGL owns, plus the surface in use.
///
/// The surface does not live in the renderer because it has to be created and destroyed on the
/// activity thread, at the only moments that thread knows about for sure: when it hands over a new
/// window and when it takes the old one away. Only the *binding* belongs to the render thread,
/// because an EGL context can be current on a single thread - the one that owns it.
pub struct Gl {
    display: EGLDisplay,
    config: EGLConfig,
    context: EGLContext,
    surface: EGLSurface,
    /// The window the current surface was built on, so a surface tied to an old window is not
    /// mistaken for a valid one when Android hands over a new window.
    window_ptr: *mut c_void,
    /// The surface the context is bound to, so binding only happens when it changes.
    bound: EGLSurface,
    swap_interval: u32,
}

static GL: std::sync::Mutex<Option<Gl>> = std::sync::Mutex::new(None);

/// Set when the driver reports that the context is gone. There is no way to rebuild the context in
/// place: every texture, program, buffer and the atlases the game holds by id belong to the old one.
/// The session is ended instead, so the engine starts again from scratch with a fresh context.
static CONTEXT_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the context was lost. Checked by the game loop before drawing another frame.
pub fn context_lost() -> bool {
    CONTEXT_LOST.load(std::sync::atomic::Ordering::Relaxed)
}

/// Notes an EGL failure; only `EGL_CONTEXT_LOST` is fatal, the rest are transient.
fn note_egl_error(what: &str) {
    let error = unsafe { eglGetError() };
    if error == EGL_CONTEXT_LOST {
        CONTEXT_LOST.store(true, std::sync::atomic::Ordering::SeqCst);
        eprintln!("[gl] contexto EGL perdido en {}; se reiniciara la sesion", what);
    } else if error != 0x3000 {
        // Not fatal, but worth having in the log when a frame does not reach the screen.
        eprintln!("[gl] {} fallo (EGL error 0x{:04X})", what, error);
    }
}

// SAFETY: the handles are only ever reached through the mutex above, one thread at a time, and the
// EGL calls made here are the ones EGL allows across threads. Nothing else can touch them.
unsafe impl Send for Gl {}
unsafe impl Sync for Gl {}

/// Held for the duration of every frame, so the surface cannot be destroyed while one is being
/// drawn. The render thread takes it shared and the activity thread takes it exclusively before
/// detaching: without that, detaching mid-frame leaves that frame clearing a surface that has gone,
/// which OpenGL answers with `GL_INVALID_OPERATION` - fatal, and what killed the render thread.
static FRAME: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Shared guard to hold while a frame is being drawn.
pub fn frame_guard() -> Option<std::sync::RwLockReadGuard<'static, ()>> {
    FRAME.read().ok()
}

/// Exclusive guard, taken before destroying the surface so that no frame is in flight.
fn detach_guard() -> Option<std::sync::RwLockWriteGuard<'static, ()>> {
    FRAME.write().ok()
}

/// Creates the surface for a window the activity has just handed over, while it is still valid.
/// Deliberately does not touch the context: that belongs to the render thread, and binding it from
/// another thread takes it away from the one that actually draws.
pub unsafe fn attach_surface(native_window: *mut c_void) {
    if native_window.is_null() {
        return
    }
    // Keep the frame gate for the whole replacement, not just the destroy: a frame that starts
    // between destroying the old surface and creating the new one sees EGL_NO_SURFACE, or worse,
    // the new window while the old surface is being torn down. The order is the same as the render
    // thread (frame, then GL).
    let _no_frame = detach_guard();
    let Ok(mut slot) = GL.lock() else { return };
    let Some(gl) = slot.as_mut() else { return };
    if gl.surface != EGL_NO_SURFACE && gl.window_ptr != native_window {
        eglDestroySurface(gl.display, gl.surface);
        gl.surface = EGL_NO_SURFACE;
        gl.bound = EGL_NO_SURFACE;
    }
    if gl.surface != EGL_NO_SURFACE {
        return
    }
    let surface = eglCreateWindowSurface(gl.display, gl.config, native_window, ptr::null());
    if surface == EGL_NO_SURFACE {
        eprintln!("[gl] no pude crear la superficie para la ventana nueva");
        return
    }
    gl.surface = surface;
    gl.window_ptr = native_window;
}

/// Destroys the surface, while the window it was built on still exists: the only moment EGL can be
/// told about it cleanly.
pub unsafe fn detach_surface() {
    // Waits for any frame in flight to finish before pulling the surface out from under it.
    let _no_frame = detach_guard();
    let Ok(mut slot) = GL.lock() else { return };
    let Some(gl) = slot.as_mut() else { return };
    if gl.surface == EGL_NO_SURFACE {
        return
    }
    eglDestroySurface(gl.display, gl.surface);
    gl.surface = EGL_NO_SURFACE;
    gl.window_ptr = ptr::null_mut();
}

/// Whether there is a surface to draw to right now.
pub fn has_surface() -> bool {
    GL.lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|gl| gl.surface != EGL_NO_SURFACE))
        .unwrap_or(false)
}

/// Binds whatever surface is current, if that changed. Called from the render thread at the start of
/// every frame, because the first thing a frame does is clear the screen: doing that against a
/// surface that has gone away answers `GL_INVALID_OPERATION`, which the engine treats as fatal, and
/// that is what killed the render thread and left the screen black for good.
pub unsafe fn bind_surface() {
    let Ok(mut slot) = GL.lock() else { return };
    let Some(gl) = slot.as_mut() else { return };
    if gl.bound == gl.surface {
        return
    }
    let bound = if gl.surface == EGL_NO_SURFACE {
        eglMakeCurrent(gl.display, EGL_NO_SURFACE, EGL_NO_SURFACE, gl.context)
    } else {
        eglSwapInterval(gl.display, gl.swap_interval as c_int);
        eglMakeCurrent(gl.display, gl.surface, gl.surface, gl.context)
    };
    if bound == 0 {
        note_egl_error("eglMakeCurrent");
    }
    gl.bound = gl.surface;
}

/// Handle to the EGL state above. Carries nothing itself: every call goes through the shared slot.
pub struct PlatformImpl;

impl PlatformImpl {
    /// Requests a GLES 3 context, falling back to GLES 2 if unavailable.
    pub unsafe fn new(connection: &Connection, window: &Window) -> Result<Self, String> {
        let _ = connection;
        let native_window = window.xid();
        if native_window.is_null() {
            return Err("Android no ha entregado una ventana nativa".into())
        }

        // A new session starts with a fresh context, so any loss noted earlier no longer applies.
        CONTEXT_LOST.store(false, std::sync::atomic::Ordering::Relaxed);
        let display = eglGetDisplay(EGL_NO_DISPLAY);
        if display == EGL_NO_DISPLAY {
            return Err("eglGetDisplay fallo".into())
        }
        let (mut major, mut minor) = (0, 0);
        if eglInitialize(display, &mut major, &mut minor) == 0 {
            return Err("eglInitialize fallo".into())
        }

        let config_attribs = [
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES3_BIT,
            EGL_SURFACE_TYPE,
            EGL_WINDOW_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_ALPHA_SIZE,
            8,
            EGL_DEPTH_SIZE,
            24,
            EGL_STENCIL_SIZE,
            8,
            EGL_NONE,
        ];
        let mut config: EGLConfig = ptr::null_mut();
        let mut num_configs = 0;
        if eglChooseConfig(display, config_attribs.as_ptr(), &mut config, 1, &mut num_configs) == 0
            || num_configs == 0
        {
            // Fall back to GLES 2 sizing, which is more widely available.
            let config_attribs = [
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES2_BIT,
                EGL_SURFACE_TYPE,
                EGL_WINDOW_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_DEPTH_SIZE,
                24,
                EGL_STENCIL_SIZE,
                8,
                EGL_NONE,
            ];
            if eglChooseConfig(display, config_attribs.as_ptr(), &mut config, 1, &mut num_configs) == 0
                || num_configs == 0
            {
                return Err("eglChooseConfig fallo".into())
            }
        }

        let version_attribs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
        let context = eglCreateContext(display, config, ptr::null_mut(), version_attribs.as_ptr());
        let context = if context == EGL_NO_CONTEXT {
            let version_attribs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
            eglCreateContext(display, config, ptr::null_mut(), version_attribs.as_ptr())
        } else {
            context
        };
        if context == EGL_NO_CONTEXT {
            return Err("eglCreateContext fallo".into())
        }

        let surface = eglCreateWindowSurface(display, config, native_window, ptr::null());
        if surface == EGL_NO_SURFACE {
            return Err("eglCreateWindowSurface fallo".into())
        }
        if eglMakeCurrent(display, surface, surface, context) == 0 {
            eglDestroySurface(display, surface);
            eglDestroyContext(display, context);
            return Err("eglMakeCurrent fallo".into())
        }

        *GL.lock().unwrap() = Some(Gl {
            display,
            config,
            context,
            surface,
            window_ptr: native_window,
            bound: surface,
            swap_interval: 1,
        });
        Ok(Self)
    }

    pub unsafe fn get_function_loader() -> Result<Box<dyn FnMut(&'static str) -> *const c_void>, String> {
        // `eglGetProcAddress` only resolves extension entry points, so the core GLES functions
        // have to come from the symbol table. We link libGLESv3, which makes them available
        // through dlsym.
        let gles = libc::dlopen(
            CString::new("libGLESv3.so").unwrap().as_ptr(),
            libc::RTLD_NOW | libc::RTLD_GLOBAL,
        );
        if gles.is_null() {
            let gles2 = libc::dlopen(
                CString::new("libGLESv2.so").unwrap().as_ptr(),
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            );
            if gles2.is_null() {
                return Err("no se encontro libGLESv3.so ni libGLESv2.so".into())
            }
        }

        Ok(Box::new(move |name: &'static str| {
            NAME_BUF.with(|buf| {
                let mut buf = buf.borrow_mut();
                buf.clear();
                buf.extend_from_slice(name.as_bytes());
                buf.push(0);

                // Extensions first, then the core symbols linked into the process.
                let mut ptr = eglGetProcAddress(buf.as_ptr().cast()) as *const c_void;
                if ptr.is_null() {
                    ptr = libc::dlsym(gles, buf.as_ptr().cast()) as *const c_void;
                }
                if ptr.is_null() {
                    ptr = libc::dlsym(ptr::null_mut(), buf.as_ptr().cast()) as *const c_void;
                }
                if ptr.is_null() {
                    // Desktop-only entry points that GLES doesn't provide. Rather than let
                    // gl_generator's fallback abort the game, point at a no-op and remember
                    // which ones we stubbed so they can be reported.
                    MISSING_FNS.lock().unwrap().push(name);
                    ptr = no_op_fn as *const c_void;
                }
                ptr
            })
        }))
    }

    pub unsafe fn clean_function_loader() {}

    pub unsafe fn swap_buffers(&self) {
        let Ok(slot) = GL.lock() else { return };
        let Some(gl) = slot.as_ref() else { return };
        if gl.surface != EGL_NO_SURFACE {
            if eglSwapBuffers(gl.display, gl.surface) == 0 {
                note_egl_error("eglSwapBuffers");
            }
        }
    }

    pub unsafe fn set_swap_interval(&self, n: u32) -> bool {
        let Ok(mut slot) = GL.lock() else { return false };
        let Some(gl) = slot.as_mut() else { return false };
        gl.swap_interval = n;
        if gl.bound == EGL_NO_SURFACE {
            // Nothing bound yet; it is applied when the surface is bound.
            return true
        }
        eglSwapInterval(gl.display, n as c_int) != 0
    }

    pub unsafe fn get_swap_interval(&self) -> u32 {
        GL.lock().ok().and_then(|slot| slot.as_ref().map(|gl| gl.swap_interval)).unwrap_or(1)
    }

    pub unsafe fn wait_vsync(&self) {
        // No portable vsync wait on Android.
    }
}

impl Drop for PlatformImpl {
    fn drop(&mut self) {
        unsafe {
            let Ok(mut slot) = GL.lock() else { return };
            let Some(gl) = slot.take() else { return };
            // Unbind first so the surface and context can be destroyed from this thread too.
            eglMakeCurrent(gl.display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
            if gl.surface != EGL_NO_SURFACE {
                eglDestroySurface(gl.display, gl.surface);
            }
            eglDestroyContext(gl.display, gl.context);
            // Deliberately no eglTerminate: it terminates every context on the display, not just
            // this one, and would break any other EGL user in the same process (Compose included).
        }
    }
}
