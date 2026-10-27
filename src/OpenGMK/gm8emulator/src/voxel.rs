//! Experimental voxel view of the current room. It is deliberately independent from the
//! normal GameMaker drawing path so it can be enabled without touching the game's scripts.

use crate::{
    game::{Game, Version},
    gml,
    math::Real,
    render::PrimitiveType,
};

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(0.00001);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = norm(sub(target, eye));
    let s = norm(cross(f, up));
    let u = cross(s, f);
    [
        s[0], u[0], f[0], 0.0,
        s[1], u[1], f[1], 0.0,
        s[2], u[2], f[2], 0.0,
        -dot(s, eye), -dot(u, eye), -dot(f, eye), 1.0,
    ]
}

fn quad(r: &mut crate::render::Renderer, p: [[f32; 3]; 4], col: i32, alpha: f64) {
    for i in [0, 1, 2, 0, 2, 3] {
        let v = p[i];
        r.vertex_3d(v[0].into(), v[1].into(), v[2].into(), 0.0, 0.0, -1.0, 0.0, 0.0, col, alpha);
    }
}

fn cube(r: &mut crate::render::Renderer, x: f32, y: f32, w: f32, h: f32, z0: f32, z1: f32, col: i32, alpha: f64) {
    let x1 = x - w / 2.0;
    let x2 = x + w / 2.0;
    let y1 = y - h / 2.0;
    let y2 = y + h / 2.0;
    // top, bottom, and four sides
    quad(r, [[x1, y1, z1], [x2, y1, z1], [x2, y2, z1], [x1, y2, z1]], col, alpha);
    quad(r, [[x1, y1, z0], [x2, y1, z0], [x2, y2, z0], [x1, y2, z0]], col, alpha);
    quad(r, [[x1, y1, z0], [x2, y1, z0], [x2, y1, z1], [x1, y1, z1]], col, alpha);
    quad(r, [[x1, y2, z0], [x2, y2, z0], [x2, y2, z1], [x1, y2, z1]], col, alpha);
    quad(r, [[x1, y1, z0], [x1, y2, z0], [x1, y2, z1], [x1, y1, z1]], col, alpha);
    quad(r, [[x2, y1, z0], [x2, y2, z0], [x2, y2, z1], [x2, y1, z1]], col, alpha);
}

pub fn draw(game: &mut Game, src_x: i32, src_y: i32, src_w: i32, src_h: i32) -> gml::Result<()> {
    let cx = f32::from(src_x as i16) + src_w as f32 / 2.0;
    let cy = f32::from(src_y as i16) + src_h as f32 / 2.0;
    let eye = [cx, cy - 220.0, 150.0];
    game.renderer.set_3d(true);
    game.renderer.set_perspective(false);
    game.renderer.set_projection_ortho(cx.into(), cy.into(), 320.0, 180.0, 0.0);
    game.renderer.set_view_matrix(look_at(eye, [cx, cy, 0.0], [0.0, 0.0, 1.0]));
    game.renderer.clear_view(game.room.colour, 1.0);
    game.renderer.set_depth_test(true);
    game.renderer.set_write_depth(true);
    game.renderer.set_depth(0.0);

    game.renderer.reset_primitive_3d(PrimitiveType::TriList, None);
    let tile_list = &game.room.tile_list;
    let mut it = tile_list.iter_by_drawing();
    while let Some(idx) = it.next(tile_list) {
        let tile = tile_list.get(idx);
        if !tile.visible.get() {
            continue;
        }
        let x = f64::from(tile.x.get()) as f32;
        let y = f64::from(tile.y.get()) as f32;
        if (x - cx).abs() > 220.0 || (y - cy).abs() > 220.0 {
            continue;
        }
        let w = tile.width.get() as f32;
        let h = tile.height.get() as f32;
        let d = f64::from(tile.depth.get());
        // Tiles above the floor (negative depth in GM rooms) become blocks; the rest is floor.
        let z1 = if d < 0.0 { 24.0 } else { 0.0 };
        let col = if z1 > 0.0 { 0x806040 } else { 0x304050 };
        cube(&mut game.renderer, x, y, w, h, 0.0, z1, col, 1.0);
    }
    game.renderer.draw_primitive_3d();

    let mut instances = game.room.instance_list.iter_by_drawing();
    while let Some(idx) = instances.next(&game.room.instance_list) {
        let inst = game.room.instance_list.get(idx);
        let x = f64::from(inst.x.get()) as f32;
        let y = f64::from(inst.y.get()) as f32;
        if (x - cx).abs() > 220.0 || (y - cy).abs() > 220.0 {
            continue;
        }
        game.renderer.reset_primitive_3d(PrimitiveType::TriList, None);
        cube(&mut game.renderer, x, y, 12.0, 12.0, 0.0, 24.0, 0xC04040, 1.0);
        game.renderer.draw_primitive_3d();
    }

    game.renderer.set_3d(false);
    Ok(())
}
