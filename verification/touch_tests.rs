#![allow(dead_code)]
extern crate self as ramen;
#[path = "../vendor/ramen/src/input.rs"] pub mod input;
#[path = "../src/OpenGMK/gm8emulator/src/touch_controls.rs"] mod touch_controls;
#[path = "../src/OpenGMK/gm8emulator/src/touch_appearance.rs"] mod touch_appearance;
#[path = "../src/OpenGMK/gm8emulator/src/field_controls.rs"] mod field_controls;
#[path = "../src/OpenGMK/gm8emulator/src/touch_navigation.rs"] mod touch_navigation;
#[path = "../src/OpenGMK/gm8emulator/src/box_touch.rs"] mod box_touch;

#[test]
fn profiles_keep_automatic_switching_and_correct_key8_in_both_visual_styles() {
    use touch_controls as tc;
    tc::set_overlay_style(tc::OverlayStyle::Classic);
    tc::set_room(100);
    tc::set_modal_id(-1);
    let idx = |name: &str| tc::controls().iter().position(|c| c.name == name).unwrap();
    let eight = idx("alpha8");
    assert_eq!(tc::controls()[eight].key, input::Key::Alpha8);
    assert!(matches!(tc::controls()[eight].label, tc::Label::Text("8")));
    for style in ["modern", "classic"] {
        touch_appearance::load(&format!("touch-appearance-v1\nstyle={style}\n"));
        assert!(tc::is_active(eight));
        assert!(tc::is_active(idx("sel_world")));
        for n in 1..=7 { assert!(!tc::is_active(idx(&format!("alpha{n}")))); }
        match tc::window_shape_for(eight, 1536.0, 720.0) {
            tc::Shape::Circle { cx, cy, r } => {
                assert!((cx - 0.228).abs() < 0.0001);
                assert!((cy - 0.05).abs() < 0.0001);
                assert!((r - 0.022).abs() < 0.0001);
            },
            _ => panic!("button8 must retain the standard circular touch target"),
        }
        assert_eq!(tc::hit(0.228, 0.05, 1536.0, 720.0), Some(eight));
    }
    touch_navigation::report(200, -1, true);
    tc::set_room(200);
    assert_eq!(tc::current_context(), tc::MENU_NAV);
    assert!(tc::is_active(idx("z")) && tc::is_active(idx("x")));
    assert!(tc::movement_active());
    assert!(!tc::is_active(eight) && !tc::is_active(idx("v")));
    touch_navigation::report(8, -1, false);
    tc::set_room(8);
    tc::set_box_open(true);
    assert_eq!(tc::current_context(), tc::MENU);
    assert!(!tc::is_active(idx("z")) && !tc::movement_active());
    for room in [7,5,6] {
        touch_navigation::report(room, -1, false);
        tc::set_room(room);
        assert_eq!(tc::current_context(), tc::MENU_NAV);
        assert!(tc::is_active(idx("z")) && tc::is_active(idx("x")) && tc::movement_active());
    }
    touch_navigation::report(8, -1, false);
    tc::set_room(8);
    assert_eq!(tc::current_context(), tc::MENU);
    tc::set_modal_id(7778);
    assert_eq!(tc::current_context(), tc::MENU_NAV);
    tc::set_modal_id(-1);
    tc::set_box_open(false);
    touch_navigation::report(47, -1, false);
    tc::set_room(47);
    assert_eq!(tc::current_context(), tc::BATTLE);
    assert!(tc::is_active(idx("pkmn")));
    assert!(!tc::is_active(idx("sel_world")) && !tc::is_active(eight));
    touch_navigation::report(100, -1, false);
    tc::set_room(100);
    assert!(tc::is_active(eight));
}

#[test]
fn small_touch_stick_motion_preserves_gamepad_deadzone() {
    use touch_controls::{touch_stick_dirs as touch, stick_dirs as pad};
    assert_eq!(touch(0.0,0.0),[false;4]);
    assert_eq!(touch(0.11,0.0),[false,false,false,true]);
    assert_eq!(touch(0.0,-0.11),[true,false,false,false]);
    assert_eq!(touch(-0.08,0.08),[false,true,true,false]);
    assert_eq!(touch(f32::NAN,0.0),[false;4]);
    assert_eq!(pad(0.11,0.0),[false;4]);
}
