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
            // The title has five rows at 18 px without Mods, and six compact rows at 16 px with
            // Mods. Keep the hitboxes on the same layout as DrawInicioControl instead of always
            // using the compact positions: otherwise disabling Mods leaves the menu shifted up and
            // the bottom row with a gap under it.
            let s = r#"var y0,h,step; y0=75; h=13; step=18; if(global.mods_available){y0=71;step=16;} if(file_exists("mods/open.flag")) exit; if(mouse_x>=80 && mouse_x<=240 && mouse_y>=y0 && mouse_y<=y0+h){SEL=0;MOUSE=0;exit;} if(mouse_x>=80 && mouse_x<=240 && mouse_y>=y0+step && mouse_y<=y0+step+h){SEL=1;MOUSE=1;exit;} if(mouse_x>=80 && mouse_x<=240 && mouse_y>=y0+2*step && mouse_y<=y0+2*step+h){SEL=2;MOUSE=2;exit;} if(mouse_x>=80 && mouse_x<=240 && mouse_y>=y0+3*step && mouse_y<=y0+3*step+h){SEL=3;MOUSE=3;exit;} if(mouse_x>=80 && mouse_x<=240 && mouse_y>=y0+4*step && mouse_y<=y0+4*step+h){SEL=4;MOUSE=4;exit;} if(global.mods_available && mouse_x>=80 && mouse_x<=240 && mouse_y>=y0+5*step && mouse_y<=y0+5*step+11){SEL=5;MOUSE=5;exit;} MOUSE=(-1);"#;
            Some(s.to_string())
        },
        "OptionsControlMouse" => Some(source.replace("mouse_x>=62 && mouse_x<=178", "mouse_x>=80 && mouse_x<=240")),
        "InitializeInicioControl" => {
            let mut s = source.to_string();
            s.push_str(" if(file_exists(\"mods/active.flag\")){global.mods_available=true; var tmp; tmp=text_op5; text_op5=\"Mods\"; text_op6=tmp;}else{global.mods_available=false;text_op6=\"\";}");
            Some(s)
        },
        "KeyActionsInicioControl" => {
            let mut s = String::new();
            // The native Mods screen owns the menu; never let title input fall through while it is open.
            s.push_str(" if(file_exists(\"mods/open.flag\")) exit; ");
            // Never let Enter/Z fall through to the old "Salir del juego" action while the Mods row
            // is selected.
            s.push_str(" if(global.mods_available && SEL=4 && (keyboard_check(key_ini_entrar)||keyboard_check(key_ini_accion))){ exit; } ");
            s.push_str(source);
            Some(s)
        },
        "DrawInicioControl" => {
            // Rows are laid out from y0/step so the same script works both ways: five original
            // rows (74 + 18n) when Mods is unavailable, or six compact rows (70 + 16n, with Mods
            // at 150) when it is available. The old unconditional coordinate shift compressed the
            // menu even after Mods was removed.
            let s = source
                .replace(
                    "if(SEL=4){col1=c_white;col2=c_white;col3=c_white;col4=c_white;col5=c_red;}",
                    "if(SEL=4){col1=c_white;col2=c_white;col3=c_white;col4=c_white; if(global.mods_available)col5=c_aqua; else col5=c_red;} if(global.mods_available && SEL=5){col1=c_white;col2=c_white;col3=c_white;col4=c_white;col5=c_white;}",
                )
                .replace("draw_text_color(160,74", "draw_text_color(160,y0")
                .replace("draw_text_color(160,92", "draw_text_color(160,y0+step")
                .replace("draw_text_color(160,110", "draw_text_color(160,y0+2*step")
                .replace("draw_text_color(160,128", "draw_text_color(160,y0+3*step")
                .replace("draw_text_color(160,146", "draw_text_color(160,y0+4*step")
                .replace(",74-1+random(2)", ",y0-1+random(2)")
                .replace(",92-1+random(2)", ",y0+step-1+random(2)")
                .replace(",110-1+random(2)", ",y0+2*step-1+random(2)")
                .replace(",128-1+random(2)", ",y0+3*step-1+random(2)")
                .replace(",146-1+random(2)", ",y0+4*step-1+random(2)");
            let page = concat!(
                "if(file_exists(\"mods/open.flag\")){ ",
                "with(all) if(object_index=99 || object_index=104 || object_index=1452 || object_index=381) visible=false; ",
                "draw_set_font(0); draw_set_halign(fa_center); ",
                "draw_set_color(make_color_rgb(62,132,255)); draw_text(160,10,\"Mods\"); ",
                "draw_set_color(make_color_rgb(42,108,235)); draw_line(0,29,320,29); ",
                "draw_set_color(c_white); var f,line,i; f=file_text_open_read(\"mods/menu.txt\"); i=0; ",
                "while(!file_text_eof(f) && i<7){ line=file_text_read_string(f); draw_text(160,38+i*16,line); file_text_readln(f); i+=1;} ",
                "file_text_close(f); draw_set_color(c_yellow); draw_text(160,148,\"Volver\"); ",
                "draw_set_color(c_gray); draw_text(160,160,\"Clic en un mod: On/Off\"); ",
                "draw_set_halign(fa_left); exit;} ",
                "with(all) if(object_index=99 || object_index=104 || object_index=1452 || object_index=381) visible=true; "
            );
            let mut out = String::from(
                "var y0,step; y0=74; step=18; if(global.mods_available){y0=70;step=16;} ",
            );
            out.push_str(page);
            out.push_str(&s);
            out.push_str(" if(global.mods_available){ var c6; c6=c_white; if(SEL=5)c6=c_red; draw_set_font(0); draw_set_halign(fa_center); draw_text_color(160,y0+5*step,text_op6,c6,c6,c6,c6,1); if(SEL=5)draw_text_color(160-1+random(2),y0+5*step-1+random(2),text_op6,c6,c6,c6,c6,0.3); draw_set_halign(fa_left); }");
            Some(out)
        },
        // The Game Boy filter looks wrong with the day/night tint: the references are flat,
        // light handheld screens. While that mod is active, leave the lighting overlay off.
        "DrawOutdoorLighting" => Some(format!("if(file_exists(\"mods/gb.flag\")) exit; {source}")),
        "DayNightManageCielo" => Some(format!("if(file_exists(\"mods/gb.flag\")) exit; {source}")),
        "InitializeCielo" => Some(format!("if(file_exists(\"mods/gb.flag\")) exit; {source}")),
        _ => None,
    }
}
