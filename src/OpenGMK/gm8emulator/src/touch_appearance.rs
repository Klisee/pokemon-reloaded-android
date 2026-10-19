//! Visual feedback is independent of key dispatch and hit testing.
use std::sync::Mutex;
use std::time::{Duration, Instant};
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style { Modern, Classic }
#[derive(Clone, Copy)]
pub struct Settings { pub style: Style, pub tint: Option<[f32;3]>, pub animations: bool, pub haptics: bool }
impl Settings { const DEFAULT: Self = Self { style: Style::Modern, tint: None, animations: true, haptics: true }; }
static SETTINGS: Mutex<Settings> = Mutex::new(Settings::DEFAULT);
#[derive(Clone, Copy)]
pub struct Transition { from: f32, target: f32, at: Option<Instant> }
impl Transition {
    pub const IDLE: Self = Self { from: 0.0, target: 0.0, at: None };
    pub fn value(&self, now: Instant) -> f32 {
        let Some(at) = self.at else { return self.target };
        let ms = if self.target > self.from { 55.0 } else { 80.0 };
        let t = (now.saturating_duration_since(at).as_secs_f32()*1000.0/ms).clamp(0.0,1.0);
        self.from + (self.target-self.from)*t*t*(3.0-2.0*t)
    }
    pub fn change(&mut self, pressed: bool, now: Instant) {
        self.from=self.value(now);self.target=if pressed {1.0}else{0.0};self.at=Some(now);
    }
}
static TRANSITIONS: Mutex<[Transition;64]> = Mutex::new([Transition::IDLE;64]);
pub fn settings() -> Settings { *SETTINGS.lock().unwrap() }
pub fn load(text: &str) {
    let mut s=Settings::DEFAULT;
    if text.len()<=4096 && text.lines().next()==Some("touch-appearance-v1") {
        for line in text.lines().skip(1) {
            let Some((key,value))=line.split_once('=') else {continue};
            match key.trim() {
                "style" => s.style=match value.trim(){"classic"|"minimal"|"pixel"=>Style::Classic,_=>Style::Modern},
                "animations" => s.animations=value.trim()!="0",
                "haptics" => s.haptics=value.trim()!="0",
                "tint" => {
                    let hex=value.trim();
                    s.tint=if hex.len()==6 {u32::from_str_radix(hex,16).ok().map(|n| [((n>>16)&255)as f32/255.0,((n>>8)&255)as f32/255.0,(n&255)as f32/255.0])}else{None};
                },
                _=>{},
            }
        }
    }
    *SETTINGS.lock().unwrap()=s;*TRANSITIONS.lock().unwrap()=[Transition::IDLE;64];
}
pub fn set_pressed(index: usize, pressed: bool) {
    if let Some(t)=TRANSITIONS.lock().unwrap().get_mut(index){t.change(pressed,Instant::now());}
}
pub fn visual(index: usize) -> (f32,f32) {
    if !settings().animations {return (1.0,1.0)}
    let p=TRANSITIONS.lock().unwrap().get(index).map_or(0.0,|t|t.value(Instant::now()));
    (1.0-0.08*p,1.0-0.22*p)
}
pub fn tint_pixel(pixel: &mut [u8;4], tint:[f32;3]) {
    let light=pixel[0].max(pixel[1]).max(pixel[2]) as f32;
    for i in 0..3 {pixel[i]=(light*tint[i]).round().clamp(0.0,255.0)as u8;}
}
