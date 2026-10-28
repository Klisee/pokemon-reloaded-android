//! Presentation-only 3D view of the current room. Game logic and collision stay in 2D.

use crate::{
    game::{Game, GetAsset, Version},
    gml,
    render::PrimitiveType,
};

/// Camera settings for the optional voxel view.
#[derive(Clone, Copy, Debug)]
pub struct VoxelSettings {
    /// Camera pitch in degrees. 0 is nearly horizontal, 90 is top-down.
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for VoxelSettings {
    fn default() -> Self {
        Self { pitch: 35.0, zoom: 1.0 }
    }
}

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
fn perspective(fov_degrees: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_degrees.to_radians() / 2.0).tan();
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, (far + near) / (near - far), (2.0 * far * near) / (near - far),
        0.0, 0.0, -1.0, 0.0,
    ]
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

fn textured_quad(
    renderer: &mut crate::render::Renderer,
    points: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    colour: i32,
    alpha: f64,
) {
    for i in [0, 1, 2, 0, 2, 3] {
        let p = points[i];
        let t = uv[i];
        renderer.vertex_3d(
            p[0].into(),
            p[1].into(),
            p[2].into(),
            0.0,
            0.0,
            1.0,
            t[0].into(),
            t[1].into(),
            colour,
            alpha,
        );
    }
}

fn draw_floor(game: &mut Game, cx: f32, cy: f32) {
    let tile_list = &game.room.tile_list;
    let mut iter = tile_list.iter_by_drawing();
    while let Some(index) = iter.next(tile_list) {
        let tile = tile_list.get(index);
        if !tile.visible.get() {
            continue;
        }
        let x = f64::from(tile.x.get()) as f32;
        let y = f64::from(tile.y.get()) as f32;
        if (x - cx).abs() > 360.0 || (y - cy).abs() > 360.0 {
            continue;
        }
        let width = tile.width.get() as f32;
        let height = tile.height.get() as f32;
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        let Some(Some(background)) = game.assets.backgrounds.get(tile.background_index.get() as usize) else {
            continue;
        };
        let Some(atlas) = background.atlas_ref else { continue };
        let (texture_width, texture_height) = (background.width.max(1) as f32, background.height.max(1) as f32);
        let tx = tile.tile_x.get() as f32;
        let ty = tile.tile_y.get() as f32;
        let u0 = tx / texture_width;
        let v0 = ty / texture_height;
        let u1 = (tx + width) / texture_width;
        let v1 = (ty + height) / texture_height;
        game.renderer.reset_primitive_3d(PrimitiveType::TriList, Some(atlas));
        textured_quad(
            &mut game.renderer,
            [
                [x, y, 0.0],
                [x + width, y, 0.0],
                [x + width, y + height, 0.0],
                [x, y + height, 0.0],
            ],
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            0xffffff,
            tile.alpha.get().into_inner(),
        );
        game.renderer.draw_primitive_3d();
    }
}


fn draw_wall(
    game: &mut Game,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    atlas: crate::render::atlas::AtlasRef,
    colour: i32,
    alpha: f64,
) {
    let half_w = width.max(16.0) / 2.0;
    let half_d = 4.0;
    let bottom = 0.1;
    let top = height.max(24.0);
    game.renderer.reset_primitive_3d(PrimitiveType::TriList, Some(atlas));
    let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let front = [
        [x - half_w, y - half_d, bottom],
        [x + half_w, y - half_d, bottom],
        [x + half_w, y - half_d, top],
        [x - half_w, y - half_d, top],
    ];
    let back = [
        [x + half_w, y + half_d, bottom],
        [x - half_w, y + half_d, bottom],
        [x - half_w, y + half_d, top],
        [x + half_w, y + half_d, top],
    ];
    let left = [
        [x - half_w, y + half_d, bottom],
        [x - half_w, y - half_d, bottom],
        [x - half_w, y - half_d, top],
        [x - half_w, y + half_d, top],
    ];
    let right = [
        [x + half_w, y - half_d, bottom],
        [x + half_w, y + half_d, bottom],
        [x + half_w, y + half_d, top],
        [x + half_w, y - half_d, top],
    ];
    let top_face = [
        [x - half_w, y - half_d, top],
        [x + half_w, y - half_d, top],
        [x + half_w, y + half_d, top],
        [x - half_w, y + half_d, top],
    ];
    for face in [front, back, left, right, top_face] {
        textured_quad(&mut game.renderer, face, uv, colour, alpha);
    }
    game.renderer.draw_primitive_3d();
}

