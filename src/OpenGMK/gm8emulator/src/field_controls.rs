//! Exploration's latched seven-key menu; battle owns its separate hold-and-drag selector.
use std::sync::atomic::{AtomicBool,Ordering};
pub const NAME:&str="sel_world";
static OPEN:AtomicBool=AtomicBool::new(false);
pub fn open()->bool {
    let active=crate::touch_controls::current_context()==crate::touch_controls::WORLD
        && !crate::touch_controls::pad_active() && !crate::touch_controls::info_window_open();
    if !active {close();}
    active && OPEN.load(Ordering::Relaxed)
}
pub fn close(){OPEN.store(false,Ordering::Relaxed);}
pub fn toggle(){if open(){close();}else if crate::touch_controls::current_context()==crate::touch_controls::WORLD{OPEN.store(true,Ordering::Relaxed);}}
pub fn rect(ww:f32,wh:f32)->(f32,f32,f32,f32){
    let h=(wh*0.54).min(ww*0.42);let w=h*484.0/512.0;
    (ww*0.50-w*0.5,wh*0.5-h*0.5,w,h)
}
// Polygons follow the supplied artwork, before its crop at (144,116). Black gaps are inert.
const CELLS:[&[(f32,f32)];7]=[
 &[(627.,130.),(789.,232.),(789.,415.),(716.,481.),(540.,481.),(466.,414.),(466.,232.)],
 &[(930.,286.),(1092.,385.),(1092.,561.),(1007.,638.),(851.,638.),(788.,580.),(723.,490.),(795.,416.),(795.,365.)],
 &[(932.,631.),(1093.,705.),(1093.,899.),(932.,1006.),(797.,916.),(797.,838.),(719.,788.),(789.,728.),(789.,705.),(844.,650.)],
 &[(627.,784.),(705.,796.),(781.,844.),(781.,1035.),(627.,1120.),(472.,1035.),(472.,844.),(549.,796.)],
 &[(326.,635.),(408.,650.),(462.,707.),(462.,728.),(536.,789.),(463.,838.),(463.,916.),(326.,1005.),(162.,900.),(162.,706.)],
 &[(327.,286.),(456.,365.),(456.,416.),(530.,492.),(468.,548.),(468.,579.),(407.,639.),(249.,639.),(162.,562.),(162.,385.)],
 &[(550.,490.),(704.,490.),(774.,547.),(774.,726.),(701.,781.),(551.,781.),(479.,726.),(479.,547.)],
];
pub const CENTERS:[(f32,f32);7]=[(627.,320.),(932.,472.),(932.,822.),(627.,967.),(327.,822.),(327.,472.),(627.,635.)];
pub const KEYS:[ramen::input::Key;7]=[ramen::input::Key::Alpha1,ramen::input::Key::Alpha2,ramen::input::Key::Alpha3,ramen::input::Key::Alpha4,ramen::input::Key::Alpha5,ramen::input::Key::Alpha6,ramen::input::Key::Alpha7];
fn polygon(x:f32,y:f32,p:&[(f32,f32)])->bool{
    let mut inside=false;let mut j=p.len()-1;
    for i in 0..p.len(){let(xi,yi)=p[i];let(xj,yj)=p[j];if (yi>y)!=(yj>y)&&x<(xj-xi)*(y-yi)/(yj-yi)+xi{inside=!inside;}j=i;}
    inside
}
pub fn item_at(x:f32,y:f32,ww:f32,wh:f32)->Option<usize>{
    if ww<=0.||wh<=0.{return None}
    let(l,t,w,h)=rect(ww,wh);let sx=(x-l)/w*966.+144.;let sy=(y-t)/h*1022.+116.;
    let badges=[(627.,167.),(930.,316.),(930.,663.),(627.,812.),(326.,665.),(326.,316.),(627.,499.)];
    for(i,(bx,by))in badges.iter().enumerate(){if(sx-bx).abs()<37.&&(sy-by).abs()<37.{return Some(i)}}
    CELLS.iter().position(|p|polygon(sx,sy,p))
}
