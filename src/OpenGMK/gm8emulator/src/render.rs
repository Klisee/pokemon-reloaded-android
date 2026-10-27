//! Game rendering functionality

pub mod atlas;
pub mod opengl;

use crate::types::Colour;
use atlas::{AtlasRect, AtlasRef};
use ramen::{connection::Connection, window::Window};
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Scaling {
    /// Fixed scale, with a multiplier. The multiplier must always be strictly positive.
    Fixed(f64),
    /// Scale with window, but preserve aspect ratio.
    /// The f64 must be strictly negative and has no meaning, but can still be accessed with window_get_region_scale().
    Aspect(f64),
    /// Scale to fill window.
    Full,
}

/// Where the game image lands in the window: the rectangle the renderer blits into, and the one the
/// input mapping has to invert. It lives in one place so the picture and the touches cannot drift
/// apart. On Android, while the system keyboard is up the picture slides up - the way Android itself
/// pans a window with a focused text field - just far enough to keep the text the game is editing
/// above the keyboard.
pub fn display_rect(
    window_width: u32,
    window_height: u32,
    fb_width: u32,
    fb_height: u32,
    scaling: Scaling,
) -> (i32, i32, i32, i32) {
    let (window_width, window_height) = (window_width as i32, window_height as i32);
    let (fb_width, fb_height) = (fb_width as i32, fb_height as i32);
    let (x, y, w, h) = window_rect(window_width, window_height, fb_width, fb_height, scaling);
    // The effective height: zero outside the game's text dialog, so a stale height cannot leave the
    // picture stuck up top.
    let inset = crate::touch_controls::text_entry_inset() as i32;
    if inset <= 0 || window_height <= 0 || h <= 0 {
        return (x, y, w, h)
    }
    // The keyboard is up. Scaling the game down to fit above it did leave the text on screen, but
    // made it tiny; sliding the picture is what a phone does with a focused text field, and it
    // keeps the game at the size the player chose.
    //
    // The game's text-entry modal has a fixed layout, so the bottom of the box it types into sits at
    // a fixed fraction of the picture: that is what has to clear the keyboard.
    const TEXT_BOTTOM: f64 = 0.42;
    /// A little air between the text and the keyboard, as a fraction of the window height.
    const COMFORT: f64 = 0.06;
    let usable = (window_height - inset).max(1);
    let text_bottom = y + (f64::from(h) * TEXT_BOTTOM).round() as i32;
    let wanted = text_bottom - usable + (f64::from(window_height) * COMFORT).round() as i32;
    // Never so far that the picture's top shows a gap at the bottom, and never so far that the text
    // would leave the screen upwards.
    let limit = (h - usable).min(text_bottom).max(0);
    let pan = wanted.clamp(0, limit);
    (x, y - pan, w, h)
}

/// The game image's rectangle with the whole window to itself, whichever scaling mode is in use.
fn window_rect(window_width: i32, window_height: i32, fb_width: i32, fb_height: i32, scaling: Scaling) -> (i32, i32, i32, i32) {
    match scaling {
        Scaling::Fixed(scale) => {
            let w = (f64::from(fb_width) * scale) as i32;
            let h = (f64::from(fb_height) * scale) as i32;
            ((window_width - w) / 2, (window_height - h) / 2, w, h)
        },
        Scaling::Aspect(_) => {
            if fb_width > 0 && fb_height > 0 {
                let fixed_width = window_height * fb_width / fb_height;
                if fixed_width < window_width {
                    // window is too wide
                    ((window_width - fixed_width) / 2, 0, fixed_width, window_height)
                } else {
                    // window is too tall
                    let fixed_height = window_width * fb_height / fb_width;
                    (0, (window_height - fixed_height) / 2, window_width, fixed_height)
                }
            } else {
                // can never be too careful
                (0, 0, fb_width, fb_height)
            }
        },
        Scaling::Full => (0, 0, window_width, window_height),
    }
}

/// The video mode the launcher pinned through the environment, if any.
///
/// Deliberately not cached: the engine and the launcher share one process, so the value can change
/// between one run of the game and the next, and a cached one would make the second choice stick.
/// `None` means the launcher said nothing this engine understands, which leaves the game's own
/// setting in charge. This exists because the game changes the scaling itself: it asks for "fill"
/// while loading a save, which undid the player's choice and stretched the picture back.
pub fn pinned_scaling() -> Option<Scaling> {
    let value = std::env::var_os("OPENGMK_SCALING")?;
    match value.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "aspect" => Some(Scaling::Aspect(-1.0)),
        "full" => Some(Scaling::Full),
        _ => None,
    }
}

/// How the game is presented when the room runs slower than the screen.
///
/// The game itself always steps at its room speed. `Auto` and `Uncapped` decouple drawing from that
/// step and interpolate instance positions, so the picture can run at the screen's rate while the
/// simulation keeps its original timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// One drawn frame per game step, exactly like the original runner.
    Native,
    /// Draw as fast as the display lets us (vsync), interpolating between game steps.
    Auto,
    /// Interpolate but pace drawing to a fixed number of frames per second. Useful when vsync is
    /// not honoured by the driver and the display's native rate is known.
    Fixed(u32),
    /// Do not wait for anything; draw as fast as the device can.
    Uncapped,
}

impl RenderMode {
    pub fn interpolates(self) -> bool {
        !matches!(self, RenderMode::Native)
    }
}

/// The display refresh rate the launcher detected, used as a ceiling for `Auto` when vsync does
/// not actually pace the swap.
pub fn pinned_render_hz() -> Option<u32> {
    let value = std::env::var("OPENGMK_RENDER_HZ").ok()?;
    value.trim().parse::<u32>().ok().filter(|hz| *hz > 0).map(|hz| hz.min(1000))
}

