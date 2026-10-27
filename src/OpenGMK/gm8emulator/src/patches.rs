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
            let mut s = source.replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240");
            s.push_str(" if(global.mods_available && mouse_x>=62 && mouse_x<=178 && mouse_y>=163 && mouse_y<=177){SEL=5;MOUSE=5;exit;}");
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
            let mut s = source.to_string();
            s.push_str(" if(global.mods_available){ var c6; c6=c_white; if(SEL=5)c6=(16744448); draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,164,text_op6,c6,c6,c6,c6,1); if(SEL=5)draw_text_color(160-1+random(2),164-1+random(2),text_op6,c6,c6,c6,c6,0.3); draw_set_halign(fa_left); }");
            s.push_str(" if(global.mods_menu_open){ draw_set_color(c_black); draw_set_alpha(0.9); draw_rectangle(30,28,290,172,false); draw_set_alpha(1); draw_set_color(c_white); draw_set_font(0); draw_set_halign(fa_left); draw_text(42,34,\"Mods\"); var f,line,i; f=file_text_open_read(\"mods/menu.txt\"); i=0; while(!file_text_eof(f) && i<10){ line=file_text_read_string(f); draw_text(42,50+i*10,line); file_text_readln(f); i+=1;} file_text_close(f); draw_text(42,158,\"ESC: cerrar\"); draw_set_halign(fa_left); }");
            Some(s)
        },
        _ => None,
    }
}