fn draw_billboard(
    game: &mut Game,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    atlas: crate::render::atlas::AtlasRef,
    colour: i32,
    alpha: f64,
) {
    let half = width / 2.0;
    game.renderer.reset_primitive_3d(PrimitiveType::TriList, Some(atlas));
    textured_quad(
        &mut game.renderer,
        [
            [x - half, y, 0.1],
            [x + half, y, 0.1],
            [x + half, y, height],
            [x - half, y, height],
        ],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        colour,
        alpha,
    );
    game.renderer.draw_primitive_3d();
}

fn draw_instances(game: &mut Game, cx: f32, cy: f32) {
    let mut iter = game.room.instance_list.iter_by_drawing();
    while let Some(index) = iter.next(&game.room.instance_list) {
        let instance = game.room.instance_list.get(index);
        if !instance.visible.get() {
            continue;
        }
        let x = f64::from(instance.x.get()) as f32;
        let y = f64::from(instance.y.get()) as f32;
        if (x - cx).abs() > 360.0 || (y - cy).abs() > 360.0 {
            continue;
        }
        let Some(object) = game.assets.objects.get_asset(instance.object_index.get()) else {
            continue;
        };
        let object_name = object.name.to_string().to_ascii_lowercase();
        let is_wall = object_name.contains("pared") || object_name.contains("wall");
        if f64::from(instance.depth.get()) < -1000.0 && !is_wall {
            // HUD and screen-space controls are drawn by the 2D pass; a world billboard
            // would put them in the wrong projection.
            continue;
        }
        let Some(sprite) = game.assets.sprites.get_asset(instance.sprite_index.get()) else {
            continue;
        };
        let Some(atlas) = sprite.get_atlas_ref(instance.image_index.get().floor().to_i32()) else {
            continue;
        };
        let frame_width = sprite.width as f32 * f64::from(instance.image_xscale.get()) as f32;
        let frame_height = sprite.height as f32 * f64::from(instance.image_yscale.get()) as f32;
        if frame_width <= 0.0 || frame_height <= 0.0 {
            continue;
        }
        if is_wall {
            draw_wall(
                game,
                x,
                y,
                frame_width.max(16.0),
                28.0,
                atlas,
                instance.image_blend.get(),
                instance.image_alpha.get().into_inner(),
            );
        } else {
            draw_billboard(
                game,
                x,
                y,
                frame_width,
                frame_height.max(8.0),
                atlas,
                instance.image_blend.get(),
                instance.image_alpha.get().into_inner(),
            );
        }
    }
}

pub fn draw(
    game: &mut Game,
    src_x: i32,
    src_y: i32,
    src_w: i32,
    src_h: i32,
    settings: VoxelSettings,
) -> gml::Result<()> {
    let cx = f32::from(src_x as i16) + src_w as f32 / 2.0;
    let cy = f32::from(src_y as i16) + src_h as f32 / 2.0;
    let pitch = settings.pitch.clamp(5.0, 89.0).to_radians();
    let distance = 220.0 / settings.zoom.clamp(0.25, 4.0);
    let eye = [cx, cy - distance * pitch.cos(), distance * pitch.sin()];
    let target = [cx, cy, 0.0];

    game.renderer.set_3d(true);
    game.renderer.set_perspective(true);
    game.renderer.set_viewproj_matrix(
        look_at(eye, target, [0.0, 0.0, 1.0]),
        perspective(42.0, 320.0 / 180.0, 1.0, 1200.0),
    );
    game.renderer.clear_view(game.room.colour, 1.0);
    game.renderer.set_depth_test(true);
    game.renderer.set_write_depth(true);
    game.renderer.set_alpha_blending(true);
    game.renderer.set_lighting_enabled(false);
    game.renderer.set_fog(None);

    draw_floor(game, cx, cy);
    draw_instances(game, cx, cy);

    game.renderer.set_fog(None);
    game.renderer.set_3d(false);
    game.renderer.set_depth_test(false);
    game.renderer.set_write_depth(false);
    Ok(())
}
