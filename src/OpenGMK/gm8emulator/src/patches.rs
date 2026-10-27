//! Runtime fix-ups for bugs in the game's own GML.
//!
//! The game file is never modified: when a script is compiled, its source is swapped here for a
//! corrected version. Keep the list short, and document what each entry changes and why.

/// Returns a corrected source for `name`, or `None` to compile the game's original.
pub fn fixup(name: &str, source: &str) -> Option<String> {
    if let Some(source) = crate::mods::script_override(name) {
        return Some(source);
    }
    match name {
        // The title menu and the options screen draw their entries horizontally centred on x=160
        // (so the text spans roughly x=110..210) but only accept clicks inside
        // `mouse_x >= 62 && mouse_x <= 178`. Widen the box to cover the text as drawn, and move the
        // six title entries up so the extra Mods row fits above the footer.
        "MouseActionsInicioControl" => {
            let s = source
                .replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")
                .replace("mouse_y>=75 && mouse_y<=88", "mouse_y>=71 && mouse_y<=84")
                .replace("mouse_y>=93 && mouse_y<=106", "mouse_y>=87 && mouse_y<=100")
                .replace("mouse_y>=111 && mouse_y<=124", "mouse_y>=103 && mouse_y<=116")
                .replace("mouse_y>=129 && mouse_y<=142", "mouse_y>=119 && mouse_y<=132")
                .replace("mouse_y>=147 && mouse_y<=160", "mouse_y>=135 && mouse_y<=148");
            Some(format!("{s} if(global.mods_available && mouse_x>=62 && mouse_x<=178 && mouse_y>=151 && mouse_y<=162){{SEL=5;MOUSE=5;exit;}}"))
        },
        "OptionsControlMouse" => Some(source.replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")),
        "InitializeInicioControl" => {
            let mut s = source.to_string();
            s.push_str(" if(file_exists(\"mods/active.flag\")){global.mods_available=true; var tmp; tmp=text_op5; text_op5=\"Mods\"; text_op6=tmp;}else{global.mods_available=false;text_op6=\"\";}");
            Some(s)
        },
        "KeyActionsInicioControl" => {
            let mut s = String::new();
            // The native Mods screen owns the click; never let Enter/Z fall through to the old
            // "Salir del juego" action while that row is selected.
            s.push_str(" if(global.mods_available && SEL=4 && (keyboard_check(key_ini_entrar)||keyboard_check(key_ini_accion))){ exit; } ");
            s.push_str(source);
            Some(s)
        },
        "DrawInicioControl" => {
            let s = source
                .replace("draw_text_color(160,74", "draw_text_color(160,70")
                .replace("draw_text_color(160,92", "draw_text_color(160,86")
                .replace("draw_text_color(160,110", "draw_text_color(160,102")
                .replace("draw_text_color(160,128", "draw_text_color(160,118")
                .replace("draw_text_color(160,146", "draw_text_color(160,134")
                .replace(",74-1+random(2)", ",70-1+random(2)")
                .replace(",92-1+random(2)", ",86-1+random(2)")
                .replace(",110-1+random(2)", ",102-1+random(2)")
                .replace(",128-1+random(2)", ",118-1+random(2)")
                .replace(",146-1+random(2)", ",134-1+random(2)");
            Some(format!("{s} if(global.mods_available){{ var c6; c6=c_white; if(SEL=5)c6=c_red; draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,150,text_op6,c6,c6,c6,c6,1); if(SEL=5)draw_text_color(160-1+random(2),149+random(2),text_op6,c6,c6,c6,c6,0.3); draw_set_halign(fa_left); }}"))
        },
        _ => None,
    }
}
