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
        // `mouse_x >= 62 && mouse_x <= 178`. The right half of every entry is dead, and a click
        // there confirms whatever entry was selected before — which looks like the game jumping
        // to a different option. Widen the box to cover the text as drawn.
        "MouseActionsInicioControl" => {
            let mut s = source
                .replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")
                .replace("mouse_y>=75 && mouse_y<=88", "mouse_y>=63 && mouse_y<=76")
                .replace("mouse_y>=93 && mouse_y<=106", "mouse_y>=79 && mouse_y<=92")
                .replace("mouse_y>=111 && mouse_y<=124", "mouse_y>=95 && mouse_y<=108")
                .replace("mouse_y>=129 && mouse_y<=142", "mouse_y>=111 && mouse_y<=124")
                .replace("mouse_y>=147 && mouse_y<=160", "mouse_y>=127 && mouse_y<=140");
            s.push_str(" if(global.mods_available && mouse_x>=62 && mouse_x<=178 && mouse_y>=143 && mouse_y<=156){SEL=5;MOUSE=5;exit;}");
            Some(s)
        },
        "OptionsControlMouse" => Some(source.replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")),
        "InitializeInicioControl" => {
            let mut s = source.to_string();
            s.push_str(" global.mods_menu_open=false; if(file_exists(\"mods/active.flag\")){global.mods_available=true;text_op6=\"Mods\";}else{global.mods_available=false;text_op6=\"\";}");
            Some(s)
        },
        "KeyActionsInicioControl" => {
            let mut s = String::new();
            s.push_str(" if(global.mods_menu_open){ if(keyboard_check(key_ini_cancelar)||keyboard_check(key_ini_salir)){global.mods_menu_open=false;} exit;}");
            s.push_str(" if(global.mods_available && SEL=5){ if(keyboard_check(key_ini_entrar)||keyboard_check(key_ini_accion)){global.mods_menu_open=true;exit;} if(keyboard_check(key_ini_subir)){SEL=4;exit;} if(keyboard_check(key_ini_bajar)){SEL=0;exit;} }");
            s.push_str(" if(global.mods_available && SEL=4 && keyboard_check(key_ini_bajar)){SEL=5;exit;} ");
            s.push_str(source);
            Some(s)
        },
        "DrawInicioControl" => {
            let mut s = source
                .replace("draw_text_color(160,74", "draw_text_color(160,62")
                .replace("draw_text_color(160,92", "draw_text_color(160,78")
                .replace("draw_text_color(160,110", "draw_text_color(160,94")
                .replace("draw_text_color(160,128", "draw_text_color(160,110")
                .replace("draw_text_color(160,146", "draw_text_color(160,126")
                .replace(",74-1+random(2)", ",62-1+random(2)")
                .replace(",92-1+random(2)", ",78-1+random(2)")
                .replace(",110-1+random(2)", ",94-1+random(2)")
                .replace(",128-1+random(2)", ",110-1+random(2)")
                .replace(",146-1+random(2)", ",126-1+random(2)");
            s.push_str(" if(global.mods_available){ var c6; c6=c_white; if(SEL=5)c6=(16744448); draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,142,text_op6,c6,c6,c6,c6,1); if(SEL=5)draw_text_color(160-1+random(2),141+random(2),text_op6,c6,c6,c6,c6,0.3); draw_set_halign(fa_left); }");
            s.push_str(" if(global.mods_menu_open){ draw_set_color(c_black); draw_set_alpha(0.97); draw_rectangle(0,0,320,180,false); draw_set_alpha(1); draw_set_color(c_white); draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,14,\"Mods\",c_aqua,c_aqua,c_aqua,c_aqua,1); draw_set_halign(fa_left); draw_set_color(c_white); draw_text(24,30,\"Mods instalados\"); draw_line(16,42,304,42); var f,line,i; f=file_text_open_read(\"mods/menu.txt\"); i=0; while(!file_text_eof(f) && i<10){ line=file_text_read_string(f); draw_text(24,52+i*11,line); file_text_readln(f); i+=1;} file_text_close(f); draw_set_halign(fa_center); draw_text(160,164,\"ESC / X: Volver\"); draw_set_halign(fa_left); }");
            Some(s)
        },
        _ => None,
    }
}