/// The render rate the launcher pinned through the environment, if any. Absent means `Native`.
pub fn pinned_render_mode() -> RenderMode {
    let value = std::env::var("OPENGMK_RENDER").unwrap_or_default();
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "auto" => RenderMode::Auto,
        "uncapped" | "unlimited" | "max" => RenderMode::Uncapped,
        // A number is a fixed cap, for drivers where vsync does not actually pace the swap.
        other => match other.parse::<u32>() {
            Ok(fps) if fps > 0 => RenderMode::Fixed(fps.min(1000)),
            _ => RenderMode::Native,
        },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedTexture {
    width: i32,
    height: i32,
    pixels: Box<[u8]>,
    zbuf: Option<Box<[f32]>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fog {
    pub colour: i32,
    pub begin: f32,
    pub end: f32,
}

/// Optional CRT-style post-process applied to the entire presented frame.
///
/// Values are deliberately plain floats so they can be read from a mod's `mod.ini` without pulling
/// the renderer into the mod loader's data model.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrtSettings {
    pub curvature: f32,
    pub scanline: f32,
    /// Number of scanline bands over the height of the picture. Kept independent of the window
    /// resolution so a fullscreen picture has the same CRT texture as a windowed one.
    pub scanline_count: f32,
    pub mask: f32,
    pub aberration: f32,
    pub vignette: f32,
    pub blur: f32,
    pub noise: f32,
    pub brightness: f32,
    pub saturation: f32,
    pub glow: f32,
    pub corner: f32,
}

impl Default for CrtSettings {
    fn default() -> Self {
        Self {
            curvature: 0.055,
            scanline: 0.40,
            scanline_count: 360.0,
            mask: 0.60,
            aberration: 0.0035,
            vignette: 0.35,
            blur: 0.0,
            noise: 0.014,
            brightness: 1.18,
            saturation: 1.08,
            glow: 0.15,
            corner: 0.07,
        }
    }
}

impl CrtSettings {
    /// A no-op CRT setup, used when only another post-process (Game Boy palette) is active.
    pub fn neutral() -> Self {
        Self {
            curvature: 0.0,
            scanline: 0.0,
            scanline_count: 0.0,
            mask: 0.0,
            aberration: 0.0,
            vignette: 0.0,
            blur: 0.0,
            noise: 0.0,
            brightness: 1.0,
            saturation: 1.0,
            glow: 0.0,
            corner: 0.0,
        }
    }
}

/// The four-shade, low-colour Game Boy look.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GbSettings {
    pub strength: f32,
    pub dither: f32,
    pub contrast: f32,
    pub brightness: f32,
}

