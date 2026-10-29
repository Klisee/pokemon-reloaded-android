//! Native Android audio output via AAudio.
//!
//! The rest of the engine feeds a `Mixer` (an `udon::Source`). On desktop, `udon`'s SoundIo
//! session pulls from it; there is no such backend for Android, so here we open an AAudio output
//! stream and drive the very same `Mixer` straight from its data callback.
//!
//! AAudio lives in `libaaudio.so` (linked from the NDK sysroot), so this is plain FFI — no extra
//! crates and no C++ toolchain.

use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex,
    },
    sync::Arc,
};

use udon::source::{ChannelCount, Sample, SampleRate, Source};

use super::mixer::{Mixer, MixerHandle};
use super::GbAudioParams;

// aaudio/AAudio.h
const AAUDIO_OK: i32 = 0;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;
// aaudio_stream_state_t, in header order.
const AAUDIO_STREAM_STATE_UNINITIALIZED: i32 = 0;
const AAUDIO_STREAM_STATE_CLOSED: i32 = 12;
const AAUDIO_STREAM_STATE_DISCONNECTED: i32 = 13;

/// The mixer currently being pulled by the AAudio callback, if any.
static MIXER: Mutex<Option<Mixer>> = Mutex::new(None);

/// The output stream, so it can be stopped and resumed when the app goes to the background and
/// comes back. Set by [open].
static STREAM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The stream's parameters, kept so a stream Android reclaimed in the background can be rebuilt.
static CHANNELS: AtomicU32 = AtomicU32::new(0);
static RATE: AtomicU32 = AtomicU32::new(0);

/// The two independent reasons the output may be paused, kept in one lock so the UI thread
/// (focus loss) and the activity pump (background/resume) cannot overwrite each other.
struct PausedState {
    activity: bool,
    focus: bool,
    applied: bool,
}

static PAUSED: Mutex<PausedState> = Mutex::new(PausedState {
    activity: false,
    focus: false,
    applied: false,
});

fn update_paused(activity: Option<bool>, focus: Option<bool>) {
    let (want, changed) = {
        let mut state = PAUSED.lock().unwrap();
        if let Some(value) = activity { state.activity = value; }
        if let Some(value) = focus { state.focus = value; }
        let want = state.activity || state.focus;
        if want == state.applied {
            (want, false)
        } else {
            state.applied = want;
            (want, true)
        }
    };
    if changed {
        apply_paused(want);
    }
}

fn apply_paused(paused: bool) {
    let stream = STREAM.load(Ordering::Relaxed) as *mut c_void;
    if stream.is_null() {
        return
    }
    unsafe {
        if paused {
            AAudioStream_requestPause(stream);
            // Drop what is already queued, so the silence arrives now instead of draining first.
            AAudioStream_requestFlush(stream);
            return
        }
        // Android reclaims streams from background apps. Starting a dead one can block forever
        // waiting for a callback that will never come, taking the game thread with it; it has to
        // be closed and rebuilt instead.
        let state = AAudioStream_getState(stream);
        if state == AAUDIO_STREAM_STATE_DISCONNECTED
            || state == AAUDIO_STREAM_STATE_CLOSED
            || state == AAUDIO_STREAM_STATE_UNINITIALIZED
        {
            eprintln!("[aaudio] stream en estado {}, reconstruyendo", state);
            AAudioStream_close(stream);
            STREAM.store(0, Ordering::Relaxed);
            if let Err(error) = reopen_stream() {
                eprintln!("[aaudio] no pude reconstruir la salida: {}", error);
            }
            return
        }
        let _ = AAudioStream_requestStart(stream);
    }
}

/// Called by the activity pump when the window goes away or comes back.
pub fn set_activity_paused(paused: bool) {
    update_paused(Some(paused), None);
}

/// Called by the audio-focus listener. Focus loss must not resume sound while the activity is paused.
pub fn set_focus_paused(paused: bool) {
    update_paused(None, Some(paused));
}

/// Builds a stream with the parameters saved by [open]. Used to replace one Android reclaimed.
unsafe fn reopen_stream() -> Result<(), String> {
    let channels = CHANNELS.load(Ordering::Relaxed);
    let rate = RATE.load(Ordering::Relaxed);
    if channels == 0 || rate == 0 {
        return Err("no hay parametros de audio guardados".into())
    }
    let stream = create_stream(rate as i32, channels as i32)?;
    let actual_channels = AAudioStream_getChannelCount(stream).max(1) as u32;
    let actual_rate = AAudioStream_getSampleRate(stream).max(8000) as u32;
    // The mixer is fixed at the original channel count and rate. If routing changed what AAudio
    // actually gave us, feeding it with the old layout would overrun the callback buffer. Refuse
    // this stream instead: sound pauses, but the process and the next session stay intact.
    if actual_channels != channels || actual_rate != rate {
        AAudioStream_close(stream);
        return Err(format!(
            "AAudio cambio el formato: {} canales @ {} Hz, se esperaban {} @ {}",
            actual_channels, actual_rate, channels, rate
        ))
    }
    STREAM.store(stream as usize, Ordering::Relaxed);
    check(AAudioStream_requestStart(stream), "AAudioStream_requestStart")
}

