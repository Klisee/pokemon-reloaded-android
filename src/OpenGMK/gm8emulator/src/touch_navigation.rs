//! Recognize taps before generating a click; scrolling emits only directional keys.
use std::{collections::VecDeque,sync::{Mutex,atomic::{AtomicU64,Ordering}},time::{Duration,Instant}};
use ramen::input::Key;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Identity{pub room:i32,pub modal:i32}
static IDENTITY:Mutex<Identity>=Mutex::new(Identity{room:-1,modal:-1});
static BAG:Mutex<Option<i32>>=Mutex::new(None);
static ARMED:Mutex<Option<(Identity,Instant)>>=Mutex::new(None);
static FRAME:AtomicU64=AtomicU64::new(0);
pub fn identity()->Identity{*IDENTITY.lock().unwrap()}
pub fn arm_bag(){*ARMED.lock().unwrap()=Some((identity(),Instant::now()));}
pub fn disarm_bag(){*ARMED.lock().unwrap()=None;}
pub fn report(room:i32,modal:i32,named_bag:bool){
    *IDENTITY.lock().unwrap()=Identity{room,modal};let mut bag=BAG.lock().unwrap();
    if bag.is_some_and(|id|id!=room){*bag=None;}
    let mut armed=ARMED.lock().unwrap();
    if let Some((old,at))=*armed{
        if at.elapsed()>Duration::from_secs(2){*armed=None;}
        else if old.room!=room{*bag=Some(room);*armed=None;}
    }
    if named_bag{*bag=Some(room);}
    FRAME.fetch_add(1,Ordering::Relaxed);
}
pub fn bag_open()->bool{BAG.lock().unwrap().is_some_and(|id|id==identity().room)}
pub fn active()->bool{crate::touch_controls::current_context()==crate::touch_controls::MENU_NAV&&!crate::touch_controls::info_window_open()}
#[derive(Clone,Debug)]
pub struct Gesture{pub id:i32,pub context:Identity,start:(f32,f32),last_y:f32,residual:f32,pub dragged:bool,slop:f32,step:f32}
impl Gesture{
    pub fn new(id:i32,x:f32,y:f32,height:f32,context:Identity)->Self{Self{id,context,start:(x,y),last_y:y,residual:0.,dragged:false,slop:(height*0.015).clamp(8.,24.),step:(height*0.055).clamp(20.,64.)}}
    pub fn moved(&mut self,x:f32,y:f32)->i32{
        let dx=x-self.start.0;let dy=y-self.start.1;
        if !self.dragged&&dx*dx+dy*dy>self.slop*self.slop{self.dragged=true;}
        if !self.dragged{return 0}
        self.residual+=self.last_y-y;self.last_y=y;
        let steps=(self.residual/self.step).trunc().clamp(-12.,12.)as i32;self.residual-=steps as f32*self.step;steps
    }
}
static GESTURE:Mutex<Option<Gesture>>=Mutex::new(None);
struct Pending{context:Identity,keys:VecDeque<Key>,last:Instant,confirm_frame:u64}
static PENDING:Mutex<Option<Pending>>=Mutex::new(None);
pub fn owns(id:i32)->bool{GESTURE.lock().unwrap().as_ref().is_some_and(|g|g.id==id)}
pub fn begin(id:i32,x:f32,y:f32,height:f32)->bool{
    let mut g=GESTURE.lock().unwrap();if g.is_some() || TAP.lock().unwrap().is_some(){return false}
    *PENDING.lock().unwrap()=None;*g=Some(Gesture::new(id,x,y,height,identity()));true
}
pub fn moved(id:i32,x:f32,y:f32)->bool{
    let mut guard=GESTURE.lock().unwrap();let Some(g)=guard.as_mut().filter(|g|g.id==id)else{return false};
    let steps=g.moved(x,y);
    if steps!=0{
        let mut pending=PENDING.lock().unwrap();let p=pending.get_or_insert_with(||Pending{context:g.context,keys:VecDeque::new(),last:Instant::now()-Duration::from_millis(60),confirm_frame:0});
        let key=if steps>0{Key::DownArrow}else{Key::UpArrow};
        if p.keys.back().is_some_and(|k|*k!=key){p.keys.clear();}
        for _ in 0..steps.abs(){if p.keys.len()<12{p.keys.push_back(key);}}
    }true
}
/// Only a stationary release schedules a primary click, never a Z plus a click.
pub fn end(id:i32,x:f32,y:f32)->Option<(f32,f32)>{
    let mut guard=GESTURE.lock().unwrap();if !guard.as_ref().is_some_and(|g|g.id==id){return None}
    let mut g=guard.take().unwrap();g.moved(x,y);
    if g.dragged||g.context!=identity()||!active(){return None}
    *PENDING.lock().unwrap()=None;
    *TAP.lock().unwrap()=Some(Tap{context:g.context,x,y,pressed:false,due:Instant::now()+Duration::from_millis(45)});
    Some((x,y))
}
pub fn next_key()->Option<Key>{
    let mut guard=PENDING.lock().unwrap();let p=guard.as_mut()?;
    if p.context!=identity()||!active(){*guard=None;return None}
    if p.last.elapsed()<Duration::from_millis(55)||FRAME.load(Ordering::Relaxed)<p.confirm_frame{return None}
    p.last=Instant::now();let key=p.keys.pop_front();if p.keys.is_empty(){*guard=None;}key
}
pub fn cancel()->Option<i32>{*PENDING.lock().unwrap()=None;GESTURE.lock().unwrap().take().map(|g|g.id)}

struct Tap {context:Identity,x:f32,y:f32,pressed:bool,due:Instant}
static TAP:Mutex<Option<Tap>>=Mutex::new(None);
/// Hover -> primary down -> primary up. Time-based release is essential: the game may
/// wait for mouse-up inside a modal loop without redrawing another frame.
pub fn next_pointer()->Option<(bool,f32,f32)>{
    let mut guard=TAP.lock().unwrap();let tap=guard.as_mut()?;
    if tap.context!=identity() || !active(){
        let release=tap.pressed.then_some((false,tap.x,tap.y));*guard=None;return release
    }
    if Instant::now()<tap.due{return None}
    let event=(!tap.pressed,tap.x,tap.y);
    if tap.pressed{*guard=None;}else{tap.pressed=true;tap.due=Instant::now()+Duration::from_millis(70);}
    Some(event)
}
pub fn cancel_pointer()->Option<(f32,f32)>{
    TAP.lock().unwrap().take().filter(|t|t.pressed).map(|t|(t.x,t.y))
}
