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
            let mut s = String::from("if(global.mods_menu_open || global.mods_ignore_mouse) exit; ");
            s.push_str(&source
                .replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")
                .replace("mouse_y>=75 && mouse_y<=88", "mouse_y>=71 && mouse_y<=84")
                .replace("mouse_y>=93 && mouse_y<=106", "mouse_y>=87 && mouse_y<=100")
                .replace("mouse_y>=111 && mouse_y<=124", "mouse_y>=103 && mouse_y<=116")
                .replace("mouse_y>=129 && mouse_y<=142", "mouse_y>=119 && mouse_y<=132")
                .replace("mouse_y>=147 && mouse_y<=160", "mouse_y>=135 && mouse_y<=148"));
            s.push_str(" if(global.mods_available && mouse_x>=62 && mouse_x<=178 && mouse_y>=151 && mouse_y<=162){SEL=5;MOUSE=5;exit;}");
            Some(s)
        },
        "OptionsControlMouse" => Some(source.replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")),
        "InitializeInicioControl" => {
            let mut s = source.to_string();
            s.push_str(" global.mods_menu_open=false; global.mods_ignore_mouse=false; global.mods_ignore_input=false; if(file_exists(\"mods/active.flag\")){global.mods_available=true; var tmp; tmp=text_op5; text_op5=\"Mods\"; text_op6=tmp;}else{global.mods_available=false;text_op6=\"\";}");
            Some(s)
        },
        "KeyActionsInicioControl" => {
            let mut s = String::new();
            s.push_str(" if(global.mods_menu_open){ if(mouse_check_button_pressed(mb_left)){global.mods_menu_open=false; global.mods_ignore_mouse=true; show_debug_message(\"[mods] close\"); with(all) if(object_index=99 || object_index=104 || object_index=1452 || object_index=381) visible=true;} exit;}");
            s.push_str(" if(!global.mods_menu_open && global.mods_available && mouse_check_button_pressed(mb_left) && mouse_x>=110 && mouse_x<=210 && mouse_y>=133 && mouse_y<=148){global.mods_menu_open=true; show_debug_message(\"[mods] open click\"); with(all) if(object_index=99 || object_index=104 || object_index=1452 || object_index=381) visible=false; exit;}");
            s.push_str(" if(global.mods_available && SEL=4){ if(keyboard_check(key_ini_entrar)||keyboard_check(key_ini_accion)){global.mods_menu_open=true; show_debug_message(\"[mods] open key\"); with(all) if(object_index=99 || object_index=104 || object_index=1452 || object_index=381) visible=false; exit;} if(keyboard_check(key_ini_subir)){SEL=3;exit;} if(keyboard_check(key_ini_bajar)){SEL=5;exit;} }");
            s.push_str(" if(global.mods_available && SEL=5){ if(keyboard_check(key_ini_entrar)||keyboard_check(key_ini_accion)){ ex(9498); exit;} if(keyboard_check(key_ini_subir)){SEL=4;exit;} if(keyboard_check(key_ini_bajar)){SEL=0;exit;} }");
            s.push_str(source);
            Some(s)
        },
        "DrawInicioControl" => {
            let mut s = String::new();
            s.push_str(" if(global.mods_menu_open){ draw_set_font(0); draw_set_halign(fa_center); draw_set_color(make_color_rgb(62,132,255)); draw_text(160,16,\"Mods\"); draw_set_color(make_color_rgb(42,108,235)); draw_line(16,30,304,30); draw_set_color(c_white); var f,line,i; f=file_text_open_read(\"mods/menu.txt\"); i=0; while(!file_text_eof(f) && i<10){ line=file_text_read_string(f); draw_text(160,50+i*12,line); file_text_readln(f); i+=1;} file_text_close(f); draw_set_color(c_yellow); draw_text(160,150,\"Volver\"); draw_set_halign(fa_left); exit; }");
            let s2 = source
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
            s.push_str(&s2);
            s.push_str(" if(global.mods_ignore_mouse) global.mods_ignore_mouse=false; ");
            s.push_str(" if(global.mods_available){ var c6; c6=c_white; if(SEL=5)c6=c_red; draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,150,text_op6,c6,c6,c6,c6,1); if(SEL=5)draw_text_color(160-1+random(2),149+random(2),text_op6,c6,c6,c6,c6,0.3); draw_set_halign(fa_left); }");
            Some(s)
        },
        _ => None,
    }
}