/// Creates the output stream; the caller owns it until it is closed.
unsafe fn create_stream(sample_rate: i32, channel_count: i32) -> Result<*mut c_void, String> {
    let mut builder: *mut c_void = std::ptr::null_mut();
    check(AAudio_createStreamBuilder(&mut builder), "AAudio_createStreamBuilder")?;
    AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
    AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
    AAudioStreamBuilder_setChannelCount(builder, channel_count);
    AAudioStreamBuilder_setSampleRate(builder, sample_rate);
    AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
    AAudioStreamBuilder_setDataCallback(builder, data_callback, std::ptr::null_mut());
    let mut stream: *mut c_void = std::ptr::null_mut();
    let result = AAudioStreamBuilder_openStream(builder, &mut stream);
    AAudioStreamBuilder_delete(builder);
    check(result, "AAudioStreamBuilder_openStream")?;
    Ok(stream)
}

extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut c_void) -> i32;
    fn AAudioStreamBuilder_setDirection(builder: *mut c_void, direction: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut c_void, format: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut c_void, channel_count: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut c_void, sample_rate: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut c_void, mode: i32);
    fn AAudioStreamBuilder_setDataCallback(
        builder: *mut c_void,
        callback: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, i32) -> i32,
        user_data: *mut c_void,
    );
    fn AAudioStreamBuilder_openStream(builder: *mut c_void, stream: *mut *mut c_void) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut c_void) -> i32;
    fn AAudioStream_requestStart(stream: *mut c_void) -> i32;
    fn AAudioStream_requestPause(stream: *mut c_void) -> i32;
    fn AAudioStream_requestFlush(stream: *mut c_void) -> i32;
    fn AAudioStream_getChannelCount(stream: *mut c_void) -> i32;
    fn AAudioStream_getSampleRate(stream: *mut c_void) -> i32;
    fn AAudioStream_getState(stream: *mut c_void) -> i32;
    fn AAudioStream_close(stream: *mut c_void) -> i32;
}

/// Set once the mixer has produced something audible. The launcher waits for this before handing the
/// screen over: the game paints a small "Loading…" screen well before its first sound.
static SOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether anything has actually been heard yet.
pub fn sound_started() -> bool {
    SOUND.load(Ordering::Relaxed)
}

/// Pulls `num_frames` of interleaved f32 audio out of the mixer. AAudio calls this from its own
/// real-time thread, so a failed `try_lock` just yields silence instead of blocking that thread.
extern "C" fn data_callback(
    _stream: *mut c_void,
    _user_data: *mut c_void,
    audio_data: *mut c_void,
    num_frames: i32,
) -> i32 {
    let Ok(mut guard) = MIXER.try_lock() else {
        return AAUDIO_CALLBACK_RESULT_CONTINUE
    };
    let channels = guard.as_ref().map(|m| m.channel_count().get() as usize).unwrap_or(2);
    let len = (num_frames.max(0) as usize) * channels;
    // SAFETY: AAudio guarantees `audio_data` points to `num_frames * channels` f32 samples.
    let buffer = unsafe { std::slice::from_raw_parts_mut(audio_data as *mut Sample, len) };
    match guard.as_mut() {
        Some(mixer) => {
            let count = mixer.write_samples(buffer);
            if count < len {
                buffer[count..].iter_mut().for_each(|x| *x = 0.0);
            }
        },
        None => buffer.iter_mut().for_each(|x| *x = 0.0),
    }
    // Cheap after the first time: the check is skipped once something has been heard.
    if !SOUND.load(Ordering::Relaxed) && buffer.iter().any(|x| *x != 0.0) {
        SOUND.store(true, Ordering::Relaxed);
    }
    AAUDIO_CALLBACK_RESULT_CONTINUE
}

fn check(result: i32, what: &str) -> Result<(), String> {
    if result == AAUDIO_OK {
        Ok(())
    } else {
        Err(format!("{} fallo (AAudio error {})", what, result))
    }
}

/// Opens the output stream and hands the mixer over to its callback. Returns the raw stream
/// pointer (keep it alive for the process lifetime), the device channel count, its sample rate,
/// and the handle used to add sounds.
pub fn open(
    global_volume: Arc<AtomicU32>,
    gb_audio: Arc<GbAudioParams>,
) -> Result<(usize, ChannelCount, SampleRate, MixerHandle), String> {
    unsafe {
        let stream = create_stream(48000, 2)?;

        let channels = AAudioStream_getChannelCount(stream).max(1) as u32;
        let sample_rate = AAudioStream_getSampleRate(stream).max(8000) as u32;
        let channels = ChannelCount::new(channels as u16).ok_or("canal de audio invalido")?;
        let sample_rate = SampleRate::new(sample_rate).ok_or("frecuencia de audio invalida")?;

        let (mixer, handle) = Mixer::new(sample_rate, channels, global_volume, gb_audio);
        *MIXER.lock().unwrap() = Some(mixer);
        CHANNELS.store(channels.get() as u32, Ordering::Relaxed);
        RATE.store(sample_rate.get() as u32, Ordering::Relaxed);
        STREAM.store(stream as usize, Ordering::Relaxed);

        check(AAudioStream_requestStart(stream), "AAudioStream_requestStart")?;
        eprintln!("[aaudio] salida abierta: {} canales @ {} Hz", channels.get(), sample_rate.get());
        Ok((stream as usize, channels, sample_rate, handle))
    }
}