impl Default for GbSettings {
    fn default() -> Self {
        Self { strength: 1.0, dither: 0.25, contrast: 0.90, brightness: 1.02 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Light {
    Directional { direction: [f32; 3], colour: i32 },
    Point { position: [f32; 3], range: f32, colour: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendType {
    Zero,
    One,
    SrcColour,
    InvSrcColour,
    SrcAlpha,
    InvSrcAlpha,
    DestAlpha,
    InvDestAlpha,
    DestColour,
    InvDestColour,
    SrcAlphaSaturate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveType {
    PointList,
    LineList,
    LineStrip,
    TriList,
    TriStrip,
    TriFan,
}

impl From<i32> for PrimitiveType {
    fn from(pt: i32) -> Self {
        match pt {
            0 | 1 => PrimitiveType::PointList,
            2 => PrimitiveType::LineList,
            3 => PrimitiveType::LineStrip,
            4 => PrimitiveType::TriList,
            5 => PrimitiveType::TriStrip,
            6 => PrimitiveType::TriFan,
            _ => PrimitiveType::PointList, // GM8 just draws nothing in this case
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PrimitiveShape {
    Point,
    Line,
    Triangle,
}

impl From<PrimitiveType> for PrimitiveShape {
    fn from(pt: PrimitiveType) -> Self {
        match pt {
            PrimitiveType::PointList => PrimitiveShape::Point,
            PrimitiveType::LineList | PrimitiveType::LineStrip => PrimitiveShape::Line,
            PrimitiveType::TriList | PrimitiveType::TriStrip | PrimitiveType::TriFan => PrimitiveShape::Triangle,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[repr(C)]
struct Vertex {
    pub pos: [f32; 3],
    /// How far this vertex was from where it should be, in world pixels, when it was submitted.
    ///
    /// The shader subtracts this by `interp_amount`, so the same frame can be presented at any point
    /// between the previous game step and the current one without touching the game state.
    ///
    /// Render-only data: keeping it out of savestates also keeps old savestates loadable.
    #[serde(skip)]
    pub interp: [f32; 2],
    pub tex_coord: [f32; 2],
    pub blend: [f32; 4],
    pub atlas_xywh: [f32; 4],
    pub normal: [f32; 3],
}

/// A builder to be used for building primitives.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrimitiveBuilder {
    vertices: Vec<Vertex>,
    ptype: PrimitiveType,
    atlas_ref: AtlasRect,
    textured: bool,
}

impl PrimitiveBuilder {
    fn new(atlas_ref: AtlasRect, ptype: PrimitiveType, textured: bool) -> Self {
        Self { vertices: Vec::new(), ptype, atlas_ref, textured }
    }

    fn push_vertex_raw(&mut self, v: Vertex) -> &mut Self {
        // if we need to fill out a shape get the other two points
        let (v1, v2) = match self.ptype {
            PrimitiveType::PointList | PrimitiveType::LineList | PrimitiveType::TriList => (None, None),
            PrimitiveType::LineStrip => (self.vertices.last().filter(|_| self.vertices.len() >= 2).copied(), None),
            PrimitiveType::TriFan | PrimitiveType::TriStrip if self.vertices.len() < 3 => (None, None),
            PrimitiveType::TriStrip | PrimitiveType::TriFan => {
                (Some(self.vertices[self.vertices.len() - 2]), self.vertices.last().copied())
            },
        };
        if let Some(v1) = v1 {
            self.vertices.push(v1);
        }
        self.vertices.push(v);
        if let Some(v2) = v2 {
            self.vertices.push(v2);
            let len = self.vertices.len();
            if len % 6 == 3 && self.ptype == PrimitiveType::TriStrip {
                self.vertices.swap(len - 2, len - 1);
                self.vertices.swap(len - 3, len - 2);
            }
        } else if self.vertices.len() == 3 && self.ptype == PrimitiveType::TriFan {
            // i hate that this works
            self.vertices.swap(0, 1);
            self.vertices.swap(1, 2);
        }
        self
    }

    fn push_vertex(&mut self, pos: [f32; 3], tex_coord: [f32; 2], blend: [f32; 4], normal: [f32; 3]) -> &mut Self {
        // -1 marks an untextured primitive. NaN used to be the marker, but mobile GPUs (and their
        // interpolators) are allowed to flush it, which turned the texture coordinate into a real
        // sample and made every filled primitive -- the characters' shadows included -- vanish.
        let tex_coord = if self.textured { tex_coord } else { [-1.0; 2] };
        self.push_vertex_raw(Vertex {
            pos,
            interp: [0.0, 0.0],
            tex_coord,
            blend,
            normal,
            atlas_xywh: self.atlas_ref.into(),
        });
        self
    }

    fn get_atlas_id(&self) -> u32 {
        self.atlas_ref.atlas_id
    }

    fn get_shape(&self) -> PrimitiveShape {
        self.ptype.into()
    }

    fn get_vertices(&self) -> &[Vertex] {
        &self.vertices
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VertexBuffer {
    points: Vec<Vertex>,
    lines: Vec<Vertex>,
    tris: Vec<Vertex>,
}

pub struct Renderer(Box<dyn RendererTrait>);

pub trait RendererTrait {
    fn as_any(&self) -> &dyn Any;
    fn max_texture_size(&self) -> u32;
    fn push_atlases(&mut self, atl: atlas::AtlasBuilder) -> Result<(), String>;
    fn upload_sprite(
        &mut self,
        data: Box<[u8]>,
        width: i32,
        height: i32,
        origin_x: i32,
        origin_y: i32,
    ) -> Result<AtlasRef, String>;
    fn duplicate_sprite(&mut self, atlas_ref: AtlasRef) -> Result<AtlasRef, String>;
    fn delete_sprite(&mut self, atlas_ref: AtlasRef);

    /// Resizes the rendering target. Usually called when the window has been resized.
    /// The contents of the old framebuffer will be copied to the new one. If `store` is true, the old one
    /// will be stored for later drawing with `draw_stored()`; otherwise, it will be deleted.
    fn resize_framebuffer(&mut self, width: u32, height: u32, store: bool);

    fn set_vsync(&self, vsync: bool);
    fn get_vsync(&self) -> bool;
    fn wait_vsync(&self);

    /// Sets the interpolation offset applied to every vertex submitted from now on. Call it before
    /// each instance's draw event and reset it to zero afterwards.
    fn set_draw_offset(&mut self, x: f64, y: f64);
    /// How much of each vertex's interpolation offset to apply: 0 shows the current step, 1 shows
    /// the previous one.
    fn set_interp_amount(&mut self, amount: f32);

    fn get_rect(&self, id: AtlasRef) -> Option<&AtlasRect>;

    /// Updates a sprite's origin (in pixels). GM's `sprite_set_offset` changes the origin after the
    /// sprite has been uploaded, so the renderer has to be told about it too or the sprite keeps
    /// drawing with its old origin.
    fn set_sprite_origin(&mut self, id: AtlasRef, origin_x: f32, origin_y: f32);

    fn draw_sprite(&mut self, tex: AtlasRef, x: f64, y: f64, xs: f64, ys: f64, ang: f64, col: i32, alpha: f64) {
        if let Some(rect) = self.get_rect(tex) {
            let (w, h) = (rect.w, rect.h);
            self.draw_sprite_general(
                tex,
                0.0,
                0.0,
                w.into(),
                h.into(),
                x,
                y,
                xs,
                ys,
                ang,
                col,
                col,
                col,
                col,
                alpha,
                true,
            );
        }
    }

    fn draw_sprite_colour(
        &mut self,
        tex: AtlasRef,
        x: f64,
        y: f64,
        xs: f64,
        ys: f64,
        ang: f64,
        col1: i32,
        col2: i32,
        col3: i32,
        col4: i32,
        alpha: f64,
    ) {
        if let Some(rect) = self.get_rect(tex) {
            let (w, h) = (rect.w, rect.h);
            self.draw_sprite_general(
                tex,
                0.0,
                0.0,
                w.into(),
                h.into(),
                x,
                y,
                xs,
                ys,
                ang,
                col1,
                col2,
                col3,
                col4,
                alpha,
                true,
            );
        }
    }

    fn draw_sprite_general(
        &mut self,
        texture: AtlasRef,
        part_x: f64,
        part_y: f64,
        part_w: f64,
        part_h: f64,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        col1: i32,
        col2: i32,
        col3: i32,
        col4: i32,
        alpha: f64,
        use_origin: bool,
    );
    fn draw_sprite_pos(
        &mut self,
        texture: AtlasRef,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
        x4: f64,
        y4: f64,
        alpha: f64
    );
    fn set_view_matrix(&mut self, view: [f32; 16]);
    fn set_viewproj_matrix(&mut self, view: [f32; 16], proj: [f32; 16]);
    fn get_model_matrix(&self) -> [f32; 16];
    fn set_model_matrix(&mut self, model: [f32; 16]);
    fn mult_model_matrix(&mut self, model: [f32; 16]);
    fn set_projection_ortho(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64);
    fn set_projection_perspective(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64);
    fn set_view(
        &mut self,
        src_x: i32,
        src_y: i32,
        src_w: i32,
        src_h: i32,
        src_angle: f64,
        port_x: i32,
        port_y: i32,
        port_w: i32,
        port_h: i32,
    );
    fn flush_queue(&mut self);
    fn set_crt(&mut self, _settings: Option<CrtSettings>) {}
    fn set_gb(&mut self, _settings: Option<GbSettings>) {}
    fn set_gb_inverted(&mut self, _inverted: bool) {}
    fn present(&mut self, window_width: u32, window_height: u32, scaling: Scaling);
    /// Presents one surface, stretched over the whole window. Used by the game's information window,
    /// which is drawn at window resolution instead of the game's small framebuffer.
    fn present_surface(&mut self, surface: AtlasRef, window_width: u32, window_height: u32);
    fn draw_stored(&mut self, target_x: i32, target_y: i32, width: u32, height: u32);
    fn stored_size(&self) -> (u32, u32);
    fn finish(&mut self, window_width: u32, window_height: u32, clear_colour: Colour);

    fn dump_sprite(&self, atlas_ref: AtlasRef) -> Box<[u8]> {
        match self.get_rect(atlas_ref) {
            Some(rect) => self.dump_sprite_part(atlas_ref, 0, 0, rect.w, rect.h),
            None => Box::new([]),
        }
    }
    fn dump_sprite_part(&self, texture: AtlasRef, part_x: i32, part_y: i32, part_w: i32, part_h: i32) -> Box<[u8]>;
    fn get_alpha_blending(&self) -> bool;
    fn set_alpha_blending(&mut self, alphablend: bool);
    fn get_colour_blending(&self) -> bool;
    fn set_colour_blending(&mut self, modulate: bool);
    fn get_blend_mode(&self) -> (BlendType, BlendType);
    fn set_blend_mode(&mut self, src: BlendType, dst: BlendType);
    fn get_pixel_interpolation(&self) -> bool;
    fn set_pixel_interpolation(&mut self, lerping: bool);
    fn get_texture_repeat(&self) -> bool;
    fn set_texture_repeat(&mut self, repeat: bool);

    fn get_pixels(&self, x: i32, y: i32, w: i32, h: i32) -> Box<[u8]>;
    fn stored_pixels(&self) -> Box<[u8]>;
    fn stored_zbuffer(&self) -> Box<[f32]>;
    fn set_stored(&mut self, rgba: Box<[u8]>, zbuf: Box<[f32]>, fb_w: u32, fb_h: u32);

    fn dump_dynamic_textures(&self) -> Vec<Option<SavedTexture>>;
    fn upload_dynamic_textures(&mut self, textures: &[Option<SavedTexture>]);

    fn create_sprite_colour(&mut self, width: i32, height: i32, col: Colour) -> Result<AtlasRef, String>;
    fn create_surface(&mut self, w: i32, h: i32, has_zbuffer: bool) -> Result<AtlasRef, String>;
    fn set_target(&mut self, atlas_ref: AtlasRef);
    fn reset_target(&mut self);
    fn copy_surface(
        &mut self,
        dest: AtlasRef,
        dest_x: i32,
        dest_y: i32,
        src: AtlasRef,
        src_x: i32,
        src_y: i32,
        width: i32,
        height: i32,
    );

    fn set_zbuf_trashed(&mut self, trashed: bool);
    fn get_zbuf_trashed(&self) -> bool;

    fn get_texture_id(&mut self, atl_ref: AtlasRef) -> i32;
    fn get_texture_from_id(&self, id: i32) -> Option<AtlasRef>;

    fn get_texture_rects(&self) -> Vec<Option<AtlasRect>>;
    fn set_texture_rects(&mut self, rects: &[Option<AtlasRect>]);

    fn draw_sprite_partial(
        &mut self,
        texture: AtlasRef,
        part_x: f64,
        part_y: f64,
        part_w: f64,
        part_h: f64,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        colour: i32,
        alpha: f64,
    ) {
        self.draw_sprite_general(
            texture, part_x, part_y, part_w, part_h, x, y, xscale, yscale, angle, colour, colour, colour, colour,
            alpha, false,
        )
    }
    fn draw_sprite_tiled(
        &mut self,
        texture: AtlasRef,
        mut x: f64,
        mut y: f64,
        xscale: f64,
        yscale: f64,
        colour: i32,
        alpha: f64,
        tile_end_x: Option<f64>,
        tile_end_y: Option<f64>,
    ) {
        let (width, height) = match self.get_rect(texture) {
            Some(rect) => (f64::from(rect.w) * xscale, f64::from(rect.h) * yscale),
            None => return,
        };

        if tile_end_x.is_some() {
            x = x.rem_euclid(width);
            if x > 0.0 {
                x -= width;
            }
        }
        if tile_end_y.is_some() {
            y = y.rem_euclid(height);
            if y > 0.0 {
                y -= height;
            }
        }

        let start_x = x;

        loop {
            loop {
                self.draw_sprite(texture, x, y, xscale, yscale, 0.0, colour, alpha);
                x += width;
                match tile_end_x {
                    Some(end_x) if x < end_x => (),
                    _ => break,
                }
            }
            x = start_x;
            y += height;
            match tile_end_y {
                Some(end_y) if y < end_y => (),
                _ => break,
            }
        }
    }

    fn draw_rectangle(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64);
    fn draw_rectangle_outline(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64);
    fn draw_rectangle_gradient(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        c1: i32,
        c2: i32,
        c3: i32,
        c4: i32,
        alpha: f64,
        outline: bool,
    );
    fn draw_point(&mut self, x: f64, y: f64, colour: i32, alpha: f64);
    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, width: Option<f64>, c1: i32, c2: i32, alpha: f64);
    fn draw_triangle(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
        c1: i32,
        c2: i32,
        c3: i32,
        alpha: f64,
        outline: bool,
    );
    fn draw_ellipse(&mut self, x: f64, y: f64, rad_x: f64, rad_y: f64, c1: i32, c2: i32, alpha: f64, outline: bool);
    fn draw_roundrect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, c1: i32, c2: i32, alpha: f64, outline: bool);
    fn set_circle_precision(&mut self, prec: i32);
    fn get_circle_precision(&self) -> i32;
    fn reset_primitive_2d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>);
    fn vertex_2d(&mut self, x: f64, y: f64, xtex: f64, ytex: f64, col: i32, alpha: f64);
    fn draw_primitive_2d(&mut self);
    fn get_primitive_2d(&self) -> PrimitiveBuilder;
    fn set_primitive_2d(&mut self, prim: PrimitiveBuilder);
    fn reset_primitive_3d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>);
    fn vertex_3d(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        nx: f64,
        ny: f64,
        nz: f64,
        xtex: f64,
        ytex: f64,
        col: i32,
        alpha: f64,
    );
    fn draw_primitive_3d(&mut self);
    fn get_primitive_3d(&self) -> PrimitiveBuilder;
    fn set_primitive_3d(&mut self, prim: PrimitiveBuilder);
    fn extend_buffers(&self, buf: &mut VertexBuffer);
    fn draw_buffers(&mut self, atlas_ref: Option<AtlasRef>, buf: &VertexBuffer);
    fn clear_view(&mut self, colour: Colour, alpha: f64);
    fn clear_view_no_zbuf(&mut self, colour: Colour, alpha: f64);
    fn clear_zbuf(&mut self);

    fn get_3d(&self) -> bool;
    fn set_3d(&mut self, use_3d: bool);
    fn get_depth(&self) -> f32;
    fn set_depth(&mut self, depth: f32);
    fn get_depth_test(&self) -> bool;
    fn set_depth_test(&mut self, depth_test: bool);
    fn get_write_depth(&self) -> bool;
    fn set_write_depth(&mut self, write_depth: bool);
    fn get_culling(&self) -> bool;
    fn set_culling(&mut self, culling: bool);
    fn get_perspective(&self) -> bool;
    fn set_perspective(&mut self, perspective: bool);
    fn get_fog(&self) -> Option<Fog>;
    fn set_fog(&mut self, fog: Option<Fog>);
    fn get_gouraud(&self) -> bool;
    fn set_gouraud(&mut self, gouraud: bool);
    fn get_lighting_enabled(&self) -> bool;
    fn set_lighting_enabled(&mut self, enabled: bool);
    fn get_ambient_colour(&self) -> i32;
    fn set_ambient_colour(&mut self, colour: i32);
    fn get_lights(&self) -> [(bool, Light); 8];
    fn set_lights(&mut self, lights: [(bool, Light); 8]);
    fn set_light_enabled(&mut self, id: usize, enabled: bool);
    fn set_light(&mut self, id: usize, light: Light);
}

pub struct RendererOptions {
    pub size: (u32, u32),
    pub vsync: bool,
    pub interpolate_pixels: bool,
    pub normalize_normals: bool,
    pub zbuf_24: bool,
}

impl Default for RendererOptions {
    fn default() -> Self {
        RendererOptions {
            size: (8, 8),
            vsync: true,
            interpolate_pixels: false,
            normalize_normals: false,
            zbuf_24: false,
        }
    }
}

impl Renderer {
    pub fn new(backend: (), connection: &Connection, options: &RendererOptions, window: &Window, clear_colour: Colour) -> Result<Self, String> {
        Ok(Self(Box::new(match backend {
            () => opengl::RendererImpl::new(options, connection, window, clear_colour)?,
        })))
    }

    pub fn max_texture_size(&self) -> u32 {
        self.0.max_texture_size()
    }

    pub fn push_atlases(&mut self, atl: atlas::AtlasBuilder) -> Result<(), String> {
        self.0.push_atlases(atl)
    }

    pub fn upload_sprite(
        &mut self,
        data: Box<[u8]>,
        width: i32,
        height: i32,
        origin_x: i32,
        origin_y: i32,
    ) -> Result<AtlasRef, String> {
        self.0.upload_sprite(data, width, height, origin_x, origin_y)
    }

    pub fn duplicate_sprite(&mut self, atlas_ref: AtlasRef) -> Result<AtlasRef, String> {
        self.0.duplicate_sprite(atlas_ref)
    }

    pub fn set_sprite_origin(&mut self, atlas_ref: AtlasRef, origin_x: f32, origin_y: f32) {
        self.0.set_sprite_origin(atlas_ref, origin_x, origin_y)
    }

    pub fn delete_sprite(&mut self, atlas_ref: AtlasRef) {
        self.0.delete_sprite(atlas_ref)
    }

    pub fn check_texture_id(&self, id: i32) -> bool {
        self.0.get_rect(AtlasRef(id)).is_some()
    }

    pub fn set_vsync(&self, vsync: bool) {
        self.0.set_vsync(vsync)
    }

    pub fn get_vsync(&self) -> bool {
        self.0.get_vsync()
    }

    pub fn wait_vsync(&self) {
        self.0.wait_vsync()
    }

    pub fn set_draw_offset(&mut self, x: f64, y: f64) {
        self.0.set_draw_offset(x, y)
    }

    pub fn set_interp_amount(&mut self, amount: f32) {
        self.0.set_interp_amount(amount)
    }

    pub fn draw_sprite(
        &mut self,
        texture: AtlasRef,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        colour: i32,
        alpha: f64,
    ) {
        self.note_character_sprite(texture, x, y, xscale, yscale);
        self.0.draw_sprite(texture, x, y, xscale, yscale, angle, colour, alpha)
    }

    /// Notes a drawn sprite when it is big enough to be one of the character picker's characters.
    ///
    /// The overlay puts that screen's arrows beside the character, and the only thing that knows where
    /// the character ended up is the game's own drawing: every sprite passes through here, whichever
    /// of the draw functions the game used.
    fn note_character_sprite(&self, texture: AtlasRef, x: f64, y: f64, xscale: f64, yscale: f64) {
        let Some(rect) = self.0.get_rect(texture) else { return };
        // A sprite is drawn from its origin, which is not always its centre - the picker's characters
        // are anchored elsewhere, and assuming the centre put the box somewhere the character is not.
        // The origin is a fraction of the sprite's own size, and the scale is signed: a mirrored
        // sprite flips around it.
        let (w, h) = (f64::from(rect.w) * xscale, f64::from(rect.h) * yscale);
        let (left, top) = (x - f64::from(rect.origin_x) * w, y - f64::from(rect.origin_y) * h);
        self.note_character_box(left.min(left + w), top.min(top + h), left.max(left + w), top.max(top + h));
    }

    fn note_character_box(&self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let (half_w, half_h) = ((x2 - x1).abs() / 2.0, (y2 - y1).abs() / 2.0);
        let (picture_width, picture_height) = self.0.stored_size();
        // On the picture: the game also draws sprites parked far outside it (the picker's room has a
        // couple, and they move, which would drag the box along with them).
        let on_picture = x1 >= 0.0
            && y1 >= 0.0
            && x2 <= f64::from(picture_width)
            && y2 <= f64::from(picture_height);
        // A quarter of the picture tall at least: the characters take about half of it, while the
        // little cursor under each name is far smaller. And no more than half of it either way: the
        // room's background is the whole picture, and it would swallow everything else. The width cap
        // is what keeps the arrows on the character: without it, a wide sprite on those screens - the
        // one behind the name, the glow around the chosen one - took the box and the arrows went with
        // it, away from the character.
        let (w, h) = ((x2 - x1).abs(), (y2 - y1).abs());
        let character_sized = on_picture
            && h >= f64::from(picture_height) / 4.0
            && w <= f64::from(picture_width) * 0.5
            && h <= f64::from(picture_height) * 0.5;
        if character_sized {
            crate::touch_controls::note_sprite(x1, y1, x2, y2);
        }
    }

    pub fn draw_sprite_colour(
        &mut self,
        tex: AtlasRef,
        x: f64,
        y: f64,
        xs: f64,
        ys: f64,
        ang: f64,
        col1: i32,
        col2: i32,
        col3: i32,
        col4: i32,
        alpha: f64,
    ) {
        self.note_character_sprite(tex, x, y, xs, ys);
        self.0.draw_sprite_colour(tex, x, y, xs, ys, ang, col1, col2, col3, col4, alpha)
    }

    pub fn draw_sprite_general(
        &mut self,
        texture: AtlasRef,
        part_x: f64,
        part_y: f64,
        part_w: f64,
        part_h: f64,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        col1: i32,
        col2: i32,
        col3: i32,
        col4: i32,
        alpha: f64,
        use_origin: bool,
    ) {
        self.note_character_sprite(texture, x, y, xscale, yscale);
        self.0.draw_sprite_general(
            texture, part_x, part_y, part_w, part_h, x, y, xscale, yscale, angle, col1, col2, col3, col4, alpha,
            use_origin,
        )
    }

    pub fn draw_sprite_pos(
        &mut self,
        texture: AtlasRef,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
        x4: f64,
        y4: f64,
        alpha: f64,
    ) {
        self.0.draw_sprite_pos(
            texture, x1, y1, x2, y2, x3, y3, x4, y4, alpha
        )
    }

    pub fn set_view_matrix(&mut self, view: [f32; 16]) {
        self.0.set_view_matrix(view)
    }

    pub fn set_viewproj_matrix(&mut self, view: [f32; 16], proj: [f32; 16]) {
        self.0.set_viewproj_matrix(view, proj)
    }

    pub fn get_model_matrix(&self) -> [f32; 16] {
        self.0.get_model_matrix()
    }

    pub fn set_model_matrix(&mut self, model: [f32; 16]) {
        self.0.set_model_matrix(model)
    }

    pub fn mult_model_matrix(&mut self, model: [f32; 16]) {
        self.0.mult_model_matrix(model)
    }

    pub fn set_projection_ortho(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64) {
        self.0.set_projection_ortho(x, y, w, h, angle)
    }

    pub fn set_projection_perspective(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64) {
        self.0.set_projection_perspective(x, y, w, h, angle)
    }

    pub fn set_view(
        &mut self,
        src_x: i32,
        src_y: i32,
        src_w: i32,
        src_h: i32,
        src_angle: f64,
        port_x: i32,
        port_y: i32,
        port_w: i32,
        port_h: i32,
    ) {
        self.0.set_view(src_x, src_y, src_w, src_h, src_angle, port_x, port_y, port_w, port_h)
    }

    pub fn draw_sprite_partial(
        &mut self,
        texture: AtlasRef,
        part_x: f64,
        part_y: f64,
        part_w: f64,
        part_h: f64,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        colour: i32,
        alpha: f64,
    ) {
        self.0.draw_sprite_partial(texture, part_x, part_y, part_w, part_h, x, y, xscale, yscale, angle, colour, alpha)
    }

    pub fn draw_sprite_tiled(
        &mut self,
        texture: AtlasRef,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        colour: i32,
        alpha: f64,
        tile_end_x: Option<f64>,
        tile_end_y: Option<f64>,
    ) {
        self.0.draw_sprite_tiled(texture, x, y, xscale, yscale, colour, alpha, tile_end_x, tile_end_y)
    }

    pub fn draw_rectangle(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64) {
        self.0.draw_rectangle(x1, y1, x2, y2, colour, alpha)
    }

    pub fn draw_rectangle_outline(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64) {
        self.0.draw_rectangle_outline(x1, y1, x2, y2, colour, alpha)
    }

    pub fn draw_rectangle_gradient(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        c1: i32,
        c2: i32,
        c3: i32,
        c4: i32,
        alpha: f64,
        outline: bool,
    ) {
        self.0.draw_rectangle_gradient(x1, y1, x2, y2, c1, c2, c3, c4, alpha, outline)
    }

    pub fn draw_point(&mut self, x: f64, y: f64, colour: i32, alpha: f64) {
        self.0.draw_point(x, y, colour, alpha)
    }

    pub fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, width: Option<f64>, c1: i32, c2: i32, alpha: f64) {
        self.0.draw_line(x1, y1, x2, y2, width, c1, c2, alpha)
    }

    pub fn draw_triangle(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
        c1: i32,
        c2: i32,
        c3: i32,
        alpha: f64,
        outline: bool,
    ) {
        self.0.draw_triangle(x1, y1, x2, y2, x3, y3, c1, c2, c3, alpha, outline)
    }

    pub fn draw_ellipse(
        &mut self,
        x: f64,
        y: f64,
        rad_x: f64,
        rad_y: f64,
        c1: i32,
        c2: i32,
        alpha: f64,
        outline: bool,
    ) {
        self.0.draw_ellipse(x, y, rad_x, rad_y, c1, c2, alpha, outline)
    }

    pub fn draw_roundrect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, c1: i32, c2: i32, alpha: f64, outline: bool) {
        self.0.draw_roundrect(x1, y1, x2, y2, c1, c2, alpha, outline)
    }

    pub fn set_circle_precision(&mut self, prec: i32) {
        self.0.set_circle_precision(prec)
    }

    pub fn get_circle_precision(&self) -> i32 {
        self.0.get_circle_precision()
    }

    pub fn reset_primitive_2d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>) {
        self.0.reset_primitive_2d(ptype, atlas_ref)
    }

    pub fn vertex_2d(&mut self, x: f64, y: f64, xtex: f64, ytex: f64, col: i32, alpha: f64) {
        self.0.vertex_2d(x, y, xtex, ytex, col, alpha)
    }

    pub fn draw_primitive_2d(&mut self) {
        self.0.draw_primitive_2d()
    }

    pub fn get_primitive_2d(&self) -> PrimitiveBuilder {
        self.0.get_primitive_2d()
    }

    pub fn set_primitive_2d(&mut self, prim: PrimitiveBuilder) {
        self.0.set_primitive_2d(prim)
    }

    pub fn reset_primitive_3d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>) {
        self.0.reset_primitive_3d(ptype, atlas_ref)
    }

    pub fn vertex_3d(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        nx: f64,
        ny: f64,
        nz: f64,
        xtex: f64,
        ytex: f64,
        col: i32,
        alpha: f64,
    ) {
        self.0.vertex_3d(x, y, z, nx, ny, nz, xtex, ytex, col, alpha)
    }

    pub fn draw_primitive_3d(&mut self) {
        self.0.draw_primitive_3d()
    }

    pub fn get_primitive_3d(&self) -> PrimitiveBuilder {
        self.0.get_primitive_3d()
    }

    pub fn set_primitive_3d(&mut self, prim: PrimitiveBuilder) {
        self.0.set_primitive_3d(prim)
    }

    pub fn extend_buffers(&self, buf: &mut VertexBuffer) {
        self.0.extend_buffers(buf)
    }

    pub fn draw_buffers(&mut self, atlas_ref: Option<AtlasRef>, buf: &VertexBuffer) {
        self.0.draw_buffers(atlas_ref, buf)
    }

    pub fn dump_sprite(&self, atlas_ref: AtlasRef) -> Box<[u8]> {
        self.0.dump_sprite(atlas_ref)
    }

    pub fn dump_sprite_part(&self, texture: AtlasRef, part_x: i32, part_y: i32, part_w: i32, part_h: i32) -> Box<[u8]> {
        self.0.dump_sprite_part(texture, part_x, part_y, part_w, part_h)
    }

    pub fn resize_framebuffer(&mut self, width: u32, height: u32, store: bool) {
        self.0.resize_framebuffer(width, height, store)
    }

    pub fn get_pixels(&self, x: i32, y: i32, w: i32, h: i32) -> Box<[u8]> {
        self.0.get_pixels(x, y, w, h)
    }

    pub fn stored_pixels(&self) -> Box<[u8]> {
        self.0.stored_pixels()
    }

    pub fn stored_zbuffer(&self) -> Box<[f32]> {
        self.0.stored_zbuffer()
    }

    pub fn set_stored(&mut self, rgba: Box<[u8]>, zbuf: Box<[f32]>, fb_w: u32, fb_h: u32) {
        self.0.set_stored(rgba, zbuf, fb_w, fb_h)
    }

    pub fn dump_dynamic_textures(&self) -> Vec<Option<SavedTexture>> {
        self.0.dump_dynamic_textures()
    }

    pub fn upload_dynamic_textures(&mut self, textures: &[Option<SavedTexture>]) {
        self.0.upload_dynamic_textures(textures)
    }

    pub fn create_sprite_colour(&mut self, width: i32, height: i32, col: Colour) -> Result<AtlasRef, String> {
        self.0.create_sprite_colour(width, height, col)
    }

    pub fn create_surface(&mut self, w: i32, h: i32, has_zbuffer: bool) -> Result<AtlasRef, String> {
        self.0.create_surface(w, h, has_zbuffer)
    }

    pub fn set_target(&mut self, atlas_ref: AtlasRef) {
        self.0.set_target(atlas_ref)
    }

    pub fn reset_target(&mut self) {
        self.0.reset_target()
    }

    pub fn copy_surface(
        &mut self,
        dest: AtlasRef,
        dest_x: i32,
        dest_y: i32,
        src: AtlasRef,
        src_x: i32,
        src_y: i32,
        width: i32,
        height: i32,
    ) {
        self.0.copy_surface(dest, dest_x, dest_y, src, src_x, src_y, width, height)
    }

    pub fn set_zbuf_trashed(&mut self, trashed: bool) {
        self.0.set_zbuf_trashed(trashed)
    }

    pub fn get_zbuf_trashed(&self) -> bool {
        self.0.get_zbuf_trashed()
    }

    pub fn get_texture_id(&mut self, atl_ref: AtlasRef) -> i32 {
        self.0.get_texture_id(atl_ref)
    }

    pub fn get_texture_from_id(&self, id: i32) -> Option<AtlasRef> {
        self.0.get_texture_from_id(id)
    }

    pub fn get_texture_rects(&self) -> Vec<Option<AtlasRect>> {
        self.0.get_texture_rects()
    }

    pub fn set_texture_rects(&mut self, rects: &[Option<AtlasRect>]) {
        self.0.set_texture_rects(rects)
    }

    pub fn get_alpha_blending(&self) -> bool {
        self.0.get_alpha_blending()
    }

    pub fn set_alpha_blending(&mut self, alphablend: bool) {
        self.0.set_alpha_blending(alphablend)
    }

    pub fn get_colour_blending(&self) -> bool {
        self.0.get_colour_blending()
    }

    pub fn set_colour_blending(&mut self, modulate: bool) {
        self.0.set_colour_blending(modulate)
    }

    pub fn get_blend_mode(&self) -> (BlendType, BlendType) {
        self.0.get_blend_mode()
    }

    pub fn set_blend_mode(&mut self, src: BlendType, dst: BlendType) {
        self.0.set_blend_mode(src, dst)
    }

    pub fn get_pixel_interpolation(&self) -> bool {
        self.0.get_pixel_interpolation()
    }

    pub fn set_pixel_interpolation(&mut self, lerping: bool) {
        self.0.set_pixel_interpolation(lerping)
    }

    pub fn get_texture_repeat(&self) -> bool {
        self.0.get_texture_repeat()
    }

    pub fn set_texture_repeat(&mut self, repeat: bool) {
        self.0.set_texture_repeat(repeat)
    }

    pub fn flush_queue(&mut self) {
        self.0.flush_queue()
    }

    pub fn clear_view(&mut self, colour: Colour, alpha: f64) {
        self.0.clear_view(colour, alpha)
    }

    pub fn clear_view_no_zbuf(&mut self, colour: Colour, alpha: f64) {
        self.0.clear_view_no_zbuf(colour, alpha)
    }

    pub fn clear_zbuf(&mut self) {
        self.0.clear_zbuf()
    }

    pub fn get_3d(&self) -> bool {
        self.0.get_3d()
    }

    pub fn set_3d(&mut self, use_3d: bool) {
        self.0.set_3d(use_3d)
    }

    pub fn get_depth(&self) -> f32 {
        self.0.get_depth()
    }

    pub fn set_depth(&mut self, depth: f32) {
        self.0.set_depth(depth)
    }

    pub fn get_depth_test(&self) -> bool {
        self.0.get_depth_test()
    }

    pub fn set_depth_test(&mut self, depth_test: bool) {
        self.0.set_depth_test(depth_test)
    }

    pub fn get_write_depth(&self) -> bool {
        self.0.get_write_depth()
    }

    pub fn set_write_depth(&mut self, write_depth: bool) {
        self.0.set_write_depth(write_depth)
    }

    pub fn get_culling(&self) -> bool {
        self.0.get_culling()
    }

    pub fn set_culling(&mut self, culling: bool) {
        self.0.set_culling(culling)
    }

    pub fn get_perspective(&self) -> bool {
        self.0.get_perspective()
    }

    pub fn set_perspective(&mut self, perspective: bool) {
        self.0.set_perspective(perspective)
    }

    pub fn get_fog(&self) -> Option<Fog> {
        self.0.get_fog()
    }

    pub fn set_fog(&mut self, fog: Option<Fog>) {
        self.0.set_fog(fog)
    }

    pub fn get_gouraud(&self) -> bool {
        self.0.get_gouraud()
    }

    pub fn set_gouraud(&mut self, gouraud: bool) {
        self.0.set_gouraud(gouraud)
    }

    pub fn get_lighting_enabled(&self) -> bool {
        self.0.get_lighting_enabled()
    }

    pub fn set_lighting_enabled(&mut self, enabled: bool) {
        self.0.set_lighting_enabled(enabled)
    }

    pub fn get_ambient_colour(&self) -> i32 {
        self.0.get_ambient_colour()
    }

    pub fn set_ambient_colour(&mut self, colour: i32) {
        self.0.set_ambient_colour(colour)
    }

    pub fn get_lights(&self) -> [(bool, Light); 8] {
        self.0.get_lights()
    }

    pub fn set_lights(&mut self, lights: [(bool, Light); 8]) {
        self.0.set_lights(lights)
    }

    pub fn set_light_enabled(&mut self, id: usize, enabled: bool) {
        self.0.set_light_enabled(id, enabled)
    }

    pub fn set_light(&mut self, id: usize, light: Light) {
        self.0.set_light(id, light)
    }

    pub fn set_crt(&mut self, settings: Option<CrtSettings>) {
        self.0.set_crt(settings)
    }

    pub fn set_gb(&mut self, settings: Option<GbSettings>) {
        self.0.set_gb(settings)
    }

    pub fn set_gb_inverted(&mut self, inverted: bool) {
        self.0.set_gb_inverted(inverted)
    }

    pub fn present(&mut self, window_width: u32, window_height: u32, scaling: Scaling) {
        self.0.present(window_width, window_height, scaling)
    }

    pub fn present_surface(&mut self, surface: AtlasRef, window_width: u32, window_height: u32) {
        self.0.present_surface(surface, window_width, window_height)
    }

    pub fn draw_stored(&mut self, target_x: i32, target_y: i32, width: u32, height: u32) {
        self.0.draw_stored(target_x, target_y, width, height)
    }

    pub fn stored_size(&self) -> (u32, u32) {
        self.0.stored_size()
    }

    pub fn finish(&mut self, window_width: u32, window_height: u32, clear_colour: Colour) {
        self.0.finish(window_width, window_height, clear_colour)
    }

    pub fn state(&self) -> RendererState {
        RendererState {
            model_matrix: self.get_model_matrix(),
            alpha_blending: self.get_alpha_blending(),
            colour_blending: self.get_colour_blending(),
            blend_mode: self.get_blend_mode(),
            pixel_interpolation: self.get_pixel_interpolation(),
            texture_repeat: self.get_texture_repeat(),
            texture_rects: self.get_texture_rects(),
            vsync: self.get_vsync(),
            ambient_colour: self.get_ambient_colour(),
            using_3d: self.get_3d(),
            depth: self.get_depth(),
            depth_test: self.get_depth_test(),
            write_depth: self.get_write_depth(),
            culling: self.get_culling(),
            perspective: self.get_perspective(),
            fog: self.get_fog(),
            gouraud: self.get_gouraud(),
            lighting_enabled: self.get_lighting_enabled(),
            lights: self.get_lights(),
            circle_precision: self.get_circle_precision(),
            primitive_2d: self.get_primitive_2d(),
            primitive_3d: self.get_primitive_3d(),
            zbuf_trashed: self.get_zbuf_trashed(),
        }
    }

    /// The part of the renderer state that draw events can touch. Cheaper than [`state`], which
    /// also snapshots every atlas rect for savestates and would be wasteful on every extra frame.
    pub fn draw_state(&self) -> DrawState {
        DrawState {
            model_matrix: self.get_model_matrix(),
            alpha_blending: self.get_alpha_blending(),
            colour_blending: self.get_colour_blending(),
            blend_mode: self.get_blend_mode(),
            pixel_interpolation: self.get_pixel_interpolation(),
            texture_repeat: self.get_texture_repeat(),
            ambient_colour: self.get_ambient_colour(),
            using_3d: self.get_3d(),
            depth: self.get_depth(),
            depth_test: self.get_depth_test(),
            write_depth: self.get_write_depth(),
            culling: self.get_culling(),
            perspective: self.get_perspective(),
            fog: self.get_fog(),
            gouraud: self.get_gouraud(),
            lighting_enabled: self.get_lighting_enabled(),
            lights: self.get_lights(),
            circle_precision: self.get_circle_precision(),
            primitive_2d: self.get_primitive_2d(),
            primitive_3d: self.get_primitive_3d(),
            zbuf_trashed: self.get_zbuf_trashed(),
        }
    }

    pub fn restore_draw_state(&mut self, state: &DrawState) {
        self.set_model_matrix(state.model_matrix);
        self.set_alpha_blending(state.alpha_blending);
        self.set_colour_blending(state.colour_blending);
        self.set_blend_mode(state.blend_mode.0, state.blend_mode.1);
        self.set_pixel_interpolation(state.pixel_interpolation);
        self.set_texture_repeat(state.texture_repeat);
        self.set_ambient_colour(state.ambient_colour);
        self.set_3d(state.using_3d);
        self.set_depth(state.depth);
        self.set_depth_test(state.depth_test);
        self.set_write_depth(state.write_depth);
        self.set_culling(state.culling);
        self.set_perspective(state.perspective);
        self.set_fog(state.fog.clone());
        self.set_gouraud(state.gouraud);
        self.set_lighting_enabled(state.lighting_enabled);
        self.set_lights(state.lights);
        self.set_circle_precision(state.circle_precision);
        self.set_primitive_2d(state.primitive_2d.clone());
        self.set_primitive_3d(state.primitive_3d.clone());
        self.set_zbuf_trashed(state.zbuf_trashed);
    }

    pub fn set_state(&mut self, state: &RendererState) {
        self.set_model_matrix(state.model_matrix);
        self.set_alpha_blending(state.alpha_blending);
        self.set_colour_blending(state.colour_blending);
        self.set_blend_mode(state.blend_mode.0, state.blend_mode.1);
        self.set_pixel_interpolation(state.pixel_interpolation);
        self.set_texture_repeat(state.texture_repeat);
        self.set_texture_rects(&state.texture_rects);
        self.set_vsync(state.vsync);
        self.set_ambient_colour(state.ambient_colour);
        self.set_3d(state.using_3d);
        self.set_depth(state.depth);
        self.set_depth_test(state.depth_test);
        self.set_write_depth(state.write_depth);
        self.set_culling(state.culling);
        self.set_perspective(state.perspective);
        self.set_fog(state.fog.clone());
        self.set_gouraud(state.gouraud);
        self.set_lighting_enabled(state.lighting_enabled);
        self.set_lights(state.lights);
        self.set_circle_precision(state.circle_precision);
        self.set_primitive_2d(state.primitive_2d.clone());
        self.set_primitive_3d(state.primitive_3d.clone());
        self.set_zbuf_trashed(state.zbuf_trashed);
    }
}

/// Easy wrapper for all the parts of the renderer which need saving in savestates.
/// Everything in this struct can be queried individually. Don't use this for querying individual things.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendererState {
    pub model_matrix: [f32; 16],
    pub alpha_blending: bool,
    pub colour_blending: bool,
    pub blend_mode: (BlendType, BlendType),
    pub pixel_interpolation: bool,
    pub texture_repeat: bool,
    pub texture_rects: Vec<Option<AtlasRect>>,
    pub vsync: bool,
    pub ambient_colour: i32,
    pub using_3d: bool,
    pub depth: f32,
    pub depth_test: bool,
    pub write_depth: bool,
    pub culling: bool,
    pub perspective: bool,
    pub fog: Option<Fog>,
    pub gouraud: bool,
    pub lighting_enabled: bool,
    pub lights: [(bool, Light); 8],
    pub circle_precision: i32,
    pub primitive_2d: PrimitiveBuilder,
    pub primitive_3d: PrimitiveBuilder,
    pub zbuf_trashed: bool,
}

/// The renderer state an extra (interpolated) draw pass has to put back afterwards. It leaves out
/// texture rects and vsync, which drawing never changes and which are expensive to copy.
#[derive(Clone, Debug)]
pub struct DrawState {
    pub model_matrix: [f32; 16],
    pub alpha_blending: bool,
    pub colour_blending: bool,
    pub blend_mode: (BlendType, BlendType),
    pub pixel_interpolation: bool,
    pub texture_repeat: bool,
    pub ambient_colour: i32,
    pub using_3d: bool,
    pub depth: f32,
    pub depth_test: bool,
    pub write_depth: bool,
    pub culling: bool,
    pub perspective: bool,
    pub fog: Option<Fog>,
    pub gouraud: bool,
    pub lighting_enabled: bool,
    pub lights: [(bool, Light); 8],
    pub circle_precision: i32,
    pub primitive_2d: PrimitiveBuilder,
    pub primitive_3d: PrimitiveBuilder,
    pub zbuf_trashed: bool,
}

/// Multiply two mat4's together
fn mat4mult(m1: [f32; 16], m2: [f32; 16]) -> [f32; 16] {
    [
        (m1[0] * m2[0]) + (m1[1] * m2[4]) + (m1[2] * m2[8]) + (m1[3] * m2[12]),
        (m1[0] * m2[1]) + (m1[1] * m2[5]) + (m1[2] * m2[9]) + (m1[3] * m2[13]),
        (m1[0] * m2[2]) + (m1[1] * m2[6]) + (m1[2] * m2[10]) + (m1[3] * m2[14]),
        (m1[0] * m2[3]) + (m1[1] * m2[7]) + (m1[2] * m2[11]) + (m1[3] * m2[15]),
        (m1[4] * m2[0]) + (m1[5] * m2[4]) + (m1[6] * m2[8]) + (m1[7] * m2[12]),
        (m1[4] * m2[1]) + (m1[5] * m2[5]) + (m1[6] * m2[9]) + (m1[7] * m2[13]),
        (m1[4] * m2[2]) + (m1[5] * m2[6]) + (m1[6] * m2[10]) + (m1[7] * m2[14]),
        (m1[4] * m2[3]) + (m1[5] * m2[7]) + (m1[6] * m2[11]) + (m1[7] * m2[15]),
        (m1[8] * m2[0]) + (m1[9] * m2[4]) + (m1[10] * m2[8]) + (m1[11] * m2[12]),
        (m1[8] * m2[1]) + (m1[9] * m2[5]) + (m1[10] * m2[9]) + (m1[11] * m2[13]),
        (m1[8] * m2[2]) + (m1[9] * m2[6]) + (m1[10] * m2[10]) + (m1[11] * m2[14]),
        (m1[8] * m2[3]) + (m1[9] * m2[7]) + (m1[10] * m2[11]) + (m1[11] * m2[15]),
        (m1[12] * m2[0]) + (m1[13] * m2[4]) + (m1[14] * m2[8]) + (m1[15] * m2[12]),
        (m1[12] * m2[1]) + (m1[13] * m2[5]) + (m1[14] * m2[9]) + (m1[15] * m2[13]),
        (m1[12] * m2[2]) + (m1[13] * m2[6]) + (m1[14] * m2[10]) + (m1[15] * m2[14]),
        (m1[12] * m2[3]) + (m1[13] * m2[7]) + (m1[14] * m2[11]) + (m1[15] * m2[15]),
    ]
}
