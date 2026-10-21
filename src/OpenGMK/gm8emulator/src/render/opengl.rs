use crate::{
    render::{
        atlas::{AtlasBuilder, AtlasRect, AtlasRef},
        mat4mult, BlendType, Fog, Light, PrimitiveBuilder, PrimitiveShape, PrimitiveType, RendererOptions,
        RendererTrait, SavedTexture, Scaling, Vertex, VertexBuffer,
    },
    types::Colour,
};
use ramen::{connection::Connection, window::Window};
use rect_packer::DensePacker;
use std::{any::Any, f64::consts::PI, ffi::{CStr, CString}, mem::{offset_of, size_of}, ptr};

/// Auto-generated OpenGL bindings from gl_generator
pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}
use gl::types::{GLchar, GLenum, GLint, GLsizei, GLuint};

#[cfg(target_os = "windows")]
mod wgl;
#[cfg(target_os = "windows")]
mod wgl_ffi;
#[cfg(target_os = "windows")]
use wgl as native_gl;
#[cfg(target_os = "linux")]
pub mod glx;
#[cfg(target_os = "linux")]
use glx as native_gl;
#[cfg(target_os = "android")]
pub mod egl;
#[cfg(target_os = "android")]
use egl as native_gl;

/// GLSL version header, plus any prelude GLES needs that desktop GLSL doesn't.
/// GLSL ES 3.00 has no default float precision in the fragment shader, so it must be declared.
#[cfg(target_os = "android")]
const SHADER_VERSION: &str = "#version 300 es\nprecision highp float;\n";
#[cfg(not(target_os = "android"))]
const SHADER_VERSION: &str = "#version 330 core\n";

macro_rules! shader_file {
    ($path: expr) => {
        include_str!($path)
    };
}

/// Builds a NUL-terminated GLSL source: the version header (plus any GLES prelude) and the file body.
fn shader_source(body: &str) -> Vec<u8> {
    let out = CString::new(format!("{}{}", SHADER_VERSION, body))
        .expect("el shader no debe contener bytes nulos");
    out.as_bytes_with_nul().to_vec()
}

// Desktop GL requires 3.3; GLES on Android only ever exposes a 3.x series starting at 3.0.
#[cfg(not(target_os = "android"))]
pub const OPENGL_VERSION: (i32, i32) = (3, 3);
#[cfg(target_os = "android")]
pub const OPENGL_VERSION: (i32, i32) = (3, 0);

#[derive(Clone, Copy, PartialEq)]
#[repr(u32)]
enum GLBool {
    False = 0,
    True = 1,
}

impl From<GLBool> for bool {
    fn from(b: GLBool) -> Self {
        match b {
            GLBool::False => false,
            GLBool::True => true,
        }
    }
}

impl From<bool> for GLBool {
    fn from(b: bool) -> Self {
        if b { GLBool::True } else { GLBool::False }
    }
}

#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
struct GLLight {
    pos: [f32; 4],
    colour: [f32; 4],
    enabled: GLBool,
    is_point: GLBool,
    range: f32,
    padding: u32, // must pad to size of vec4
}

#[derive(Clone, PartialEq)]
#[repr(C)]
struct RenderState {
    // vertex shader
    model_matrix: [f32; 16],
    viewproj_matrix: [f32; 16],
    lights: [GLLight; 8],
    ambient_colour: [f32; 4],
    lighting: GLBool,
    gouraud: GLBool,
    texture_blend: GLBool, // emulates glTexEnv(): GL_MODULATE if true, GL_REPLACE if false
    // frag shader
    texture_repeat: GLBool,
    interpolate_pixels: GLBool,
    alpha_test: GLBool,
    fog_enabled: GLBool,
    fog_begin: f32,
    fog_end: f32,
    // std140 alignment: `fog_colour` is a vec4, so it has to start on a 16-byte boundary (offset 576).
    // Without this padding the shader read every field after it from the wrong place.
    _fog_pad: [u32; 3],
    fog_colour: [f32; 4],
    // end of shader stuff
    end_of_uniform: (),
    view_matrix: [f32; 16],
    proj_matrix: [f32; 16],
    alpha_blending: bool,
    blend_mode: (BlendType, BlendType),
    // Depth test is a GL state, not something the shader reads: it lives after `end_of_uniform` so
    // it never lands in front of a shader field.
    depth_test: GLBool,
    write_depth: bool,
    culling: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        // Create identity matrix to initialize MVP matrices with
        #[rustfmt::skip]
            let identity_matrix: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        Self {
            alpha_blending: true,
            blend_mode: (BlendType::SrcAlpha, BlendType::InvSrcAlpha),
            interpolate_pixels: GLBool::False,
            texture_repeat: GLBool::False,
            texture_blend: GLBool::True,
            model_matrix: identity_matrix.clone(),
            view_matrix: identity_matrix.clone(),
            proj_matrix: identity_matrix.clone(),
            depth_test: false.into(),
            write_depth: false,
            culling: false,
            alpha_test: GLBool::True,
            fog_enabled: GLBool::False,
            fog_begin: 0.0,
            fog_end: 0.0,
            _fog_pad: [0; 3],
            lighting: GLBool::False,
            gouraud: GLBool::True,
            ambient_colour: [0.0; 4],
            lights: [GLLight {
                pos: [0.0; 4],
                colour: [0.0; 4],
                enabled: GLBool::False,
                is_point: GLBool::False,
                range: 0.0,
                padding: 0,
            }; 8],
            viewproj_matrix: identity_matrix.clone(),
            fog_colour: [0.0; 4],
            end_of_uniform: (),
        }
    }
}

impl RenderState {
    fn update_matrix(&mut self, gl: &gl::Gl) {
        let mut viewport = [0; 4];
        unsafe {
            gl.GetIntegerv(gl::VIEWPORT, viewport.as_mut_ptr());
        }
        let offset_x = 1.0 / f64::from(viewport[2]);
        let offset_y = 1.0 / f64::from(viewport[3]);
        // build viewproj matrix
        #[rustfmt::skip]
            let viewproj = mat4mult(
            mat4mult(self.view_matrix, self.proj_matrix),
            // flip vertically because GL textures are flipped vertically vs DX
            // also GL's screen space is offset half a pixel vs DX so shift it
            [
                1.0,             0.0,             0.0, 0.0,
                0.0,             -1.0,            0.0, 0.0,
                0.0,             0.0,             1.0, 0.0,
                offset_x as f32, offset_y as f32, 0.0, 1.0,
            ],
        );
        self.viewproj_matrix = viewproj;
    }
}

pub struct RendererImpl {
    imp: native_gl::PlatformImpl,
    gl: gl::Gl,
    /// The sprite shader and the buffers it draws from. They are bound again before every draw,
    /// because another pass (the touch overlay, for one) may have changed them in between, and on
    /// some drivers — PowerVR among them — drawing with the wrong vertex array produces nothing.
    program: GLuint,
    vao: GLuint,
    commands_vbo: GLuint,
    atlas_packers: Vec<DensePacker>,
    texture_ids: Vec<Option<GLuint>>,
    zbuf_ids: Vec<Option<GLuint>>,
    fbo_ids: Vec<Option<GLuint>>,
    texture_rects: Vec<Option<AtlasRect>>,
    stock_texture_count: usize,
    stock_atlas_count: u32,
    current_atlas: u32,
    /// What is bound right now, so the texture state is only touched when it actually changes.
    current_texture: Option<GLuint>,
    last_filter_mode: Option<GLenum>,
    framebuffer: Framebuffer,
    stored_framebuffer: Option<Framebuffer>,
    zbuf_format: GLint,
    zbuf_type: GLenum,
    zbuf_trashed: bool,
    white_pixel: AtlasRect,
    vertex_queue: Vec<Vertex>,
    queue_type: PrimitiveShape,
    queue_render_state: RenderState,
    next_render_state: RenderState,
    render_state_updated: bool,
    circle_precision: i32,
    using_3d: bool,
    perspective: bool,
    depth: f32,
    primitive_2d: PrimitiveBuilder,
    primitive_3d: PrimitiveBuilder,

    loc_gm81_normalize: GLint, // uniform bool gm81_normalize
    loc_tex: GLint,            // uniform sampler2D tex
    buf_state: GLuint,         // uniform RenderState state

    /// Lazily-built program/buffers for the on-screen touch controls (Android only).
    #[cfg(target_os = "android")]
    overlay: Option<Overlay>,
}

/// One image in the packed control artwork: which control it belongs to, and where it sits in the
/// texture (u1, v1, u2, v2).
#[cfg(target_os = "android")]
#[derive(Clone, Copy)]
struct SkinSlot {
    name: &'static str,
    uv: [f32; 4],
    /// Width over height of the artwork, so a picture that is not square is not stretched into one
    /// when it is drawn. Most of the artwork is square and this makes no difference for it.
    aspect: f32,
    custom: bool,
    context: u8,
    has_art: bool,
}

/// How many control images the overlay can carry. Everything a control needs is one slot.
#[cfg(target_os = "android")]
const MAX_SKIN_SLOTS: usize = 96;

/// Side of each party sprite's square cell in the strip the overlay keeps for them.
#[cfg(target_os = "android")]
const PARTY_CELL: u32 = 64;

/// A tiny self-contained GL program to draw the touch controls over the frame, without touching
/// the emulator's own render state.
#[cfg(target_os = "android")]
#[derive(Clone, Copy)]
struct Overlay {
    program: GLuint,
    vao: GLuint,
    vbo: GLuint,
    /// A 1x1 white texture, so the plain shapes sample something neutral.
    white: GLuint,
    /// The packed pixel-art control artwork, or 0 when none was found.
    skin: GLuint,
    slots: [Option<SkinSlot>; MAX_SKIN_SLOTS],
    loc_tex: GLint,
    /// The party's own sprite strip, one cell per slot, refilled when the game's party changes.
    party: GLuint,
    party_version: u64,
}

#[cfg(target_os = "android")]
impl Overlay {
    fn slot(&self,name:&str)->Option<&SkinSlot>{
        let context=crate::touch_controls::current_context();
        self.slots.iter().flatten().find(|s|s.name==name&&s.context==context).or_else(||self.slots.iter().flatten().find(|s|s.name==name&&s.context==0))
    }
    /// Where a control's artwork sits in the packed texture, if it has any.
    fn art(&self, name: &str) -> Option<[f32; 4]> {
        self.slot(name).filter(|slot|slot.has_art).map(|slot|slot.uv)
    }

    fn custom_art(&self, name: &str) -> bool {
        self.slot(name).is_some_and(|slot|slot.custom)
    }

    /// The shape of a control's artwork, so it can be drawn as it was painted.
    fn art_aspect(&self, name: &str) -> Option<f32> {
        self.slot(name).map(|slot|slot.aspect)
    }
}

#[derive(Clone, Copy)]
struct Framebuffer {
    pub texture: GLuint,
    pub zbuf: GLuint,
    pub fbo: GLuint,
    /// Kept here instead of asking the driver with `glGetTexLevelParameteriv`, which is not
    /// dependable everywhere: on PowerVR it answered nothing, the viewport came out empty and the
    /// whole frame was clipped away.
    pub width: u32,
    pub height: u32,
}

static VERTEX_SHADER_SOURCE: &str = shader_file!("glsl/vertex.glsl");
static FRAGMENT_SHADER_SOURCE: &str = shader_file!("glsl/fragment.glsl");

unsafe fn shader_info_log(gl: &gl::Gl, name: &str, id: GLuint) -> String {
    let mut info_len: GLint = 0;
    gl.GetShaderiv(id, gl::INFO_LOG_LENGTH, &mut info_len);
    let mut info = vec![0u8; info_len as usize];
    gl.GetShaderInfoLog(id, info_len as GLsizei, ptr::null_mut(), info.as_mut_ptr() as *mut GLchar);
    info.set_len((info_len - 1) as usize); // ignore null for str::from_utf8
    format!(
        "Failed to compile {} shader, compiler output:\n{}",
        name,
        std::str::from_utf8(&info).unwrap_or("<INVALID UTF-8>")
    )
}

fn make_view_matrix(x: f64, y: f64, z: f64, w: f64, h: f64, angle: f64) -> [f32; 16] {
    // Note: sin is negated because it's the same as negating the angle, which is how GM8 does view angles
    let angle = angle.to_radians();
    let sin_angle = -angle.sin() as f32;
    let cos_angle = angle.cos() as f32;

    #[rustfmt::skip]
    let view_matrix: [f32; 16] = {
        // source rectangle's center coordinates aka -(x + w/2) and -(y + h/2)
        let scx = -((x as f32) + (w as f32 / 2.0));
        let scy = -((y as f32) + (h as f32 / 2.0));
        let scz = -z as f32;
        mat4mult(
            // Place camera at (scx, scy, scz)
            [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                scx, scy, scz, 1.0,
            ],
            // Rotate to view_angle
            [
                cos_angle,  sin_angle, 0.0, 0.0,
                -sin_angle, cos_angle, 0.0, 0.0,
                0.0,        0.0,       1.0, 0.0,
                0.0,        0.0,       0.0, 1.0,
            ]
        )
    };

    view_matrix
}

fn split_colour(rgb: i32, alpha: f64) -> [f32; 4] {
    [
        ((rgb & 0xFF) as f32) / 255.0,
        (((rgb >> 8) & 0xFF) as f32) / 255.0,
        (((rgb >> 16) & 0xFF) as f32) / 255.0,
        alpha.max(0.0).min(1.0) as f32,
    ]
}

// TODO: probably put this in render.rs instead
impl VertexBuffer {
    pub fn swap_colour(&mut self, old: (i32, f64), new: (i32, f64)) {
        let old = split_colour(old.0, old.1);
        let new = split_colour(new.0, new.1);
        for vert in self.points.iter_mut().chain(&mut self.lines).chain(&mut self.tris) {
            if vert.blend == old {
                vert.blend = new;
            }
        }
    }
}

impl From<AtlasRect> for [f32; 4] {
    fn from(ar: AtlasRect) -> Self {
        [ar.x as f32, ar.y as f32, ar.w as f32, ar.h as f32]
    }
}

#[derive(Debug)]
struct LightUniform {
    enabled: GLint,
    is_point: GLint,
    pos: GLint,
    colour: GLint,
    range: GLint,
}

impl From<BlendType> for GLenum {
    fn from(bt: BlendType) -> Self {
        match bt {
            BlendType::Zero => gl::ZERO,
            BlendType::One => gl::ONE,
            BlendType::SrcColour => gl::SRC_COLOR,
            BlendType::InvSrcColour => gl::ONE_MINUS_SRC_COLOR,
            BlendType::SrcAlpha => gl::SRC_ALPHA,
            BlendType::InvSrcAlpha => gl::ONE_MINUS_SRC_ALPHA,
            BlendType::DestAlpha => gl::DST_ALPHA,
            BlendType::InvDestAlpha => gl::ONE_MINUS_DST_ALPHA,
            BlendType::DestColour => gl::DST_COLOR,
            BlendType::InvDestColour => gl::ONE_MINUS_DST_COLOR,
            BlendType::SrcAlphaSaturate => gl::SRC_ALPHA_SATURATE,
        }
    }
}

impl From<GLenum> for BlendType {
    fn from(bt: GLenum) -> Self {
        match bt {
            gl::ZERO => BlendType::Zero,
            gl::ONE => BlendType::One,
            gl::SRC_COLOR => BlendType::SrcColour,
            gl::ONE_MINUS_SRC_COLOR => BlendType::InvSrcColour,
            gl::SRC_ALPHA => BlendType::SrcAlpha,
            gl::ONE_MINUS_SRC_ALPHA => BlendType::InvSrcAlpha,
            gl::DST_ALPHA => BlendType::DestAlpha,
            gl::ONE_MINUS_DST_ALPHA => BlendType::InvDestAlpha,
            gl::DST_COLOR => BlendType::DestColour,
            gl::ONE_MINUS_DST_COLOR => BlendType::InvDestColour,
            gl::SRC_ALPHA_SATURATE => BlendType::SrcAlphaSaturate,
            _ => unreachable!(),
        }
    }
}

impl From<PrimitiveShape> for GLenum {
    fn from(shape: PrimitiveShape) -> Self {
        match shape {
            PrimitiveShape::Point => gl::POINTS,
            PrimitiveShape::Line => gl::LINES,
            PrimitiveShape::Triangle => gl::TRIANGLES,
        }
    }
}

/// A builder to be used for building basic shapes.
struct ShapeBuilder {
    primitive: PrimitiveBuilder,
    outline: bool,
    depth: f32,
    alpha: f64,
}

impl ShapeBuilder {
    fn new(outline: bool, atlas_ref: AtlasRect, alpha: f64, depth: f32) -> Self {
        Self {
            primitive: PrimitiveBuilder::new(
                atlas_ref,
                if outline { PrimitiveType::LineStrip } else { PrimitiveType::TriFan },
                false,
            ),
            outline,
            depth,
            alpha,
        }
    }

    /// Shortcut for basic shapes.
    fn push_point(&mut self, x: f64, y: f64, colour: i32) -> &mut Self {
        self.primitive.push_vertex([x as f32, y as f32, self.depth], [0.0, 0.0], split_colour(colour, self.alpha), [
            0.0, 0.0, 0.0,
        ]);
        self
    }

    /// Should only be called once. This is only used for basic shapes, so it's fine for it to be *possible* to
    /// call it multiple times, as that makes things easier elsewhere.
    fn build(&mut self) -> &PrimitiveBuilder {
        if self.outline {
            let vertices = self.primitive.get_vertices();
            if vertices.len() > 2 {
                let vertex = vertices[0];
                self.primitive.push_vertex_raw(vertex);
            }
        }
        &self.primitive
    }
}

// TODO: Implement Drop trait for RendererImpl to delete OpenGL objects we create? This doesn't make
// much sense in Release builds - because then we're doing the OS's work for it and just increasing
// the process termination time - but can be quite useful for Debug ones.

impl RendererImpl {
    pub fn new(options: &RendererOptions, connection: &Connection, window: &Window, clear_colour: Colour) -> Result<Self, String> {
        unsafe {
            let imp = native_gl::PlatformImpl::new(connection, window)?;

            // gl function pointers
            let gl = gl::Gl::load_with(native_gl::PlatformImpl::get_function_loader()?);
            native_gl::PlatformImpl::clean_function_loader();

            // debug print
            let ver_str = CStr::from_ptr(gl.GetString(gl::VERSION).cast()).to_str().unwrap();
            let vendor_str = CStr::from_ptr(gl.GetString(gl::VENDOR).cast()).to_str().unwrap();
            eprintln!("creating graphics context\n  > GL_VERSION: \"{}\"\n  > GL_VENDOR: \"{}\"", ver_str, vendor_str);

            let mut v_maj: GLint = 0;
            let mut v_min: GLint = 0;
            gl.GetIntegerv(gl::MAJOR_VERSION, &mut v_maj);
            gl.GetIntegerv(gl::MINOR_VERSION, &mut v_min);
            assert!(
                (v_maj == OPENGL_VERSION.0 && v_min >= OPENGL_VERSION.1) || v_maj > OPENGL_VERSION.0,
                "OpenGL version {}.{} or later is required, but found version {}.{}",
                OPENGL_VERSION.0,
                OPENGL_VERSION.1,
                v_maj,
                v_min
            );

            imp.set_swap_interval(if options.vsync { 1 } else { 0 });
            // Desktop GL accepts DEPTH_COMPONENT* with a FLOAT type; GLES requires an integer
            // type matching the depth size, so the pair has to be chosen per platform.
            let (zbuf_format, zbuf_type) = if cfg!(target_os = "android") {
                if options.zbuf_24 {
                    (gl::DEPTH_COMPONENT24 as GLint, gl::UNSIGNED_INT)
                } else {
                    (gl::DEPTH_COMPONENT16 as GLint, gl::UNSIGNED_SHORT)
                }
            } else if options.zbuf_24 {
                (gl::DEPTH_COMPONENT24 as GLint, gl::FLOAT)
            } else {
                (gl::DEPTH_COMPONENT16 as GLint, gl::FLOAT)
            };

            // Compile vertex shader
            let vertex_shader = gl.CreateShader(gl::VERTEX_SHADER);
            let vertex_source = shader_source(VERTEX_SHADER_SOURCE);
            gl.ShaderSource(vertex_shader, 1, &(vertex_source.as_ptr().cast()), ptr::null());
            gl.CompileShader(vertex_shader);

            // Check for vertex shader compile errors
            let mut success = gl::FALSE as GLint;
            gl.GetShaderiv(vertex_shader, gl::COMPILE_STATUS, &mut success);
            if success != gl::TRUE as GLint {
                return Err(shader_info_log(&gl, "vertex", vertex_shader))
            }

            // Compile fragment shader
            let fragment_shader = gl.CreateShader(gl::FRAGMENT_SHADER);
            let fragment_source = shader_source(FRAGMENT_SHADER_SOURCE);
            gl.ShaderSource(fragment_shader, 1, &(fragment_source.as_ptr().cast()), ptr::null());
            gl.CompileShader(fragment_shader);

            // Check for fragment shader compile errors
            gl.GetShaderiv(fragment_shader, gl::COMPILE_STATUS, &mut success);
            if success != gl::TRUE as GLint {
                return Err(shader_info_log(&gl, "fragment", fragment_shader))
            }

            // Link shaders
            let program = gl.CreateProgram();
            gl.AttachShader(program, vertex_shader);
            gl.AttachShader(program, fragment_shader);
            gl.LinkProgram(program);

            // Check for linking errors
            // TODO: generalize this like with shader info logs!! please!!!
            gl.GetProgramiv(program, gl::LINK_STATUS, &mut success);
            if success != gl::TRUE as GLint {
                let mut info_len: GLint = 0;
                gl.GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut info_len);
                let mut info = vec![0u8; info_len as usize];
                gl.GetProgramInfoLog(program, info_len as GLsizei, ptr::null_mut(), info.as_mut_ptr() as *mut GLchar);
                info.set_len((info_len - 1) as usize); // ignore null for str::from_utf8
                return Err(format!(
                    "Failed to link shaders, compiler output:\n{}",
                    std::str::from_utf8(&info).unwrap_or("<INVALID UTF-8>")
                ))
            }
            gl.DeleteShader(vertex_shader);
            gl.DeleteShader(fragment_shader);

            // set up vertex array
            let mut vao = 0;
            gl.GenVertexArrays(1, &mut vao);
            gl.BindVertexArray(vao);

            // Enable and disable GL features
            gl.Enable(gl::SCISSOR_TEST);
            // gl::Enable(gl::TEXTURE_2D);
            gl.Disable(gl::CULL_FACE);
            gl.Enable(gl::BLEND);
            gl.Disable(gl::DEPTH_TEST);

            gl.BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl.DepthFunc(gl::LEQUAL);

            // Use DX provoking vertex convention
            gl.ProvokingVertex(gl::FIRST_VERTEX_CONVENTION);

            // Create and bind the VBO once
            let mut commands_vbo: GLuint = 0;
            gl.GenBuffers(1, &mut commands_vbo);
            gl.BindBuffer(gl::ARRAY_BUFFER, commands_vbo);

            // layout (location = 0) in vec3 pos;
            // layout (location = 1) in vec4 blend;
            // layout (location = 2) in vec2 tex_coord;
            // layout (location = 3) in vec3 normal;
            // layout (location = 4) in vec4 atlas_xywh;
            gl.EnableVertexAttribArray(0);
            gl.VertexAttribPointer(
                0,
                3,
                gl::FLOAT,
                gl::FALSE,
                size_of::<Vertex>() as i32,
                offset_of!(Vertex, pos) as *const _,
            );
            gl.EnableVertexAttribArray(1);
            gl.VertexAttribPointer(
                1,
                4,
                gl::FLOAT,
                gl::FALSE,
                size_of::<Vertex>() as i32,
                offset_of!(Vertex, blend) as *const _,
            );
            gl.EnableVertexAttribArray(2);
            gl.VertexAttribPointer(
                2,
                2,
                gl::FLOAT,
                gl::FALSE,
                size_of::<Vertex>() as i32,
                offset_of!(Vertex, tex_coord) as *const _,
            );
            gl.EnableVertexAttribArray(3);
            gl.VertexAttribPointer(
                3,
                3,
                gl::FLOAT,
                gl::FALSE,
                size_of::<Vertex>() as i32,
                offset_of!(Vertex, normal) as *const _,
            );
            gl.EnableVertexAttribArray(4);
            gl.VertexAttribPointer(
                4,
                4,
                gl::FLOAT,
                gl::FALSE,
                size_of::<Vertex>() as i32,
                offset_of!(Vertex, atlas_xywh) as *const _,
            );

            // Use program
            gl.UseProgram(program);

            // Configure gl::ReadPixels() to read from the back buffer
            gl.ReadBuffer(gl::BACK);

            // Configure gl::ReadPixels() to align to 1 byte
            gl.PixelStorei(gl::PACK_ALIGNMENT, 1);

            // Create framebuffer
            let (mut framebuffer_texture, mut framebuffer_zbuf, mut framebuffer_fbo) = (0, 0, 0);
            gl.GenTextures(1, &mut framebuffer_texture);
            gl.BindTexture(gl::TEXTURE_2D, framebuffer_texture);
            gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);
            gl.TexImage2D(
                gl::TEXTURE_2D,      // target
                0,                   // level
                gl::RGBA as _,       // internalformat
                options.size.0 as _, // width
                options.size.1 as _, // height
                0,                   // border ("must be 0")
                gl::RGBA,            // format
                gl::UNSIGNED_BYTE,   // type
                ptr::null(),         // data
            );
            gl.GenTextures(1, &mut framebuffer_zbuf);
            gl.BindTexture(gl::TEXTURE_2D, framebuffer_zbuf);
            gl.TexImage2D(
                gl::TEXTURE_2D,      // target
                0,                   // level
                zbuf_format,         // internalformat
                options.size.0 as _, // width
                options.size.1 as _, // height
                0,                   // border ("must be 0")
                gl::DEPTH_COMPONENT, // format
                zbuf_type,           // type
                ptr::null(),         // data
            );
            gl.GenFramebuffers(1, &mut framebuffer_fbo);
            gl.BindFramebuffer(gl::FRAMEBUFFER, framebuffer_fbo);
            gl.FramebufferTexture2D(
                gl::READ_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                framebuffer_texture,
                0,
            );
            gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::DEPTH_ATTACHMENT, gl::TEXTURE_2D, framebuffer_zbuf, 0);

            // setup uniform buffer for render state
            let loc_state = 1;
            let ind = gl.GetUniformBlockIndex(program, b"RenderState\0".as_ptr().cast());
            assert_ne!(ind, gl::INVALID_INDEX);
            gl.UniformBlockBinding(program, ind, loc_state);
            let mut buf_state: GLuint = 0;
            gl.GenBuffers(1, &mut buf_state);
            gl.BindBufferBase(gl::UNIFORM_BUFFER, loc_state, buf_state);
            assert_eq!(gl.GetError(), 0);

            // Create Renderer
            let mut renderer = Self {
                imp,
                program,
                vao,
                commands_vbo,
                atlas_packers: vec![],
                texture_ids: vec![],
                zbuf_ids: vec![],
                fbo_ids: vec![],
                texture_rects: vec![],
                stock_texture_count: 0,
                stock_atlas_count: 0,
                current_atlas: 0,
                current_texture: None,
                last_filter_mode: None,
                framebuffer: Framebuffer {
                    texture: framebuffer_texture,
                    zbuf: framebuffer_zbuf,
                    fbo: framebuffer_fbo,
                    width: options.size.0,
                    height: options.size.1,
                },
                stored_framebuffer: None,
                zbuf_format,
            zbuf_type,
                zbuf_trashed: false,
                white_pixel: Default::default(),
                vertex_queue: Vec::with_capacity(1536),
                queue_type: PrimitiveShape::Triangle,
                queue_render_state: RenderState {
                    interpolate_pixels: options.interpolate_pixels.into(),
                    ..Default::default()
                },
                next_render_state: RenderState {
                    interpolate_pixels: options.interpolate_pixels.into(),
                    ..Default::default()
                },
                render_state_updated: false,
                circle_precision: 24,
                using_3d: false,
                perspective: false,
                depth: 0.0,
                primitive_2d: PrimitiveBuilder::new(Default::default(), PrimitiveType::PointList, false),
                primitive_3d: PrimitiveBuilder::new(Default::default(), PrimitiveType::PointList, false),

                loc_gm81_normalize: gl.GetUniformLocation(program, b"gm81_normalize\0".as_ptr().cast()),
                loc_tex: gl.GetUniformLocation(program, b"tex\0".as_ptr().cast()),
                buf_state,

                #[cfg(target_os = "android")]
                overlay: None,

                gl,
            };
            assert_eq!(renderer.gl.GetError(), 0);

            // default uniform values
            renderer.gl.Uniform1i(renderer.loc_gm81_normalize, options.normalize_normals as _);
            renderer.gl.Uniform1i(renderer.loc_tex, 0 as _);
            renderer.gl.BindBuffer(gl::UNIFORM_BUFFER, renderer.buf_state);
            renderer.gl.BufferData(
                gl::UNIFORM_BUFFER,
                offset_of!(RenderState, end_of_uniform) as _,
                (&renderer.queue_render_state as *const RenderState).cast(),
                gl::STATIC_DRAW,
            );
            assert_eq!(renderer.gl.GetError(), 0);

            // Start first frame
            renderer.setup_frame(clear_colour);

            // verify it actually worked
            match renderer.gl.GetError() {
                0 => Ok(renderer),
                err => Err(format!("Failed to fully initialize OpenGL! (OpenGL code {})", err)),
            }
        }
    }

    fn setup_frame(&mut self, clear_colour: Colour) {
        unsafe {
            let width = self.framebuffer.width as i32;
            let height = self.framebuffer.height as i32;
            // set view
            self.set_view(0, 0, width, height, 0.0, 0, 0, width, height);
            // clear screen
            self.clear_view(clear_colour, 1.0);
        }
    }

    fn get_rect_mut(&mut self, id: AtlasRef) -> Option<&mut AtlasRect> {
        id.0.try_into()
            .ok()
            .and_then(move |id: usize| self.texture_rects.get_mut(id))
            .and_then(|o: &mut Option<AtlasRect>| o.as_mut())
    }

    /// Updates the renderer state.
    /// Call this before doing any drawing.
    fn update_render_state(&mut self) {
        if self.render_state_updated && self.queue_render_state != self.next_render_state {
            self.flush_queue();
            // update the viewproj matrix before doing any cloning
            if self.queue_render_state.view_matrix != self.next_render_state.view_matrix
                || self.queue_render_state.proj_matrix != self.next_render_state.proj_matrix
            {
                self.next_render_state.update_matrix(&self.gl);
            }

            let mut old_render_state = self.next_render_state.clone();
            std::mem::swap(&mut old_render_state, &mut self.queue_render_state);
            self.render_state_updated = false;
            let alpha_blending = self.next_render_state.alpha_blending;
            let blend_mode = self.next_render_state.blend_mode;
            let depth_test = self.next_render_state.depth_test;
            let write_depth = self.next_render_state.write_depth;
            let culling = self.next_render_state.culling;
            unsafe {
                if old_render_state.alpha_blending != alpha_blending {
                    if alpha_blending.into() {
                        self.gl.Enable(gl::BLEND);
                    } else {
                        self.gl.Disable(gl::BLEND);
                    }
                }
                if old_render_state.blend_mode != blend_mode {
                    let (src, dst) = blend_mode;
                    self.gl.BlendFunc(src.into(), dst.into());
                }
                if old_render_state.depth_test != depth_test {
                    if depth_test.into() {
                        self.gl.Enable(gl::DEPTH_TEST);
                    } else {
                        self.gl.Disable(gl::DEPTH_TEST);
                    }
                }
                if old_render_state.culling != culling {
                    if culling {
                        self.gl.Enable(gl::CULL_FACE);
                    } else {
                        self.gl.Disable(gl::CULL_FACE);
                    }
                }
                if old_render_state.write_depth != write_depth {
                    self.gl.DepthMask(write_depth as _);
                }

                self.gl.BindBuffer(gl::UNIFORM_BUFFER, self.buf_state);
                self.gl.BufferData(
                    gl::UNIFORM_BUFFER,
                    offset_of!(RenderState, end_of_uniform) as _,
                    (&self.queue_render_state as *const RenderState).cast(),
                    gl::STATIC_DRAW,
                );
                debug_assert_eq!(self.gl.GetError(), 0);
            }
        }
    }

    fn setup_queue(&mut self, atlas_id: u32, queue_type: PrimitiveShape) {
        self.update_render_state();
        if atlas_id != self.current_atlas || self.queue_type != queue_type {
            self.flush_queue();
            self.current_atlas = atlas_id;
            self.queue_type = queue_type;
        }
    }

    fn push_primitive(&mut self, builder: &PrimitiveBuilder) {
        self.setup_queue(builder.get_atlas_id(), builder.get_shape());
        self.vertex_queue.extend_from_slice(builder.get_vertices());
    }

    fn draw_buffer(&mut self, atlas_id: u32, shape: PrimitiveShape, buffer: &[Vertex]) {
        if buffer.is_empty() {
            return
        }

        unsafe {
            // if something else broke check here just in case (only in debug builds to prevent mobile GPU stalls)
            #[cfg(debug_assertions)]
            match self.gl.GetError() {
                0 => (),
                err => panic!("OpenGL threw an error somewhere (error code {})", err),
            }

            self.gl.UseProgram(self.program);
            self.gl.BindVertexArray(self.vao);
            self.gl.BindBuffer(gl::ARRAY_BUFFER, self.commands_vbo);

            let tex_id = self.texture_ids[atlas_id as usize].unwrap();
            let filter_mode = if self.queue_render_state.interpolate_pixels.into() { gl::LINEAR } else { gl::NEAREST };
            if self.current_texture != Some(tex_id) {
                self.gl.BindTexture(gl::TEXTURE_2D, tex_id);
                self.current_texture = Some(tex_id);
                self.last_filter_mode = None;
            }
            if self.last_filter_mode != Some(filter_mode) {
                self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, filter_mode as _);
                self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, filter_mode as _);
                self.last_filter_mode = Some(filter_mode);
            }

            self.gl.BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<Vertex>() * buffer.len()) as _,
                buffer.as_ptr().cast(),
                gl::STREAM_DRAW,
            );
            debug_assert_eq!(self.gl.GetError(), 0);

            self.gl.DrawArrays(shape.into(), 0, buffer.len() as i32);
        }
    }
}

/// Packs the pixel-art artwork the launcher leaves next to the game into one texture: the joystick's
/// two images, plus one for every control that has a `<name>.png` beside it. Returns the texture (0
/// when there is nothing) and where each image sits.
#[cfg(target_os = "android")]
fn load_skin(gl: &gl::Gl) -> (GLuint, [Option<SkinSlot>; MAX_SKIN_SLOTS]) {
    let mut slots: [Option<SkinSlot>; MAX_SKIN_SLOTS] = [None; MAX_SKIN_SLOTS];
    // Same folder the launcher writes `controls.conf` and `layout.conf` to.
    let dir = format!("/data/data/{}/files", "com.klisee.reloaded");
    let mut images: Vec<(&'static str, image::RgbaImage, bool, u8, bool)> = Vec::new();
    let load = |name: &str, fallback: &str| {
        image::open(format!("{}/touch-sprites-active/{}.png", dir, name)).ok().map(|img| (img,true))
            .or_else(|| image::open(format!("{}/{}", dir, fallback)).ok().map(|img| (img,false)))
    };
    for (name, file) in [
        ("joystick", "joystick-base.png"),
        ("joystick-knob", "joystick-knob.png"),
        // The honeycomb the party wheel puts its slots in, and the border that marks the picked one.
        ("selector", "seleccionarpokemon.png"),
        ("selection", "bordeseleccionar.png"),
        ("field-selector", "field-selector.png"),
    ] {
        if let Some((img, custom)) = load(name, file) {
            images.push((name, img.to_rgba8(), custom,0,true));
        }
    }
    for control in crate::touch_controls::controls() {
        // The bundled aspect defines input geometry even with an imported picture.
        if !matches!(control.name,"p"|"l"|"t"|"e"|"pkmn"|"sel_world"|"a"|"m"|"n"|"s"|"space") { if let Ok((w,h)) = image::image_dimensions(format!("{}/{}.png",dir,control.name)) {
            crate::touch_controls::set_art_aspect(control.name,w as f32 / h.max(1) as f32);
        }}
        if let Some((img, custom)) = load(control.name, &format!("{}.png",control.name)) {
            let mut image = img.to_rgba8();
            if let Some(tint) = crate::touch_appearance::settings().tint {
                for pixel in image.pixels_mut() { crate::touch_appearance::tint_pixel(&mut pixel.0, tint); }
            }
            images.push((control.name, image, custom,0,true));
        }
    }
    for(prefix,context)in[("world",crate::touch_controls::WORLD),("battle",crate::touch_controls::BATTLE)]{
        for name in["m","e","p","l","t","space","a"]{
            let original=std::path::Path::new(&format!("{}/touch-sprites-active/{}__{}.original",dir,prefix,name)).is_file();
            let scoped=format!("{}/touch-sprites-active/{}__{}.png",dir,prefix,name);
            let bundled=format!("{}/{}__{}.png",dir,prefix,name);
            let legacy=format!("{}/touch-sprites-active/{}.png",dir,name);
            let (file,custom)=if !original && std::path::Path::new(&scoped).is_file(){(scoped,true)}
                else if !original && std::path::Path::new(&legacy).is_file(){continue}
                else if std::path::Path::new(&bundled).is_file(){(bundled,false)}
                else if original{(format!("{}/{}.png",dir,name),false)}else{continue};
            if let Ok(img)=image::open(file){
                let mut img=img.to_rgba8();
                if let Some(tint)=crate::touch_appearance::settings().tint{for pixel in img.pixels_mut(){crate::touch_appearance::tint_pixel(&mut pixel.0,tint);}}
                images.push((name,img,custom,context,true));
            }else if original{images.push((name,image::RgbaImage::new(1,1),false,context,false));}
        }
    }
    images.sort_by_key(|(_,img,_,_,_)|std::cmp::Reverse(img.height()));
    if images.is_empty() {
        eprintln!("[overlay] sin arte pixel para los controles; se usan las formas dibujadas");
        return (0, slots)
    }
    let sizes: Vec<_> = images.iter().map(|(_,img,_,_,_)| (img.width(),img.height())).collect();
    let mut gpu_limit=0;
    unsafe {gl.GetIntegerv(gl::MAX_TEXTURE_SIZE,&mut gpu_limit);}
    let Some((width,height,positions))=crate::touch_skin::atlas_layout(&sizes,(gpu_limit.max(1) as u32).min(2048)) else {
        eprintln!("[overlay] arte demasiado grande; se usan las formas dibujadas");
        return (0,slots)
    };
    let mut pixels=vec![0u8;(width*height*4) as usize];
    for (index,(name,img,custom,context,has_art)) in images.iter().enumerate() {
        let (x,top)=positions[index];let row=(img.width()*4) as usize;
        for y in 0..img.height() as usize {
            let src=y*row;let dst=(((top+y as u32)*width+x)*4) as usize;
            pixels[dst..dst+row].copy_from_slice(&img.as_raw()[src..src+row]);
        }
        if index<MAX_SKIN_SLOTS {slots[index]=Some(SkinSlot {name,custom:*custom,context:*context,has_art:*has_art,
            aspect:img.width() as f32/img.height().max(1) as f32,
            uv:[x as f32/width as f32,top as f32/height as f32,(x+img.width()) as f32/width as f32,(top+img.height()) as f32/height as f32],
        });}
    }
    unsafe {
        let mut texture = 0;
        gl.GenTextures(1, &mut texture);
        gl.BindTexture(gl::TEXTURE_2D, texture);
        // Nearest keeps the pixel art crisp whatever size the control ends up.
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as _);
        gl.TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as _,
            width as _,
            height as _,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_ptr().cast(),
        );
        assert_eq!(gl.GetError(), 0);
        eprintln!("[overlay] arte pixel de los controles cargado ({} imagenes, {}x{})", images.len(), width, height);
        (texture, slots)
    }
}

/// The clean text labels the launcher draws with a real sans-serif face. They live in their own
/// texture so they can be sampled with LINEAR filtering without blurring the pixel-art controls.
static LABEL_TEX: std::sync::Mutex<GLuint> = std::sync::Mutex::new(0);
static LABELS: std::sync::Mutex<Vec<(String, [f32; 4], f32)>> = std::sync::Mutex::new(Vec::new());

#[cfg(target_os = "android")]
fn label_file(text: &str) -> String {
    let mut name = String::from("label_");
    for c in text.chars() {
        name.push(if c.is_ascii_alphanumeric() { c } else { '_' });
    }
    name.push_str(".png");
    name
}

/// Packs the label images the launcher left in `files/touch-labels`, one slot per label text.
#[cfg(target_os = "android")]
fn load_label_skin(gl: &gl::Gl) -> GLuint {
    let dir = "/data/data/com.klisee.reloaded/files/touch-labels";
    let mut texts: Vec<&'static str> = Vec::new();
    for control in crate::touch_controls::CLASSIC_CONTROLS {
        if let crate::touch_controls::Label::Text(text) = control.label {
            if !texts.contains(&text) {
                texts.push(text);
            }
        }
    }
    let mut images: Vec<(&'static str, image::RgbaImage)> = Vec::new();
    for text in texts {
        if let Ok(img) = image::open(format!("{}/{}", dir, label_file(text))) {
            images.push((text, img.to_rgba8()));
        }
    }
    if images.is_empty() {
        return 0
    }
    let height = images.iter().map(|(_, img)| img.height()).max().unwrap_or(1).max(1);
    let width: u32 = images.iter().map(|(_, img)| img.width()).sum::<u32>().max(1);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut slots: Vec<(String, [f32; 4], f32)> = Vec::new();
    let mut x = 0u32;
    for (text, img) in &images {
        let row = (img.width() * 4) as usize;
        for y in 0..img.height() as usize {
            let src = y * row;
            let dst = (((y as u32) * width + x) * 4) as usize;
            pixels[dst..dst + row].copy_from_slice(&img.as_raw()[src..src + row]);
        }
        slots.push((
            (*text).to_string(),
            [
                x as f32 / width as f32,
                0.0,
                (x + img.width()) as f32 / width as f32,
                img.height() as f32 / height as f32,
            ],
            img.width() as f32 / img.height().max(1) as f32,
        ));
        x += img.width();
    }
    unsafe {
        let mut texture = 0;
        gl.GenTextures(1, &mut texture);
        gl.BindTexture(gl::TEXTURE_2D, texture);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as _);
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as _);
        gl.TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as _,
            width as _,
            height as _,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_ptr().cast(),
        );
        *LABEL_TEX.lock().unwrap() = texture;
        *LABELS.lock().unwrap() = slots;
        eprintln!("[overlay] etiquetas limpias cargadas ({} textos, {}x{})", images.len(), width, height);
        texture
    }
}

#[cfg(target_os = "android")]
fn label_art(text: &str) -> Option<([f32; 4], f32)> {
    LABELS.lock().unwrap().iter().find(|(known, _, _)| known == text).map(|(_, uv, aspect)| (*uv, *aspect))
}

#[cfg(target_os = "android")]
fn label_texture() -> GLuint {
    *LABEL_TEX.lock().unwrap()
}

/// Pushes a label as a texture quad, capped so a wide label still fits inside its button.
#[cfg(target_os = "android")]
fn push_label_sprite(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, half_h: f32, uv: [f32; 4], aspect: f32) {
    let max_w = half_h * 3.0;
    let mut hh = half_h;
    let mut hw = hh * aspect;
    if hw > max_w {
        hh *= max_w / hw;
        hw = max_w;
    }
    push_sprite(out, ww, wh, cx - hw, cy - hh, cx + hw, cy + hh, uv[0], uv[1], uv[2], uv[3]);
}

/// Creates the strip the party's six sprites go in: one square cell each, transparent to start with.
#[cfg(target_os = "android")]
unsafe fn create_party_texture(gl: &gl::Gl) -> GLuint {
    let mut texture = 0;
    gl.GenTextures(1, &mut texture);
    gl.BindTexture(gl::TEXTURE_2D, texture);
    gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
    gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
    gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as _);
    gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as _);
    let width = PARTY_CELL * crate::touch_controls::PICKER_SLOTS as u32;
    gl.TexImage2D(
        gl::TEXTURE_2D,
        0,
        gl::RGBA as _,
        width as _,
        PARTY_CELL as _,
        0,
        gl::RGBA,
        gl::UNSIGNED_BYTE,
        std::ptr::null(),
    );
    assert_eq!(gl.GetError(), 0);
    texture
}

/// Reads the party's sprites out of the game's own files and refills the strip, one cell each. Shiny
/// Pokémon keep the offset the game stores, which is also what picks their folder.
#[cfg(target_os = "android")]
unsafe fn upload_party(gl: &gl::Gl, texture: GLuint, species: [i32; crate::touch_controls::PICKER_SLOTS]) {
    let dir = format!("/data/data/{}/files/game/data", "com.klisee.reloaded");
    let cell = PARTY_CELL as usize;
    let cells = crate::touch_controls::PICKER_SLOTS;
    let stride = cell * cells;
    let mut strip = vec![0u8; stride * cell * 4];
    for (slot, &value) in species.iter().enumerate() {
        if value == 0 {
            continue
        }
        let shiny = (1000..=2000).contains(&value) || (3000..=4000).contains(&value);
        let folder = if shiny { "pkmn_shiny" } else { "pkmn" };
        let Ok(image) = image::open(format!("{}/{}/{}.gif", dir, folder, value)) else {
            continue
        };
        let image = image.to_rgba8();
        let (iw, ih) = (image.width() as usize, image.height() as usize);
        if iw == 0 || ih == 0 {
            continue
        }
        // Scale to fit the cell, square and centred, sampling nearest so the pixels stay crisp.
        let scale = (cell as f32 / iw as f32).min(cell as f32 / ih as f32);
        let (dw, dh) = (((iw as f32 * scale) as usize).max(1), ((ih as f32 * scale) as usize).max(1));
        let (ox, oy) = ((cell - dw) / 2, (cell - dh) / 2);
        for y in 0..dh {
            for x in 0..dw {
                let pixel = image.get_pixel((x * iw / dw) as u32, (y * ih / dh) as u32).0;
                let index = ((oy + y) * stride + slot * cell + ox + x) * 4;
                strip[index..index + 4].copy_from_slice(&pixel);
            }
        }
    }
    gl.BindTexture(gl::TEXTURE_2D, texture);
    gl.TexSubImage2D(
        gl::TEXTURE_2D,
        0,
        0,
        0,
        (cell * cells) as _,
        cell as _,
        gl::RGBA,
        gl::UNSIGNED_BYTE,
        strip.as_ptr().cast(),
    );
}

/// Pushes one vertex, converting window pixels to clip space. The texture coordinate is left at the
/// top-left of the overlay's 1x1 white texture, so a plain colour comes through untouched.
#[cfg(target_os = "android")]
fn push_vertex(out: &mut Vec<f32>, ww: f32, wh: f32, x: f32, y: f32, col: [f32; 4]) {
    out.extend_from_slice(&[x / ww * 2.0 - 1.0, 1.0 - y / wh * 2.0, col[0], col[1], col[2], col[3], 0.0, 0.0]);
}

/// Transforms only the drawing; touch areas and key dispatch remain independent.
#[cfg(target_os = "android")]
fn transform_touch_vertices(vertices: &mut [f32], center: (f32, f32), scale: f32, brightness: f32) {
    let (cx, cy) = (center.0*2.0-1.0, 1.0-center.1*2.0);
    for vertex in vertices.chunks_exact_mut(8) {
        vertex[0] = cx + (vertex[0]-cx)*scale;
        vertex[1] = cy + (vertex[1]-cy)*scale;
        for value in &mut vertex[2..5] { *value *= brightness; }
    }
}

/// Pushes a textured quad, used for the pixel-art control artwork. The colour is white, so the
/// sprite comes through as drawn.
#[cfg(target_os = "android")]
fn push_sprite(
    out: &mut Vec<f32>,
    ww: f32,
    wh: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    u1: f32,
    v1: f32,
    u2: f32,
    v2: f32,
) {
    let mut corner = |x: f32, y: f32, u: f32, v: f32| {
        out.extend_from_slice(&[x / ww * 2.0 - 1.0, 1.0 - y / wh * 2.0, 1.0, 1.0, 1.0, 1.0, u, v]);
    };
    corner(x1, y1, u1, v1);
    corner(x2, y1, u2, v1);
    corner(x2, y2, u2, v2);
    corner(x1, y1, u1, v1);
    corner(x2, y2, u2, v2);
    corner(x1, y2, u1, v2);
}

#[cfg(target_os = "android")]
fn push_tri(out: &mut Vec<f32>, ww: f32, wh: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32), col: [f32; 4]) {
    push_vertex(out, ww, wh, a.0, a.1, col);
    push_vertex(out, ww, wh, b.0, b.1, col);
    push_vertex(out, ww, wh, c.0, c.1, col);
}

#[cfg(target_os = "android")]
fn push_quad(out: &mut Vec<f32>, ww: f32, wh: f32, x1: f32, y1: f32, x2: f32, y2: f32, col: [f32; 4]) {
    push_tri(out, ww, wh, (x1, y1), (x2, y1), (x2, y2), col);
    push_tri(out, ww, wh, (x1, y1), (x2, y2), (x1, y2), col);
}

#[cfg(target_os = "android")]
fn push_circle(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, col: [f32; 4]) {
    const SEG: usize = 24;
    for i in 0..SEG {
        let a0 = (i as f32) / (SEG as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32) / (SEG as f32) * std::f32::consts::TAU;
        push_tri(out, ww, wh, (cx, cy), (cx + r * a0.cos(), cy + r * a0.sin()), (cx + r * a1.cos(), cy + r * a1.sin()), col);
    }
}

#[cfg(target_os = "android")]
fn push_ring(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, thickness: f32, col: [f32; 4]) {
    const SEG: usize = 24;
    let r0 = (r - thickness).max(0.0);
    for i in 0..SEG {
        let a0 = (i as f32) / (SEG as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32) / (SEG as f32) * std::f32::consts::TAU;
        let (c0, s0) = (a0.cos(), a0.sin());
        let (c1, s1) = (a1.cos(), a1.sin());
        push_tri(out, ww, wh, (cx + r * c0, cy + r * s0), (cx + r0 * c0, cy + r0 * s0), (cx + r * c1, cy + r * s1), col);
        push_tri(out, ww, wh, (cx + r * c1, cy + r * s1), (cx + r0 * c0, cy + r0 * s0), (cx + r0 * c1, cy + r0 * s1), col);
    }
}

#[cfg(target_os = "android")]
fn push_arrow(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, size: f32, dir: crate::touch_controls::Dir, col: [f32; 4]) {
    use crate::touch_controls::Dir;
    let (dx, dy) = match dir {
        Dir::Up => (0.0, -1.0),
        Dir::Down => (0.0, 1.0),
        Dir::Left => (-1.0, 0.0),
        Dir::Right => (1.0, 0.0),
    };
    let (px, py) = (-dy, dx);
    let tip = (cx + dx * size, cy + dy * size);
    let b1 = (cx - dx * size * 0.5 + px * size * 0.85, cy - dy * size * 0.5 + py * size * 0.85);
    let b2 = (cx - dx * size * 0.5 - px * size * 0.85, cy - dy * size * 0.5 - py * size * 0.85);
    push_tri(out, ww, wh, tip, b1, b2, col);
}

/// A tiny 3x5 bitmap font, just for the control labels.
#[cfg(target_os = "android")]
fn glyph(c: char) -> Option<[u8; 5]> {
    Some(match c {
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'N' => [0b101, 0b111, 0b101, 0b101, 0b101],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => return None,
    })
}

#[cfg(target_os = "android")]
fn push_text(out: &mut Vec<f32>, ww: f32, wh: f32, text: &str, cx: f32, cy: f32, px: f32, col: [f32; 4]) {
    let chars: Vec<char> = text.chars().collect();
    let char_w = 4.0 * px; // 3 columns of glyph + 1 gap
    let total = char_w * chars.len() as f32 - px;
    let mut x = cx - total / 2.0;
    for ch in chars {
        if let Some(g) = glyph(ch) {
            for (row, bits) in g.iter().enumerate() {
                for col_i in 0..3u8 {
                    if *bits & (1u8 << (2 - col_i)) != 0 {
                        let gx = x + f32::from(col_i) * px;
                        let gy = cy + (row as f32 - 2.0) * px;
                        push_quad(out, ww, wh, gx, gy, gx + px, gy + px, col);
                    }
                }
            }
        }
        x += char_w;
    }
}

// --- New overlay style: smooth, semi-transparent shapes. All geometry, so it stays crisp at any size
// and needs no artwork.

#[cfg(target_os = "android")]
fn lerp_col(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[cfg(target_os = "android")]
fn with_alpha(mut col: [f32; 4], alpha: f32) -> [f32; 4] {
    col[3] *= alpha;
    col
}

/// The outline of a rounded rectangle, walked clockwise so it can be drawn as a fan from the centre.
#[cfg(target_os = "android")]
fn round_rect_outline(cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> Vec<(f32, f32)> {
    const SEG: usize = 5;
    let r = r.min(hw).min(hh).max(0.0);
    let corners = [
        (cx + hw - r, cy - hh + r, -90.0_f32),
        (cx + hw - r, cy + hh - r, 0.0),
        (cx - hw + r, cy + hh - r, 90.0),
        (cx - hw + r, cy - hh + r, 180.0),
    ];
    let mut points = Vec::with_capacity(4 * (SEG + 1));
    for (ax, ay, start) in corners {
        for i in 0..=SEG {
            let a = (start + 90.0 * i as f32 / SEG as f32).to_radians();
            points.push((ax + r * a.cos(), ay + r * a.sin()));
        }
    }
    points
}

/// A filled rounded rectangle with a vertical gradient (top colour down to bottom colour).
#[cfg(target_os = "android")]
fn push_round_rect(
    out: &mut Vec<f32>,
    ww: f32,
    wh: f32,
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    r: f32,
    top: [f32; 4],
    bottom: [f32; 4],
) {
    if hw <= 0.0 || hh <= 0.0 {
        return
    }
    let points = round_rect_outline(cx, cy, hw, hh, r);
    let mid = lerp_col(top, bottom, 0.5);
    let span = (2.0 * hh).max(0.0001);
    let n = points.len();
    for i in 0..n {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % n];
        let c0 = lerp_col(top, bottom, (y0 - (cy - hh)) / span);
        let c1 = lerp_col(top, bottom, (y1 - (cy - hh)) / span);
        push_vertex(out, ww, wh, cx, cy, mid);
        push_vertex(out, ww, wh, x0, y0, c0);
        push_vertex(out, ww, wh, x1, y1, c1);
    }
}

/// A filled circle with a vertical gradient, for the joystick and the D-pad's centre.
#[cfg(target_os = "android")]
fn push_gradient_circle(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, top: [f32; 4], bottom: [f32; 4]) {
    if r <= 0.0 {
        return
    }
    const SEG: usize = 40;
    let mid = lerp_col(top, bottom, 0.5);
    let span = (2.0 * r).max(0.0001);
    for i in 0..SEG {
        let a0 = i as f32 / SEG as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / SEG as f32 * std::f32::consts::TAU;
        let (x0, y0) = (cx + r * a0.cos(), cy + r * a0.sin());
        let (x1, y1) = (cx + r * a1.cos(), cy + r * a1.sin());
        let c0 = lerp_col(top, bottom, (y0 - (cy - r)) / span);
        let c1 = lerp_col(top, bottom, (y1 - (cy - r)) / span);
        push_vertex(out, ww, wh, cx, cy, mid);
        push_vertex(out, ww, wh, x0, y0, c0);
        push_vertex(out, ww, wh, x1, y1, c1);
    }
}

/// A round, soft button: shadow, light rim, gradient fill and its label.
#[cfg(target_os = "android")]
fn push_soft_button(
    out: &mut Vec<f32>,
    ww: f32,
    wh: f32,
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    pressed: bool,
    label: crate::touch_controls::Label,
    opacity: f32,
) {
    let radius = hw.min(hh) * 0.44;
    let shadow = with_alpha([0.0, 0.0, 0.0, 0.32], opacity);
    let rim = with_alpha([1.0, 1.0, 1.0, 0.40], opacity);
    let (top, bottom) = if pressed {
        (with_alpha([1.0, 0.90, 0.35, 0.85], opacity), with_alpha([0.95, 0.60, 0.05, 0.85], opacity))
    } else {
        (with_alpha([0.20, 0.23, 0.34, 0.50], opacity), with_alpha([0.09, 0.10, 0.16, 0.34], opacity))
    };
    let ink = if pressed {
        [0.16, 0.10, 0.0, 0.95]
    } else {
        with_alpha([0.97, 0.98, 1.0, 0.96], opacity)
    };
    // The shadow sits a touch lower and a touch bigger, so the button reads as floating.
    push_round_rect(out, ww, wh, cx, cy + hh * 0.10, hw * 1.06, hh * 1.06, radius, shadow, shadow);
    // The rim, then the fill on top of it.
    push_round_rect(out, ww, wh, cx, cy, hw * 1.09, hh * 1.09, radius * 1.09, rim, rim);
    push_round_rect(out, ww, wh, cx, cy, hw, hh, radius, top, bottom);
    match label {
        crate::touch_controls::Label::Text(text) => {
            let len = text.chars().count().max(1) as f32;
            let px = (hh * 0.46).min(hw * 1.5 / (len * 4.0)).max(1.0);
            push_text(out, ww, wh, text, cx, cy, px, ink);
        },
        crate::touch_controls::Label::Arrow(dir) => push_arrow(out, ww, wh, cx, cy, hh * 0.5, dir, ink),
        crate::touch_controls::Label::None => {},
    }
}

/// The joystick of the new overlay: a soft translucent ring with a knob that follows the thumb.
#[cfg(target_os = "android")]
fn push_soft_joystick(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, ox: f32, oy: f32, opacity: f32) {
    let shadow = with_alpha([0.0, 0.0, 0.0, 0.28], opacity);
    let rim = with_alpha([1.0, 1.0, 1.0, 0.28], opacity);
    let top = with_alpha([0.22, 0.25, 0.36, 0.44], opacity);
    let bottom = with_alpha([0.09, 0.10, 0.16, 0.30], opacity);
    push_gradient_circle(out, ww, wh, cx, cy + r * 0.05, r * 1.03, shadow, shadow);
    push_gradient_circle(out, ww, wh, cx, cy, r, top, bottom);
    push_ring(out, ww, wh, cx, cy, r, 3.0, rim);
    let (kx, ky) = (cx + ox * r * 0.62, cy + oy * r * 0.62);
    let kr = r * 0.42;
    push_gradient_circle(
        out,
        ww,
        wh,
        kx,
        ky,
        kr,
        with_alpha([0.82, 0.87, 1.0, 0.55], opacity),
        with_alpha([0.52, 0.58, 0.85, 0.42], opacity),
    );
    push_ring(out, ww, wh, kx, ky, kr, 2.5, with_alpha([1.0, 1.0, 1.0, 0.50], opacity));
}

/// The D-pad of the new overlay: a soft translucent cross with four arrows.
#[cfg(target_os = "android")]
fn push_soft_dpad(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, opacity: f32) {
    use crate::touch_controls::Dir;
    let shadow = with_alpha([0.0, 0.0, 0.0, 0.26], opacity);
    let rim = with_alpha([1.0, 1.0, 1.0, 0.32], opacity);
    let top = with_alpha([0.20, 0.23, 0.34, 0.46], opacity);
    let bottom = with_alpha([0.09, 0.10, 0.16, 0.32], opacity);
    let arm = r * 1.02;
    let thick = r * 0.38;
    let radius = r * 0.30;
    push_round_rect(out, ww, wh, cx, cy + r * 0.07, arm * 1.05, thick * 1.12, radius, shadow, shadow);
    push_round_rect(out, ww, wh, cx, cy + r * 0.07, thick * 1.12, arm * 1.05, radius, shadow, shadow);
    push_round_rect(out, ww, wh, cx, cy, arm * 1.05, thick * 1.10, radius, rim, rim);
    push_round_rect(out, ww, wh, cx, cy, thick * 1.10, arm * 1.05, radius, rim, rim);
    push_round_rect(out, ww, wh, cx, cy, arm, thick, radius * 0.95, top, bottom);
    push_round_rect(out, ww, wh, cx, cy, thick, arm, radius * 0.95, top, bottom);
    push_gradient_circle(out, ww, wh, cx, cy, thick * 0.92, top, bottom);
    let ink = with_alpha([0.97, 0.98, 1.0, 0.95], opacity);
    push_arrow(out, ww, wh, cx, cy - r * 0.62, r * 0.30, Dir::Up, ink);
    push_arrow(out, ww, wh, cx, cy + r * 0.62, r * 0.30, Dir::Down, ink);
    push_arrow(out, ww, wh, cx - r * 0.62, cy, r * 0.30, Dir::Left, ink);
    push_arrow(out, ww, wh, cx + r * 0.62, cy, r * 0.30, Dir::Right, ink);
}

/// A modern control: a dark translucent disc with a soft rim, an inner ring and its label. Wide
/// labels become a rounded pill instead of being squeezed into a circle.
#[cfg(target_os = "android")]
fn push_modern_control(
    out: &mut Vec<f32>,
    ww: f32,
    wh: f32,
    shape: crate::touch_controls::Shape,
    pressed: bool,
    aspect: Option<f32>,
    tint: Option<[f32; 3]>,
) -> (f32, f32, f32) {
    let (cx, cy, r) = match shape {
        crate::touch_controls::Shape::Circle { cx, cy, r } => (cx * ww, cy * wh, r * ww),
        crate::touch_controls::Shape::Rect { cx, cy, w, h } => (cx * ww, cy * wh, (w * ww + h * wh) / 4.0),
    };
    let aspect = aspect.unwrap_or(1.0);
    // The chosen colour multiplies the button greys, so the discs take its hue and stay translucent.
    let t = tint.unwrap_or([1.0, 1.0, 1.0]);
    let shade = |g: f32, a: f32| [t[0] * g, t[1] * g, t[2] * g, a];
    let base = if pressed { shade(0.30, 0.50) } else { shade(0.20, 0.38) };
    let groove = if pressed { shade(0.15, 0.54) } else { shade(0.115, 0.42) };
    let raised = if pressed { shade(0.36, 0.48) } else { shade(0.26, 0.40) };
    let inner = if pressed { shade(0.33, 0.50) } else { shade(0.235, 0.40) };
    let ink = if pressed { [0.98, 0.98, 1.0, 0.98] } else { [0.62, 0.62, 0.65, 0.95] };
    push_circle(out, ww, wh, cx, cy, r * 1.06, [0.0, 0.0, 0.0, 0.22]);
    if (aspect - 1.0).abs() > 0.12 {
        let hw = r * aspect;
        push_round_rect(out, ww, wh, cx, cy, hw * 1.05, r * 1.05, r * 0.46, groove, groove);
        push_round_rect(out, ww, wh, cx, cy, hw, r, r * 0.40, base, base);
        push_round_rect(out, ww, wh, cx, cy, hw * 0.92, r * 0.92, r * 0.36, groove, groove);
        push_round_rect(out, ww, wh, cx, cy, hw * 0.86, r * 0.86, r * 0.34, raised, raised);
        push_round_rect(out, ww, wh, cx, cy, hw * 0.74, r * 0.74, r * 0.30, groove, groove);
        push_round_rect(out, ww, wh, cx, cy, hw * 0.66, r * 0.66, r * 0.26, inner, inner);
    } else {
        push_circle(out, ww, wh, cx, cy, r, base);
        push_ring(out, ww, wh, cx, cy, r * 0.92, (r * 0.05).max(2.0), groove);
        push_ring(out, ww, wh, cx, cy, r * 0.82, (r * 0.055).max(2.0), raised);
        push_ring(out, ww, wh, cx, cy, r * 0.70, (r * 0.04).max(2.0), groove);
        push_circle(out, ww, wh, cx, cy, r * 0.62, inner);
        push_ring(out, ww, wh, cx, cy, r * 0.995, (r * 0.02).max(1.0), shade(0.36, 0.20));
    }
    let _ = ink;
    (cx, cy, r)
}

#[cfg(target_os = "android")]
fn poly_dir(from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt().max(0.0001);
    (dx / len, dy / len)
}

/// The outline of a rounded cross, walked in order, so the D-pad reads as one clean shape.
#[cfg(target_os = "android")]
fn plus_points(cx: f32, cy: f32, arm: f32, thick: f32, radius: f32) -> Vec<(f32, f32)> {
    let corners = [
        (cx - thick, cy - arm), (cx + thick, cy - arm), (cx + thick, cy - thick), (cx + arm, cy - thick),
        (cx + arm, cy + thick), (cx + thick, cy + thick), (cx + thick, cy + arm), (cx - thick, cy + arm),
        (cx - thick, cy + thick), (cx - arm, cy + thick), (cx - arm, cy - thick), (cx - thick, cy - thick),
    ];
    let n = corners.len();
    let mut points = Vec::with_capacity(n * 4);
    for i in 0..n {
        let p = corners[i];
        let prev = corners[(i + n - 1) % n];
        let next = corners[(i + 1) % n];
        let d1 = poly_dir(p, prev);
        let d2 = poly_dir(p, next);
        let a = (p.0 + d1.0 * radius, p.1 + d1.1 * radius);
        let b = (p.0 + d2.0 * radius, p.1 + d2.1 * radius);
        for s in 0..=3 {
            let t = s as f32 / 3.0;
            let u = 1.0 - t;
            points.push((u * u * a.0 + 2.0 * u * t * p.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * p.1 + t * t * b.1));
        }
    }
    points
}

/// Fills a polygon as a fan from (cx, cy), with a vertical gradient.
#[cfg(target_os = "android")]
fn push_polygon(out: &mut Vec<f32>, ww: f32, wh: f32, points: &[(f32, f32)], cx: f32, cy: f32, top: [f32; 4], bottom: [f32; 4]) {
    if points.len() < 3 {
        return
    }
    let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    let span = (max_y - min_y).max(0.0001);
    let mid = lerp_col(top, bottom, 0.5);
    for i in 0..points.len() {
        let p0 = points[i];
        let p1 = points[(i + 1) % points.len()];
        let c0 = lerp_col(top, bottom, (p0.1 - min_y) / span);
        let c1 = lerp_col(top, bottom, (p1.1 - min_y) / span);
        push_vertex(out, ww, wh, cx, cy, mid);
        push_vertex(out, ww, wh, p0.0, p0.1, c0);
        push_vertex(out, ww, wh, p1.0, p1.1, c1);
    }
}

/// The modern movement control: a clean translucent disc with a single rounded cross that lights up
/// the arm being pressed, or a knob that follows the thumb (joystick).
#[cfg(target_os = "android")]
fn push_modern_movement(out: &mut Vec<f32>, ww: f32, wh: f32, cx: f32, cy: f32, r: f32, ox: f32, oy: f32, dpad: bool, tint: Option<[f32; 3]>) {
    use crate::touch_controls::Dir;
    let t = tint.unwrap_or([1.0, 1.0, 1.0]);
    let shade = |g: f32, a: f32| [t[0] * g, t[1] * g, t[2] * g, a];
    let disc = shade(0.21, 0.36);
    let edge = shade(0.34, 0.18);
    let outline = shade(0.10, 0.66);
    let fill = shade(0.30, 0.55);
    let ink = shade(0.62, 0.80);
    push_circle(out, ww, wh, cx, cy, r * 1.05, [0.0, 0.0, 0.0, 0.18]);
    push_circle(out, ww, wh, cx, cy, r, disc);
    push_ring(out, ww, wh, cx, cy, r * 0.995, (r * 0.02).max(1.0), edge);
    if dpad {
        let arm = r * 0.74;
        let thick = r * 0.255;
        let radius = r * 0.13;
        let border = r * 0.065;
        // A slightly larger cross behind the fill gives a continuous dark outline: no stroke seams
        // at the corners, which is what made the previous version look hatched.
        let outer = plus_points(cx, cy, arm + border, thick + border, radius + border * 0.6);
        push_polygon(out, ww, wh, &outer, cx, cy, outline, outline);
        let points = plus_points(cx, cy, arm, thick, radius);
        push_polygon(out, ww, wh, &points, cx, cy, fill, fill);
        let dirs = crate::touch_controls::move_dirs();
        let arms = [(0.0_f32, -1.0_f32), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)];
        // Light only the pressed arm, from its tip to the centre (never across it), then its arrow
        // goes dark so it reads as pressed.
        for (i, (dx, dy)) in arms.iter().enumerate() {
            if !dirs[i] {
                continue
            }
            // Fill the arm from its tip down to the edge of the central square, so it stops at the
            // junction instead of reaching the centre.
            let outer = (arm + thick) / 2.0;
            let inner = (arm - thick) / 2.0;
            let (ax, ay, hw, hh) = if *dx < 0.0 {
                (cx - outer, cy, inner, thick)
            } else if *dx > 0.0 {
                (cx + outer, cy, inner, thick)
            } else if *dy < 0.0 {
                (cx, cy - outer, thick, inner)
            } else {
                (cx, cy + outer, thick, inner)
            };
            push_round_rect(out, ww, wh, ax, ay, hw, hh, radius * 0.8, [t[0]*0.5+0.5, t[1]*0.5+0.5, t[2]*0.5+0.5, 0.92], [t[0]*0.45+0.55, t[1]*0.45+0.55, t[2]*0.45+0.55, 0.92]);
        }
        for (i, (dx, dy)) in arms.iter().enumerate() {
            let color = if dirs[i] { shade(0.14, 0.95) } else { ink };
            let dir = match i {
                0 => Dir::Up,
                1 => Dir::Down,
                2 => Dir::Left,
                _ => Dir::Right,
            };
            push_arrow(out, ww, wh, cx + dx * r * 0.45, cy + dy * r * 0.45, r * 0.115, dir, color);
        }
    } else {
        let (kx, ky) = (cx + ox * r * 0.52, cy + oy * r * 0.52);
        let kr = r * 0.34;
        push_circle(out, ww, wh, kx, ky, kr * 1.06, [0.0, 0.0, 0.0, 0.22]);
        push_circle(out, ww, wh, kx, ky, kr, shade(0.27, 0.55));
        push_ring(out, ww, wh, kx, ky, kr, (r * 0.04).max(2.0), shade(0.40, 0.50));
    }
}

#[cfg(target_os = "android")]
impl RendererImpl {
    /// Builds (once) the tiny program used to draw the touch controls over the frame.
    fn overlay_program(&mut self) -> Overlay {
        if let Some(o) = &self.overlay {
            return Overlay {
                program: o.program,
                vao: o.vao,
                vbo: o.vbo,
                white: o.white,
                skin: o.skin,
                slots: o.slots,
                loc_tex: o.loc_tex,
                party: o.party,
                party_version: o.party_version,
            }
        }
        unsafe {
            let vs_src = b"#version 300 es\nlayout(location=0) in vec2 a_pos;\nlayout(location=1) in vec4 a_col;\nlayout(location=2) in vec2 a_uv;\nout vec4 v_col;\nout vec2 v_uv;\nvoid main(){ gl_Position = vec4(a_pos, 0.0, 1.0); v_col = a_col; v_uv = a_uv; }\0";
            let fs_src = b"#version 300 es\nprecision mediump float;\nin vec4 v_col;\nin vec2 v_uv;\nuniform sampler2D u_tex;\nout vec4 o_col;\nvoid main(){ o_col = v_col * texture(u_tex, v_uv); }\0";
            let vs = self.gl.CreateShader(gl::VERTEX_SHADER);
            self.gl.ShaderSource(vs, 1, &(vs_src.as_ptr() as *const GLchar), std::ptr::null());
            self.gl.CompileShader(vs);
            let mut vs_ok = 0;
            self.gl.GetShaderiv(vs, gl::COMPILE_STATUS, &mut vs_ok);
            if vs_ok == 0 {
                eprintln!("[overlay] fallo al compilar el vertex shader");
            }
            let fs = self.gl.CreateShader(gl::FRAGMENT_SHADER);
            self.gl.ShaderSource(fs, 1, &(fs_src.as_ptr() as *const GLchar), std::ptr::null());
            self.gl.CompileShader(fs);
            let mut fs_ok = 0;
            self.gl.GetShaderiv(fs, gl::COMPILE_STATUS, &mut fs_ok);
            if fs_ok == 0 {
                eprintln!("[overlay] fallo al compilar el fragment shader");
            }
            let program = self.gl.CreateProgram();
            self.gl.AttachShader(program, vs);
            self.gl.AttachShader(program, fs);
            self.gl.LinkProgram(program);
            let mut ok = 0;
            self.gl.GetProgramiv(program, gl::LINK_STATUS, &mut ok);
            if ok == 0 {
                eprintln!("[overlay] fallo al enlazar el programa del overlay");
            }
            self.gl.DeleteShader(vs);
            self.gl.DeleteShader(fs);
            let loc_tex = self.gl.GetUniformLocation(program, b"u_tex\0".as_ptr().cast());

            // A 1x1 white texture: the plain shapes carry their colour in the vertex, and sampling
            // white leaves it untouched, so one shader covers both shapes and sprites.
            let mut white = 0;
            self.gl.GenTextures(1, &mut white);
            self.gl.BindTexture(gl::TEXTURE_2D, white);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as _);
            let white_pixel: [u8; 4] = [255, 255, 255, 255];
            self.gl.TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as _,
                1,
                1,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                white_pixel.as_ptr().cast(),
            );

            let (skin, slots) = load_skin(&self.gl);
            load_label_skin(&self.gl);
            let party = create_party_texture(&self.gl);

            let mut vao = 0;
            self.gl.GenVertexArrays(1, &mut vao);
            let mut vbo = 0;
            self.gl.GenBuffers(1, &mut vbo);
            // u64::MAX on purpose: it no longer matches the party's version, so the first frame
            // fills the strip.
            let fields = Overlay { program, vao, vbo, white, skin, slots, loc_tex, party, party_version: u64::MAX };
            self.overlay = Some(fields);
            fields
        }
    }

    /// Draws the on-screen touch controls over the presented frame with its own GL state, so the
    /// emulator's render state is never disturbed.
    fn draw_touch_overlay(&mut self, window_width: u32, window_height: u32) {
        use crate::touch_controls;
        if window_width == 0 || window_height == 0 {
            return
        }
        let mut overlay = self.overlay_program();
        let (ww, wh) = (window_width as f32, window_height as f32);

        // The party's sprites come from the game itself, so refill their strip when the party changes.
        let party_version = touch_controls::party_version();
        if overlay.party_version != party_version {
            unsafe { upload_party(&self.gl, overlay.party, touch_controls::party_species()) };
            overlay.party_version = party_version;
            if let Some(stored) = self.overlay.as_mut() {
                stored.party_version = party_version;
            }
        }

        let mut verts: Vec<f32> = Vec::new();
        // Textured geometry (the pixel-art controls) goes in its own buffer, because it samples the
        // artwork rather than the 1x1 white texture the plain shapes use.
        let mut sprites: Vec<f32> = Vec::new();
        // The party's sprites come from a texture of their own, so they travel in their own batch.
        let mut party_sprites: Vec<f32> = Vec::new();
        // The clean label atlas always travels in its own batch, because it is sampled with LINEAR.
        let mut label_sprites: Vec<f32> = Vec::new();
        let style = touch_controls::overlay_style();
        let opacity = touch_controls::sprite_opacity();
        let appearance = crate::touch_appearance::settings();
        for (i, c) in touch_controls::controls().iter().enumerate() {
            if !touch_controls::is_active(i) {
                continue
            }
            // While the wheel is open it stands in for the button that opened it, so that button and
            // its label get out of the way.
            if touch_controls::wheel_open() && c.name == touch_controls::PARTY_NAME {
                continue
            }
            let pressed = touch_controls::is_pressed(i);
            let (visual_scale, brightness) = crate::touch_appearance::visual(i);
            let plain_start = verts.len();
            let sprite_start = sprites.len();
            let center = match touch_controls::window_shape_for(i, ww, wh) {
                touch_controls::Shape::Circle { cx, cy, .. } | touch_controls::Shape::Rect { cx, cy, .. } => (cx, cy),
            };
            // Moderno: the artwork we already had, drawn semi-transparent.
            if appearance.style == crate::touch_appearance::Style::Modern {
                if let Some(uv) = overlay.art(c.name) {
                    let (cx, cy, half) = match touch_controls::window_shape_for(i, ww, wh) {
                        touch_controls::Shape::Circle { cx, cy, r } => (cx * ww, cy * wh, r * ww),
                        touch_controls::Shape::Rect { cx, cy, w, h } => (cx * ww, cy * wh, (w * ww + h * wh) / 4.0),
                    };
                    let aspect = overlay.art_aspect(c.name).unwrap_or(1.0);
                    let (half_w, half_h) = (half * aspect, half);
                    push_sprite(&mut sprites, ww, wh, cx - half_w, cy - half_h, cx + half_w, cy + half_h, uv[0], uv[1], uv[2], uv[3]);
                    for v in sprites[sprite_start..].chunks_exact_mut(8) {
                        v[5] *= touch_controls::sprite_opacity();
                        if let Some(t) = appearance.tint {
                            v[2] *= t[0];
                            v[3] *= t[1];
                            v[4] *= t[2];
                        }
                    }
                    transform_touch_vertices(&mut sprites[sprite_start..], center, visual_scale, brightness);
                    continue;
                }
            }
            // Clasico: our soft discs, and the fallback for a Modern control without artwork.
            let (lcx, lcy, lr) = push_modern_control(
                &mut verts,
                ww,
                wh,
                touch_controls::window_shape_for(i, ww, wh),
                pressed,
                touch_controls::control_art_aspect(c.name),
                appearance.tint,
            );
            match c.label {
                touch_controls::Label::Text(text) => {
                    if let Some((uv, aspect)) = label_art(text) {
                        push_label_sprite(&mut label_sprites, ww, wh, lcx, lcy, lr * 0.42, uv, aspect);
                    } else {
                        push_text(&mut verts, ww, wh, text, lcx, lcy, lr * 0.28, [0.72, 0.72, 0.76, 0.92]);
                    }
                },
                touch_controls::Label::Arrow(dir) => push_arrow(&mut verts, ww, wh, lcx, lcy, lr * 0.5, dir, [0.72, 0.72, 0.76, 0.92]),
                touch_controls::Label::None => {},
            }
            transform_touch_vertices(&mut verts[plain_start..], center, visual_scale, brightness);
        }
        if !label_sprites.is_empty() {
            let factor = touch_controls::sprite_opacity();
            for v in label_sprites.chunks_exact_mut(8) {
                v[5] *= factor;
            }
        }

        if crate::field_controls::open(){
            if let Some(uv)=overlay.art("field-selector"){
                let(x,y,w,h)=crate::field_controls::rect(ww,wh);let start=sprites.len();
                push_sprite(&mut sprites,ww,wh,x,y,x+w,y+h,uv[0],uv[1],uv[2],uv[3]);
                for v in sprites[start..].chunks_exact_mut(8){v[5]*=touch_controls::sprite_opacity();}
            }
        }
        // Movement joystick: the pixel-art base with the knob on top that follows the thumb, or the
        // drawn ring when the artwork is not available.
        if touch_controls::movement_active() {
            let (jcx, jcy) = touch_controls::joystick_center_in_window(ww, wh);
            let (cx, cy) = (jcx * ww, jcy * wh);
            let r = touch_controls::joystick_radius() * wh;
            let (ox, oy) = touch_controls::movement_offset();
            let mut drew_sprite = false;
            // Moderno draws the joystick artwork, semi-transparent.
            if appearance.style == crate::touch_appearance::Style::Modern {
                if let (Some(base), Some(knob)) = (overlay.art("joystick"), overlay.art("joystick-knob")) {
                    let start = sprites.len();
                    let half = r * 1.05;
                    push_sprite(&mut sprites, ww, wh, cx - half, cy - half, cx + half, cy + half, base[0], base[1], base[2], base[3]);
                    let (kx, ky) = (cx + ox * r * 0.72, cy + oy * r * 0.72);
                    let kh = r * 0.88;
                    push_sprite(&mut sprites, ww, wh, kx - kh, ky - kh, kx + kh, ky + kh, knob[0], knob[1], knob[2], knob[3]);
                    for v in sprites[start..].chunks_exact_mut(8) {
                        v[5] *= touch_controls::sprite_opacity();
                        if let Some(t) = appearance.tint {
                            v[2] *= t[0];
                            v[3] *= t[1];
                            v[4] *= t[2];
                        }
                    }
                    drew_sprite = true;
                }
            }
            if !drew_sprite {
                push_modern_movement(
                    &mut verts,
                    ww,
                    wh,
                    cx,
                    cy,
                    r,
                    ox,
                    oy,
                    touch_controls::move_mode() == touch_controls::MoveMode::Dpad,
                    appearance.tint,
                );
            }
        }

        // Party wheel: the honeycomb artwork, each Pokémon of the team in its own hexagon and the
        // selected border on whichever one the finger is pointing at.
        if touch_controls::wheel_open() {
            let species = touch_controls::party_species();
            let (cx, cy) = touch_controls::wheel_center(ww, wh);
            let radius = touch_controls::wheel_radius(ww, wh);
            let selected = touch_controls::wheel_selected();
            // Sized so the artwork's own hexagons land right where the slots are.
            if let Some(uv) = overlay.art("selector") {
                let half = radius / touch_controls::SELECTOR_SLOT_DISTANCE;
                let tall = half * touch_controls::SELECTOR_ASPECT;
                push_sprite(&mut sprites, ww, wh, cx - half, cy - tall, cx + half, cy + tall, uv[0], uv[1], uv[2], uv[3]);
            }
            let selection = overlay.art("selection");
            let cell = 1.0 / touch_controls::PICKER_SLOTS as f32;
            for index in 0..touch_controls::PICKER_SLOTS {
                let (ix, iy) = touch_controls::wheel_item_center(index, ww, wh);
                if species[index] != 0 {
                    let x0 = index as f32 * cell;
                    // Just inside the hexagon, so the sprite reads as sitting in it.
                    let inner = radius * 0.42;
                    push_sprite(&mut party_sprites, ww, wh, ix - inner, iy - inner, ix + inner, iy + inner, x0, 0.0, x0 + cell, 1.0);
                }
                if selected == index as i32 {
                    // The border is one hexagon, sized to the artwork's own so it nests inside the
                    // outline it draws.
                    if let Some(uv) = selection {
                        let wide = radius * 0.55;
                        let tall = wide * 1.155;
                        push_sprite(&mut sprites, ww, wh, ix - wide, iy - tall, ix + wide, iy + tall, uv[0], uv[1], uv[2], uv[3]);
                    }
                }
            }
        }

        if verts.is_empty() && sprites.is_empty() && party_sprites.is_empty() && label_sprites.is_empty() {
            return
        }
        unsafe {
            let gl = &self.gl;
            let mut prev_program = 0;
            gl.GetIntegerv(gl::CURRENT_PROGRAM, &mut prev_program);
            let mut prev_vp = [0i32; 4];
            gl.GetIntegerv(gl::VIEWPORT, prev_vp.as_mut_ptr());
            let mut prev_vbo = 0;
            gl.GetIntegerv(gl::ARRAY_BUFFER_BINDING, &mut prev_vbo);
            let mut prev_vao = 0;
            gl.GetIntegerv(gl::VERTEX_ARRAY_BINDING, &mut prev_vao);
            let mut prev_fbo = 0;
            gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut prev_fbo);
            let prev_blend = gl.IsEnabled(gl::BLEND) == gl::TRUE;
            let prev_depth = gl.IsEnabled(gl::DEPTH_TEST) == gl::TRUE;

            // `present` restores whatever framebuffer was bound while the game drew. The overlay
            // must go to the screen (framebuffer 0), not back into the game's offscreen target.
            gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            gl.Viewport(0, 0, window_width as i32, window_height as i32);
            gl.Disable(gl::SCISSOR_TEST);
            gl.Disable(gl::DEPTH_TEST);
            gl.Enable(gl::BLEND);
            gl.BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl.UseProgram(overlay.program);
            gl.BindVertexArray(overlay.vao);
            gl.BindBuffer(gl::ARRAY_BUFFER, overlay.vbo);
            gl.ActiveTexture(gl::TEXTURE0);
            gl.Uniform1i(overlay.loc_tex, 0);
            // x, y, r, g, b, a, u, v
            const STRIDE: i32 = 32;
            gl.EnableVertexAttribArray(0);
            gl.VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, STRIDE, std::ptr::null());
            gl.EnableVertexAttribArray(1);
            gl.VertexAttribPointer(1, 4, gl::FLOAT, gl::FALSE, STRIDE, 8 as *const std::ffi::c_void);
            gl.EnableVertexAttribArray(2);
            gl.VertexAttribPointer(2, 2, gl::FLOAT, gl::FALSE, STRIDE, 24 as *const std::ffi::c_void);

            // Plain shapes first (they sample the white pixel, so their colour passes through), then
            // the pixel-art controls, which sample the artwork.
            gl.BindTexture(gl::TEXTURE_2D, overlay.white);
            gl.BufferData(gl::ARRAY_BUFFER, (verts.len() * 4) as _, verts.as_ptr().cast(), gl::STREAM_DRAW);
            gl.DrawArrays(gl::TRIANGLES, 0, (verts.len() / 8) as i32);
            if !sprites.is_empty() {
                gl.BindTexture(gl::TEXTURE_2D, overlay.skin);
                gl.BufferData(gl::ARRAY_BUFFER, (sprites.len() * 4) as _, sprites.as_ptr().cast(), gl::STREAM_DRAW);
                gl.DrawArrays(gl::TRIANGLES, 0, (sprites.len() / 8) as i32);
            }
            if !label_sprites.is_empty() {
                // The clean label atlas, sampled with LINEAR so the text stays smooth.
                gl.BindTexture(gl::TEXTURE_2D, label_texture());
                gl.BufferData(gl::ARRAY_BUFFER, (label_sprites.len() * 4) as _, label_sprites.as_ptr().cast(), gl::STREAM_DRAW);
                gl.DrawArrays(gl::TRIANGLES, 0, (label_sprites.len() / 8) as i32);
            }
            if !party_sprites.is_empty() {
                // The game's own sprites, which live in their own strip.
                gl.BindTexture(gl::TEXTURE_2D, overlay.party);
                gl.BufferData(gl::ARRAY_BUFFER, (party_sprites.len() * 4) as _, party_sprites.as_ptr().cast(), gl::STREAM_DRAW);
                gl.DrawArrays(gl::TRIANGLES, 0, (party_sprites.len() / 8) as i32);
            }

            gl.BindVertexArray(prev_vao as u32);
            gl.BindBuffer(gl::ARRAY_BUFFER, prev_vbo as u32);
            gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, prev_fbo as u32);
            gl.UseProgram(prev_program as u32);
            gl.Viewport(prev_vp[0], prev_vp[1], prev_vp[2], prev_vp[3]);
            if prev_blend {
                gl.Enable(gl::BLEND)
            } else {
                gl.Disable(gl::BLEND)
            }
            if prev_depth {
                gl.Enable(gl::DEPTH_TEST)
            } else {
                gl.Disable(gl::DEPTH_TEST)
            }
        }
    }
}

impl RendererTrait for RendererImpl {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn max_texture_size(&self) -> u32 {
        let mut size: GLint = 0;
        unsafe {
            self.gl.GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut size);
            if size == 16384 && CStr::from_ptr(self.gl.GetString(gl::VENDOR).cast()).to_bytes() == b"Intel" {
                // Intel driver bug throws GL_OUT_OF_MEMORY when allocating 16384x16384 texture
                // TODO: find proper fix or maybe allow allocating 16384x8192?
                size = 8192;
            }
            assert_eq!(self.gl.GetError(), 0);
        }
        let raw = size.max(0) as u32;
        if raw == 0 {
            return 64
        }
        // AtlasBuilder requires a power of two, and some drivers report a value that
        // isn't one (16383 rather than 16384), so round down to the nearest power of two.
        let pow2 = (raw as u64).next_power_of_two() as u32 >> usize::from(!raw.is_power_of_two());
        // A max-size atlas would be gigabytes of RGBA; that fails on mobile GPUs and
        // mostly on desktops too. Several smaller atlases work just as well.
        let cap = if cfg!(target_os = "android") { 4096 } else { 8192 };
        pow2.min(cap)
    }

    fn push_atlases(&mut self, mut atl: AtlasBuilder) -> Result<(), String> {
        assert!(self.atlas_packers.is_empty(), "atlases should be initialized only once");

        let white_pixel_ref =
            atl.texture(1, 1, 0, 0, Box::new([0xFF, 0xFF, 0xFF, 0xFF])).ok_or("Couldn't pack white_pixel")?;
        let (packers, mut sprites) = atl.into_inner();
        self.white_pixel = sprites[white_pixel_ref.0 as usize].0;

        // update primitive buffers with white pixel
        self.reset_primitive_2d(PrimitiveType::PointList, None);
        self.reset_primitive_3d(PrimitiveType::PointList, None);

        unsafe {
            let textures: Vec<GLuint> = {
                let mut buf = vec![0 as GLuint; packers.len()];
                self.gl.GenTextures(buf.len() as _, buf.as_mut_ptr());
                for (i, (tex_id, packer)) in buf.iter().copied().zip(&packers).enumerate() {
                    let (width, height) = packer.size();

                    self.gl.BindTexture(gl::TEXTURE_2D, tex_id);
                    self.current_atlas = i as u32;

                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as _);
                    self.gl.TexImage2D(
                        gl::TEXTURE_2D,    // target
                        0,                 // level
                        gl::RGBA as _,     // internalformat
                        width as _,        // width
                        height as _,       // height
                        0,                 // border ("must be 0")
                        // GLES has no BGRA; only the allocation happens here, so either works.
                        if cfg!(target_os = "android") { gl::RGBA } else { gl::BGRA },
                        gl::UNSIGNED_BYTE, // type
                        ptr::null(),       // data
                    );
                }
                buf
            };

            // verify it actually worked
            match self.gl.GetError() {
                0 => (),
                err => return Err(format!("Failed to allocate texture on GPU! (OpenGL code {})", err)),
            }

            // upload textures
            // GLES has no BGRA format, so the pixels are swapped to RGBA and uploaded that way.
            if cfg!(target_os = "android") {
                for (_, pixels) in &mut sprites {
                    crate::util::bgra2rgba(pixels);
                }
            }

            // Compose and upload each atlas in a single batch to avoid thousands of small TexSubImage2D calls
            for (atlas_idx, (packer, tex_id)) in packers.iter().zip(&textures).enumerate() {
                let (atlas_w, atlas_h) = packer.size();
                let mut atlas_buf = vec![0u8; (atlas_w * atlas_h * 4) as usize];

                for (atl_ref, pixels) in &sprites {
                    if atl_ref.atlas_id as usize == atlas_idx {
                        let sw = atl_ref.w as usize;
                        let sh = atl_ref.h as usize;
                        let sx = atl_ref.x as usize;
                        let sy = atl_ref.y as usize;
                        let aw = atlas_w as usize;

                        for row in 0..sh {
                            let src_start = row * sw * 4;
                            let src_end = src_start + sw * 4;
                            let dst_start = ((sy + row) * aw + sx) * 4;
                            let dst_end = dst_start + sw * 4;
                            if src_end <= pixels.len() && dst_end <= atlas_buf.len() {
                                atlas_buf[dst_start..dst_end].copy_from_slice(&pixels[src_start..src_end]);
                            }
                        }
                    }
                }

                self.gl.BindTexture(gl::TEXTURE_2D, *tex_id);
                self.current_atlas = atlas_idx as u32;

                self.gl.TexSubImage2D(
                    gl::TEXTURE_2D,       // target
                    0,                    // level
                    0,                    // xoffset
                    0,                    // yoffset
                    atlas_w as _,         // width
                    atlas_h as _,         // height
                    if cfg!(target_os = "android") { gl::RGBA } else { gl::BGRA },
                    gl::UNSIGNED_BYTE,    // type
                    atlas_buf.as_ptr() as _, // pixels
                );
            }

            // verify it actually worked
            match self.gl.GetError() {
                0 => (),
                err => return Err(format!("Failed to upload textures to GPU! (OpenGL code {})", err)),
            }

            // generate framebuffers
            let mut fbo_ids = Vec::with_capacity(textures.len());
            for tex_id in &textures {
                let mut fbo = 0;
                self.gl.GenFramebuffers(1, &mut fbo);
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fbo);
                self.gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, *tex_id, 0);
                fbo_ids.push(Some(fbo));
            }
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.framebuffer.fbo);

            // verify it actually worked
            match self.gl.GetError() {
                0 => (),
                err => return Err(format!("Failed to generate framebuffers! (OpenGL code {})", err)),
            }

            // store opengl texture handles
            self.texture_ids = textures.iter().map(|t| Some(*t)).collect();
            self.zbuf_ids.resize(self.texture_ids.len(), None);
            self.fbo_ids = fbo_ids;
            self.stock_atlas_count = textures.len() as u32 + 2; // TODO remove +2 when -o works
        }

        // store packers, discard pixeldata
        self.atlas_packers = packers;
        self.texture_rects = sprites.drain(..).map(|(ar, _)| Some(ar)).collect();
        self.stock_texture_count = self.texture_rects.len();

        Ok(())
    }

    fn upload_sprite(
        &mut self,
        data: Box<[u8]>,
        width: i32,
        height: i32,
        origin_x: i32,
        origin_y: i32,
    ) -> Result<AtlasRef, String> {
        let atlas_ref = self.create_surface(width, height, false)?;
        if let Some(rect) = self.get_rect_mut(atlas_ref) {
            rect.origin_x = origin_x as f32 / width as f32;
            rect.origin_y = origin_y as f32 / height as f32;
            let rect = self.get_rect(atlas_ref).unwrap();
            unsafe {
                // store previous
                let mut prev_tex2d = 0;
                self.gl.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut prev_tex2d);

                // upload texture
                self.gl.BindTexture(gl::TEXTURE_2D, self.texture_ids[rect.atlas_id as usize].unwrap());
                self.gl.TexSubImage2D(
                    gl::TEXTURE_2D,     // target
                    0,                  // level
                    rect.x as _,        // xoffset
                    rect.y as _,        // yoffset
                    rect.w as _,        // width
                    rect.h as _,        // height
                    gl::RGBA,           // format
                    gl::UNSIGNED_BYTE,  // type
                    data.as_ptr() as _, // pixels
                );

                // verify it actually worked
                match self.gl.GetError() {
                    0 => (),
                    err => return Err(format!("Failed to upload texture to GPU! (OpenGL code {})", err)),
                }

                // cleanup
                self.gl.BindTexture(gl::TEXTURE_2D, prev_tex2d as _);
                assert_eq!(self.gl.GetError(), 0);
            }
        }
        Ok(atlas_ref)
    }

    fn duplicate_sprite(&mut self, atlas_ref: AtlasRef) -> Result<AtlasRef, String> {
        if let Some(rect) = self.get_rect(atlas_ref).cloned() {
            let sprite = self.create_surface(rect.w, rect.h, false)?;
            let new_rect = self.get_rect_mut(sprite).unwrap();
            new_rect.origin_x = rect.origin_x;
            new_rect.origin_y = rect.origin_y;
            let new_rect = self.get_rect(sprite).unwrap();
            unsafe {
                // store previous
                let mut prev_read_fbo = 0;
                self.gl.GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut prev_read_fbo);
                let mut prev_tex2d = 0;
                self.gl.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut prev_tex2d);

                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.fbo_ids[rect.atlas_id as usize].unwrap());
                self.gl.BindTexture(gl::TEXTURE_2D, self.texture_ids[new_rect.atlas_id as usize].unwrap());

                self.gl.CopyTexImage2D(gl::TEXTURE_2D, 0, gl::RGBA, rect.x, rect.y, rect.w as _, rect.h as _, 0);

                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_read_fbo as _);
                self.gl.BindTexture(gl::TEXTURE_2D, prev_tex2d as _);

                // verify it actually worked
                match self.gl.GetError() {
                    0 => (),
                    err => return Err(format!("Failed to duplicate texture! (OpenGL code {})", err)),
                }
            }
            Ok(sprite)
        } else {
            Ok(AtlasRef(-1))
        }
    }

    fn delete_sprite(&mut self, atlas_ref: AtlasRef) {
        // this only deletes sprites created with upload_sprite
        self.flush_queue();
        if let Some(rect) = atlas_ref
            .0
            .try_into()
            .ok()
            .and_then(|id: usize| self.texture_rects.get_mut(id))
            .and_then(|o: &mut Option<AtlasRect>| o.take())
        {
            if rect.atlas_id >= self.stock_atlas_count {
                let tex_id = self.texture_ids[rect.atlas_id as usize].unwrap();
                unsafe {
                    self.gl.DeleteTextures(1, &tex_id);
                    self.texture_ids[rect.atlas_id as usize] = None;
                    if let Some(Some(zbuf)) = self.zbuf_ids.get(rect.atlas_id as usize) {
                        self.gl.DeleteTextures(1, zbuf);
                        self.zbuf_ids[rect.atlas_id as usize] = None;
                    }
                    if let Some(Some(fbo)) = self.fbo_ids.get(rect.atlas_id as usize) {
                        self.gl.DeleteFramebuffers(1, fbo);
                        self.fbo_ids[rect.atlas_id as usize] = None;
                    }
                    assert_eq!(self.gl.GetError(), 0);
                }
            }
        }
    }

    fn set_vsync(&self, vsync: bool) {
        unsafe { self.imp.set_swap_interval(if vsync { 1 } else { 0 }) };
    }

    fn get_vsync(&self) -> bool {
        unsafe { self.imp.get_swap_interval() != 0 }
    }

    fn wait_vsync(&self) {
        unsafe { self.imp.wait_vsync() }
    }

    fn create_sprite_colour(&mut self, width: i32, height: i32, col: Colour) -> Result<AtlasRef, String> {
        let atlas_ref = self.create_surface(width, height, false)?;
        if let Some(rect) = self.get_rect(atlas_ref) {
            unsafe {
                let mut prev_read_fbo = 0;
                self.gl.GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut prev_read_fbo);
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.fbo_ids[rect.atlas_id as usize].unwrap());
                self.gl.ClearColor(col.r as f32, col.g as f32, col.b as f32, 1.0);
                self.gl.Clear(gl::COLOR_BUFFER_BIT);
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_read_fbo as _);
            }
        }
        Ok(atlas_ref)
    }

    fn create_surface(&mut self, width: i32, height: i32, has_zbuffer: bool) -> Result<AtlasRef, String> {
        let atlas_id = if let Some(id) = self.texture_ids.iter().position(|x| x.is_none()) {
            id as u32
        } else {
            self.texture_ids.push(None);
            self.zbuf_ids.push(None);
            self.fbo_ids.push(None);
            self.texture_ids.len() as u32 - 1
        };
        unsafe {
            // store previous
            let mut prev_tex2d = 0;
            self.gl.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut prev_tex2d);
            let mut prev_fbo = 0;
            self.gl.GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut prev_fbo);

            // generate new
            let mut tex_id: GLuint = 0;
            self.gl.GenTextures(1, &mut tex_id);
            self.gl.BindTexture(gl::TEXTURE_2D, tex_id);

            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as _);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as _);

            self.gl.TexImage2D(
                gl::TEXTURE_2D,    // target
                0,                 // level
                gl::RGBA as _,     // internalformat
                width as _,        // width
                height as _,       // height
                0,                 // border ("must be 0")
                gl::RGBA,          // format
                gl::UNSIGNED_BYTE, // type
                ptr::null(),       // data
            );

            // verify it actually worked
            match self.gl.GetError() {
                0 => (),
                err => return Err(format!("Failed to allocate texture on GPU! (OpenGL code {})", err)),
            }

            // generate fbo
            let mut fbo = 0;
            self.gl.GenFramebuffers(1, &mut fbo);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fbo);
            self.gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, tex_id, 0);
            match self.gl.GetError() {
                0 => (),
                err => return Err(format!("Failed to generate framebuffer! (OpenGL code {})", err)),
            }

            // generate zbuffer if applicable
            if has_zbuffer {
                let mut zbuf_id: GLuint = 0;
                self.gl.GenTextures(1, &mut zbuf_id);
                self.gl.BindTexture(gl::TEXTURE_2D, zbuf_id);
                self.gl.TexImage2D(
                    gl::TEXTURE_2D,      // target
                    0,                   // level
                    self.zbuf_format,    // internalformat
                    width as _,          // width
                    height as _,         // height
                    0,                   // border ("must be 0")
                    gl::DEPTH_COMPONENT, // format
                    self.zbuf_type,        // type
                    ptr::null(),         // data
                );
                self.gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::DEPTH_ATTACHMENT, gl::TEXTURE_2D, zbuf_id, 0);
                match self.gl.GetError() {
                    0 => (),
                    err => return Err(format!("Failed to generate depth buffer! (OpenGL code {})", err)),
                }
                // store handle
                self.zbuf_ids[atlas_id as usize] = Some(zbuf_id);
            }

            // store opengl texture handles
            self.texture_ids[atlas_id as usize] = Some(tex_id);
            self.fbo_ids[atlas_id as usize] = Some(fbo);

            // cleanup
            self.gl.BindTexture(gl::TEXTURE_2D, prev_tex2d as _);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_fbo as _);
            assert_eq!(self.gl.GetError(), 0);
        }
        let id = self.texture_rects.len() as i32;
        self.texture_rects.push(Some(AtlasRect {
            atlas_id,
            x: 0,
            y: 0,
            w: width,
            h: height,
            origin_x: 0.0,
            origin_y: 0.0,
        }));
        Ok(AtlasRef(id))
    }

    fn set_target(&mut self, atlas_ref: AtlasRef) {
        self.flush_queue();
        if let Some((rect, Some(Some(fbo_id)))) =
            self.get_rect(atlas_ref).map(|rect| (rect, self.fbo_ids.get(rect.atlas_id as usize)))
        {
            let AtlasRect { x, y, w, h, .. } = *rect;
            unsafe {
                self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, *fbo_id);
                // set viewport here since set_view doesn't
                self.gl.Viewport(x, y, w, h);
                self.gl.Scissor(x, y, w, h);
                assert_eq!(self.gl.GetError(), 0);
            }
            self.set_view(x, y, w, h, 0.0, x, y, w, h);
        }
    }

    fn reset_target(&mut self) {
        self.flush_queue();
        unsafe {
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffer.fbo);

            // reset view
            let fb_width = self.framebuffer.width as i32;
            let fb_height = self.framebuffer.height as i32;
            self.set_view(0, 0, fb_width, fb_height, 0.0, 0, 0, fb_width, fb_height);
        }
    }

    fn copy_surface(
        &mut self,
        dest: AtlasRef,
        mut dest_x: i32,
        mut dest_y: i32,
        src: AtlasRef,
        mut src_x: i32,
        mut src_y: i32,
        mut width: i32,
        mut height: i32,
    ) {
        let (src_rect, dest_rect) = match (self.get_rect(src), self.get_rect(dest)) {
            (Some(src), Some(dest)) => (src, dest),
            _ => return,
        };
        // correct coordinates
        if src_x < 0 {
            dest_x -= src_x;
            width += src_x;
            src_x = 0;
        }
        if src_y < 0 {
            dest_y -= src_y;
            height += src_y;
            src_y = 0;
        }
        if src_x + width > src_rect.w {
            width = src_rect.w - src_x;
        }
        if src_y + height > src_rect.h {
            height = dest_rect.h - src_y;
        }
        if dest_x < 0 {
            src_x -= dest_x;
            width += dest_x;
            dest_x = 0;
        }
        if dest_y < 0 {
            src_y -= dest_y;
            height += dest_y;
            dest_y = 0;
        }
        if dest_x + width > dest_rect.w {
            width = dest_rect.w - dest_x;
        }
        if dest_y + height > dest_rect.h {
            height = dest_rect.h - dest_y;
        }
        if width > 0 && height > 0 {
            // actual copy
            if let (Some(Some(src_id)), Some(Some(dst_id))) =
                (self.fbo_ids.get(src_rect.atlas_id as usize), self.fbo_ids.get(dest_rect.atlas_id as usize))
            {
                unsafe {
                    // On Intel, glBlitFrameBuffer just does nothing if the scissor box is too big, which it
                    // very well could be. So just disable the scissor test for now.
                    self.gl.Disable(gl::SCISSOR_TEST);

                    // Remember old framebuffer so we can rebind it after we're done
                    let mut fb_old = 0;
                    self.gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut fb_old);
                    assert_eq!(self.gl.GetError(), 0);

                    self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, *dst_id);
                    self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, *src_id);
                    self.gl.BlitFramebuffer(
                        src_x,
                        src_y,
                        src_x + width,
                        src_y + height,
                        dest_x,
                        dest_y,
                        dest_x + width,
                        dest_y + height,
                        gl::COLOR_BUFFER_BIT,
                        gl::NEAREST,
                    );
                    self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, fb_old as u32);

                    self.gl.Enable(gl::SCISSOR_TEST);

                    assert_eq!(self.gl.GetError(), 0);
                }
            }
        }
    }

    fn set_zbuf_trashed(&mut self, trashed: bool) {
        if trashed != self.zbuf_trashed {
            self.flush_queue();
            self.zbuf_trashed = trashed;
            unsafe {
                let mut prev_fbo = 0;
                self.gl.GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut prev_fbo);
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.framebuffer.fbo);
                assert_eq!(self.gl.GetError(), 0);
                self.gl.FramebufferTexture2D(
                    gl::READ_FRAMEBUFFER,
                    gl::DEPTH_ATTACHMENT,
                    gl::TEXTURE_2D,
                    if trashed { 0 } else { self.framebuffer.zbuf },
                    0,
                );
                assert_eq!(self.gl.GetError(), 0);
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_fbo as _);
            }
        }
    }

    fn get_zbuf_trashed(&self) -> bool {
        self.zbuf_trashed
    }

    fn resize_framebuffer(&mut self, width: u32, height: u32, store: bool) {
        self.flush_queue();
        unsafe {
            // get old size
            let old_tex = self.framebuffer.texture;
            let old_width = self.framebuffer.width as i32;
            let old_height = self.framebuffer.height as i32;
            self.framebuffer.width = width;
            self.framebuffer.height = height;
            // set up new texture
            self.gl.GenTextures(1, &mut self.framebuffer.texture);
            self.gl.BindTexture(gl::TEXTURE_2D, self.framebuffer.texture);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);
            self.gl.TexImage2D(
                gl::TEXTURE_2D,    // target
                0,                 // level
                gl::RGBA as _,     // internalformat
                width as _,        // width
                height as _,       // height
                0,                 // border ("must be 0")
                gl::RGBA,          // format
                gl::UNSIGNED_BYTE, // type
                ptr::null(),       // data
            );
            assert_eq!(self.gl.GetError(), 0);
            // set up new zbuffer
            let old_zbuf = self.framebuffer.zbuf;
            self.gl.GenTextures(1, &mut self.framebuffer.zbuf);
            self.gl.BindTexture(gl::TEXTURE_2D, self.framebuffer.zbuf);
            self.gl.TexImage2D(
                gl::TEXTURE_2D,      // target
                0,                   // level
                self.zbuf_format,    // internalformat
                width as _,          // width
                height as _,         // height
                0,                   // border ("must be 0")
                gl::DEPTH_COMPONENT, // format
                self.zbuf_type,        // type
                ptr::null(),         // data
            );
            assert_eq!(self.gl.GetError(), 0);
            // set up new fbo
            let old_fbo = self.framebuffer.fbo;
            self.gl.GenFramebuffers(1, &mut self.framebuffer.fbo);
            self.gl.BindFramebuffer(gl::FRAMEBUFFER, self.framebuffer.fbo);
            self.gl.FramebufferTexture2D(
                gl::READ_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                self.framebuffer.texture,
                0,
            );
            self.gl.FramebufferTexture2D(
                gl::READ_FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::TEXTURE_2D,
                self.framebuffer.zbuf,
                0,
            );
            assert_eq!(self.gl.GetError(), 0);
            // draw old fb onto new
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, old_fbo);
            let copy_width = width.min(old_width as _);
            let copy_height = height.min(old_height as _);
            self.gl.BlitFramebuffer(
                0,
                0,
                copy_width as _,
                copy_height as _,
                0,
                0,
                copy_width as _,
                copy_height as _,
                gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT,
                gl::NEAREST,
            );
            assert_eq!(self.gl.GetError(), 0);
            if store {
                // store old texture and fbo, delete previous stored one
                if let Some(framebuffer) = &self.stored_framebuffer {
                    self.gl.DeleteTextures(1, &framebuffer.texture);
                    self.gl.DeleteTextures(1, &framebuffer.zbuf);
                    self.gl.DeleteFramebuffers(1, &framebuffer.fbo);
                }
                self.stored_framebuffer = Some(Framebuffer {
                    texture: old_tex,
                    zbuf: old_zbuf,
                    fbo: old_fbo,
                    width: old_width as u32,
                    height: old_height as u32,
                });
            } else {
                // delete old texture and fbo
                self.gl.DeleteTextures(1, &old_tex);
                self.gl.DeleteTextures(1, &old_zbuf);
                self.gl.DeleteFramebuffers(1, &old_fbo);
            }
            assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn get_texture_id(&mut self, atl_ref: AtlasRef) -> i32 {
        atl_ref.0
    }

    fn get_texture_from_id(&self, id: i32) -> Option<AtlasRef> {
        Some(AtlasRef(id))
    }

    fn get_texture_rects(&self) -> Vec<Option<AtlasRect>> {
        self.texture_rects[self.stock_texture_count..].to_vec()
    }

    fn set_texture_rects(&mut self, rects: &[Option<AtlasRect>]) {
        self.texture_rects.truncate(self.stock_texture_count);
        self.texture_rects.extend_from_slice(rects);
    }

    fn dump_sprite_part(&self, atlas_ref: AtlasRef, part_x: i32, part_y: i32, part_w: i32, part_h: i32) -> Box<[u8]> {
        let rect = match self.get_rect(atlas_ref) {
            Some(rect) => {
                AtlasRect { x: rect.x + part_x, y: rect.y + part_y, w: part_w, h: part_h, ..*rect }
            },
            None => return Box::new([]),
        };
        unsafe {
            // store read fbo
            let mut prev_read_fbo: GLint = 0;
            self.gl.GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut prev_read_fbo);

            // bind texture fbo (the atlas may have been recreated during a scene change)
            let Some(fbo) = self.fbo_ids.get(rect.atlas_id as usize).and_then(|fbo| *fbo) else {
                return Box::new([])
            };
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fbo);

            // An incomplete framebuffer makes ReadPixels undefined and can stall the driver.
            if self.gl.CheckFramebufferStatus(gl::READ_FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_read_fbo as GLuint);
                return Box::new([])
            }

            // read data
            let len = (rect.w * rect.h * 4) as usize;
            let mut data: Vec<u8> = Vec::with_capacity(len);
            data.set_len(len);
            self.gl.ReadPixels(rect.x, rect.y, rect.w, rect.h, gl::RGBA, gl::UNSIGNED_BYTE, data.as_mut_ptr().cast());

            // A GL error here used to panic the game thread (the screen froze). The caller handles
            // an empty buffer instead, and the error is left for the next frame to reset.
            let _ = self.gl.GetError();

            // cleanup
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, prev_read_fbo as GLuint);
            let _ = self.gl.GetError();

            data.into_boxed_slice()
        }
    }

    fn get_pixels(&self, x: i32, y: i32, w: i32, h: i32) -> Box<[u8]> {
        unsafe {
            let len = (w * h * 4) as usize;
            let mut data: Vec<u8> = Vec::with_capacity(len);
            data.set_len(len);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.framebuffer.fbo);
            self.gl.ReadPixels(x, y, w, h, gl::RGBA, gl::UNSIGNED_BYTE, data.as_mut_ptr().cast());
            assert_eq!(self.gl.GetError(), 0);
            data.into_boxed_slice()
        }
    }

    fn stored_pixels(&self) -> Box<[u8]> {
        let fb = self.stored_framebuffer.unwrap_or(self.framebuffer);
        unsafe {
            let (width, height) = self.stored_size();
            let len = (width * height * 4) as usize;
            let mut data: Vec<u8> = Vec::with_capacity(len);
            data.set_len(len);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fb.fbo);
            self.gl.ReadPixels(0, 0, width as _, height as _, gl::RGBA, gl::UNSIGNED_BYTE, data.as_mut_ptr().cast());
            assert_eq!(self.gl.GetError(), 0);
            data.into_boxed_slice()
        }
    }

    fn stored_zbuffer(&self) -> Box<[f32]> {
        let fb = self.stored_framebuffer.unwrap_or(self.framebuffer);
        unsafe {
            self.gl.BindTexture(gl::TEXTURE_2D, fb.zbuf);
            let width = fb.width as i32;
            let height = fb.height as i32;
            let len = (width * height) as usize;
            let mut data: Vec<f32> = Vec::with_capacity(len);
            data.set_len(len);
            self.gl.GetTexImage(gl::TEXTURE_2D, 0, gl::DEPTH_COMPONENT, gl::FLOAT, data.as_mut_ptr().cast());
            data.into_boxed_slice()
        }
    }

    fn set_stored(&mut self, rgba: Box<[u8]>, zbuf: Box<[f32]>, fb_w: u32, fb_h: u32) {
        unsafe {
            let mut fbo = 0;
            let mut texture = 0;
            let mut zbuffer = 0;
            self.gl.GenFramebuffers(1, &mut fbo);
            self.gl.GenTextures(1, &mut texture);
            self.gl.GenTextures(1, &mut zbuffer);

            assert_eq!(self.gl.GetError(), 0);

            self.gl.BindTexture(gl::TEXTURE_2D, texture);
            assert_eq!(self.gl.GetError(), 0);
            self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);
            assert_eq!(self.gl.GetError(), 0);
            self.gl.TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as _,
                fb_w as _,
                fb_h as _,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                rgba.as_ptr().cast(),
            );

            assert_eq!(self.gl.GetError(), 0);

            self.gl.BindTexture(gl::TEXTURE_2D, zbuffer);
            self.gl.TexImage2D(
                gl::TEXTURE_2D,
                0,
                self.zbuf_format,
                fb_w as _,
                fb_h as _,
                0,
                gl::DEPTH_COMPONENT,
                gl::FLOAT,
                zbuf.as_ptr().cast(),
            );

            assert_eq!(self.gl.GetError(), 0);

            self.gl.BindFramebuffer(gl::FRAMEBUFFER, fbo);
            self.gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, texture, 0);
            assert_eq!(self.gl.GetError(), 0);
            self.gl.FramebufferTexture2D(gl::READ_FRAMEBUFFER, gl::DEPTH_ATTACHMENT, gl::TEXTURE_2D, zbuffer, 0);

            if let Some(framebuffer) = &self.stored_framebuffer {
                self.gl.DeleteTextures(1, &framebuffer.texture);
                self.gl.DeleteTextures(1, &framebuffer.zbuf);
                self.gl.DeleteFramebuffers(1, &framebuffer.fbo);
            }

            self.stored_framebuffer = Some(Framebuffer {
                fbo,
                texture,
                zbuf: zbuffer,
                width: fb_w,
                height: fb_h,
            });

            assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn dump_dynamic_textures(&self) -> Vec<Option<SavedTexture>> {
        unsafe {
            // store previous
            let mut prev_tex2d = 0;
            self.gl.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut prev_tex2d);

            let mut textures = Vec::with_capacity(self.texture_ids.len() - self.stock_atlas_count as usize);
            for (tex_id, zbuf_id) in self
                .texture_ids
                .iter()
                .copied()
                .zip(self.zbuf_ids.iter().copied())
                .skip(self.stock_atlas_count as usize)
            {
                textures.push(match tex_id {
                    Some(tex_id) => {
                        self.gl.BindTexture(gl::TEXTURE_2D, tex_id);
                        let mut width = 0;
                        let mut height = 0;
                        self.gl.GetTexLevelParameteriv(gl::TEXTURE_2D, 0, gl::TEXTURE_WIDTH, &mut width);
                        self.gl.GetTexLevelParameteriv(gl::TEXTURE_2D, 0, gl::TEXTURE_HEIGHT, &mut height);
                        let len = (width * height) as usize;
                        let mut pixels: Vec<u8> = Vec::with_capacity(len * 4);
                        pixels.set_len(len * 4);
                        self.gl.GetTexImage(gl::TEXTURE_2D, 0, gl::RGBA, gl::UNSIGNED_BYTE, pixels.as_mut_ptr().cast());
                        let zbuf = if let Some(zbuf_id) = zbuf_id {
                            self.gl.BindTexture(gl::TEXTURE_2D, zbuf_id);
                            let mut zbuf: Vec<f32> = Vec::with_capacity(len);
                            zbuf.set_len(len);
                            self.gl.GetTexImage(
                                gl::TEXTURE_2D,
                                0,
                                gl::DEPTH_COMPONENT,
                                self.zbuf_type,
                                zbuf.as_mut_ptr().cast(),
                            );
                            Some(zbuf.into_boxed_slice())
                        } else {
                            None
                        };
                        Some(SavedTexture { width, height, pixels: pixels.into_boxed_slice(), zbuf })
                    },
                    None => None,
                });
            }

            self.gl.BindTexture(gl::TEXTURE_2D, prev_tex2d as _);
            assert_eq!(self.gl.GetError(), 0);

            textures
        }
    }

    fn upload_dynamic_textures(&mut self, textures: &[Option<SavedTexture>]) {
        unsafe {
            for tex_id in self.texture_ids.iter_mut().skip(self.stock_atlas_count as usize) {
                if let Some(tex_id) = tex_id.as_ref() {
                    self.gl.DeleteTextures(1, tex_id);
                }
                *tex_id = None;
            }
            self.texture_ids.resize(self.stock_atlas_count as usize + textures.len(), None);
            for fbo_id in self.fbo_ids.iter_mut().skip(self.stock_atlas_count as usize) {
                if let Some(fbo_id) = fbo_id.as_ref() {
                    self.gl.DeleteFramebuffers(1, fbo_id);
                }
                *fbo_id = None;
            }
            self.fbo_ids.resize(self.stock_atlas_count as usize + textures.len(), None);
            for tex_id in self.zbuf_ids.iter_mut().skip(self.stock_atlas_count as usize) {
                if let Some(tex_id) = tex_id.as_ref() {
                    self.gl.DeleteTextures(1, tex_id);
                }
                *tex_id = None;
            }
            self.zbuf_ids.resize(self.stock_atlas_count as usize + textures.len(), None);
            for (i, tex) in textures.iter().enumerate() {
                let i = i + self.stock_atlas_count as usize;
                if let Some(tex) = tex.as_ref() {
                    let mut tex_id = 0;
                    self.gl.GenTextures(1, &mut tex_id);
                    self.gl.BindTexture(gl::TEXTURE_2D, tex_id);

                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as _);
                    self.gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as _);

                    self.gl.TexImage2D(
                        gl::TEXTURE_2D,
                        0,
                        gl::RGBA as _,
                        tex.width,
                        tex.height,
                        0,
                        gl::RGBA,
                        gl::UNSIGNED_BYTE,
                        tex.pixels.as_ptr().cast(),
                    );
                    self.texture_ids[i] = Some(tex_id);

                    let mut fbo_id = 0;
                    self.gl.GenFramebuffers(1, &mut fbo_id);
                    self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fbo_id);
                    self.gl.FramebufferTexture2D(
                        gl::READ_FRAMEBUFFER,
                        gl::COLOR_ATTACHMENT0,
                        gl::TEXTURE_2D,
                        tex_id,
                        0,
                    );
                    self.fbo_ids[i] = Some(fbo_id);

                    if let Some(zbuf) = tex.zbuf.as_ref() {
                        let mut zbuf_id = 0;
                        self.gl.GenTextures(1, &mut zbuf_id);
                        self.gl.BindTexture(gl::TEXTURE_2D, zbuf_id);
                        self.gl.TexImage2D(
                            gl::TEXTURE_2D,
                            0,
                            self.zbuf_format,
                            tex.width,
                            tex.height,
                            0,
                            gl::DEPTH_COMPONENT,
                            gl::FLOAT,
                            zbuf.as_ptr().cast(),
                        );
                        self.zbuf_ids[i] = Some(zbuf_id);
                        self.gl.FramebufferTexture2D(
                            gl::READ_FRAMEBUFFER,
                            gl::DEPTH_ATTACHMENT,
                            gl::TEXTURE_2D,
                            zbuf_id,
                            0,
                        );
                    }
                }
            }
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.framebuffer.fbo);
            assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn get_rect(&self, id: AtlasRef) -> Option<&AtlasRect> {
        id.0.try_into()
            .ok()
            .and_then(|id: usize| self.texture_rects.get(id))
            .and_then(|o: &Option<AtlasRect>| o.as_ref())
    }

    fn set_sprite_origin(&mut self, id: AtlasRef, origin_x: f32, origin_y: f32) {
        if let Some(rect) = self.get_rect_mut(id) {
            rect.origin_x = origin_x / rect.w as f32;
            rect.origin_y = origin_y / rect.h as f32;
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
    ) {
        let atlas_ref = match self.get_rect(texture) {
            Some(rect) => *rect,
            None => return,
        };

        self.set_texture_repeat(false);

        // get angle
        let angle = -angle.to_radians();
        let angle_sin = angle.sin();
        let angle_cos = angle.cos();

        // get real width of drawn sprite
        let width: f64 = xscale * f64::from(part_w);
        let height: f64 = yscale * f64::from(part_h);
        // calculate pre-rotation corner offsets from sprite origin
        // incl. subtraction 0.5 from left and top (GM does this in an attempt to combat the DX half-pixel offset)
        let (left, top): (f64, f64) = if use_origin {
            (-width * f64::from(atlas_ref.origin_x) - 0.5, -height * f64::from(atlas_ref.origin_y) - 0.5)
        } else {
            (-0.5, -0.5)
        };
        let right: f64 = left + width;
        let bottom: f64 = top + height;

        // get texture corners.
        // A negative part_w/part_h means the part is mirrored: its size is still |part_w|/|part_h|,
        // but using the signed value put the UVs *outside* the sprite's atlas cell (0 .. -1), so the
        // draw sampled whatever sat next to it in the atlas. The flip itself is already handled by
        // the destination geometry (width/height carry the scale sign), so take the source extent as
        // positive and just swap the corners each vertex samples.
        let tex_x0 = f64::from(part_x) / f64::from(atlas_ref.w);
        let tex_y0 = f64::from(part_y) / f64::from(atlas_ref.h);
        let tex_x1 = tex_x0 + f64::from(part_w.abs()) / f64::from(atlas_ref.w);
        let tex_y1 = tex_y0 + f64::from(part_h.abs()) / f64::from(atlas_ref.h);
        let (tex_left, tex_right) = if part_w < 0.0 { (tex_x1, tex_x0) } else { (tex_x0, tex_x1) };
        let (tex_top, tex_bottom) = if part_h < 0.0 { (tex_y1, tex_y0) } else { (tex_y0, tex_y1) };

        let (tex_left, tex_top, tex_right, tex_bottom) =
            (tex_left as f32, tex_top as f32, tex_right as f32, tex_bottom as f32);

        let normal = [0.0, 0.0, 0.0];
        let depth = self.depth;

        // rotate around draw origin
        let rotate = |xoff, yoff| {
            [(x + xoff * angle_cos - yoff * angle_sin) as f32, (y + yoff * angle_cos + xoff * angle_sin) as f32, depth]
        };

        // push the vertices
        self.push_primitive(
            PrimitiveBuilder::new(atlas_ref, PrimitiveType::TriFan, true)
                .push_vertex(rotate(left, top), [tex_left, tex_top], split_colour(col1, alpha), normal)
                .push_vertex(rotate(right, top), [tex_right, tex_top], split_colour(col2, alpha), normal)
                .push_vertex(rotate(right, bottom), [tex_right, tex_bottom], split_colour(col3, alpha), normal)
                .push_vertex(rotate(left, bottom), [tex_left, tex_bottom], split_colour(col4, alpha), normal),
        );
    }

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
        alpha: f64,
    ) {
        let atlas_ref = match self.get_rect(texture) {
            Some(rect) => *rect,
            None => return,
        };

        self.set_texture_repeat(false);

        // get texture corners
        let tex_left = 0.0;
        let tex_top = 0.0;
        let tex_right = tex_left + 1.0;
        let tex_bottom = tex_top + 1.0;

        let (tex_left, tex_top, tex_right, tex_bottom) =
            (tex_left as f32, tex_top as f32, tex_right as f32, tex_bottom as f32);

        let normal = [0.0, 0.0, 0.0];
        let depth = self.depth;

        // correct for gm offset
        let correct = |xoff, yoff| {
            [(xoff-0.5) as f32, (yoff-0.5) as f32, depth]
        };

        // push the vertices
        self.push_primitive(
            PrimitiveBuilder::new(atlas_ref, PrimitiveType::TriFan, true)
                .push_vertex(correct(x1, y1), [tex_left, tex_top], split_colour(0xffffff, alpha), normal)
                .push_vertex(correct(x2, y2), [tex_right, tex_top], split_colour(0xffffff, alpha), normal)
                .push_vertex(correct(x3, y3), [tex_right, tex_bottom], split_colour(0xffffff, alpha), normal)
                .push_vertex(correct(x4, y4), [tex_left, tex_bottom], split_colour(0xffffff, alpha), normal),
        );
    }

    fn draw_rectangle(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64) {
        let (x1, x2) = if x2 < x1 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y2 < y1 { (y2, y1) } else { (y1, y2) };
        let x2 = if x2 == x2.floor() { x2 + 0.01 } else { x2 };
        let y2 = if y2 == y2.floor() { y2 + 0.01 } else { y2 };
        self.push_primitive(
            ShapeBuilder::new(false, self.white_pixel, alpha, self.depth)
                .push_point(x1, y1, colour)
                .push_point(x2, y1, colour)
                .push_point(x2, y2, colour)
                .push_point(x1, y2, colour)
                .build(),
        );
    }

    fn draw_rectangle_outline(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, colour: i32, alpha: f64) {
        let (x1, x2) = if x2 < x1 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y2 < y1 { (y2, y1) } else { (y1, y2) };
        let x2 = if x2 == x2.floor() { x2 + 0.01 } else { x2 };
        let y2 = if y2 == y2.floor() { y2 + 0.01 } else { y2 };
        self.push_primitive(
            ShapeBuilder::new(true, self.white_pixel, alpha, self.depth)
                .push_point(x1, y1, colour)
                .push_point(x2, y1, colour)
                .push_point(x2, y2, colour)
                .push_point(x1, y2, colour)
                .build(),
        );
    }

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
    ) {
        let (x1, x2) = if x2 < x1 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y2 < y1 { (y2, y1) } else { (y1, y2) };
        let x2 = if x2 == x2.floor() { x2 + 0.01 } else { x2 };
        let y2 = if y2 == y2.floor() { y2 + 0.01 } else { y2 };
        self.push_primitive(
            ShapeBuilder::new(outline, self.white_pixel, alpha, self.depth)
                .push_point(x1, y1, c1)
                .push_point(x2, y1, c2)
                .push_point(x2, y2, c3)
                .push_point(x1, y2, c4)
                .build(),
        );
    }

    fn draw_point(&mut self, x: f64, y: f64, colour: i32, alpha: f64) {
        self.setup_queue(self.white_pixel.atlas_id, PrimitiveShape::Point);
        self.vertex_queue.push(Vertex {
            pos: [x as f32, y as f32, self.depth],
            tex_coord: [f32::NAN; 2],
            blend: split_colour(colour, alpha),
            atlas_xywh: self.white_pixel.into(),
            normal: [0.0, 0.0, 0.0],
        });
    }

    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, width: Option<f64>, c1: i32, c2: i32, alpha: f64) {
        if let Some(width) = width {
            let length = (x2 - x1).hypot(y2 - y1);
            // on the off chance that they're in different points but the length is still somehow 0, check length
            if length != 0.0 {
                // calculate corners
                let width_x = (y2 - y1) * (width / 2.0) / length;
                let width_y = (x2 - x1) * (width / 2.0) / length;
                // actually push the rectangle
                self.push_primitive(
                    ShapeBuilder::new(false, self.white_pixel, alpha, self.depth)
                        .push_point(x1 - width_x, y1 + width_y, c1)
                        .push_point(x1 + width_x, y1 - width_y, c1)
                        .push_point(x2 + width_x, y2 - width_y, c2)
                        .push_point(x2 - width_x, y2 + width_y, c2)
                        .build(),
                );
            }
        } else {
            self.push_primitive(
                ShapeBuilder::new(true, self.white_pixel, alpha, self.depth)
                    .push_point(x1, y1, c1)
                    .push_point(x2, y2, c2)
                    .build(),
            );
        }
    }

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
    ) {
        self.push_primitive(
            ShapeBuilder::new(outline, self.white_pixel, alpha, self.depth)
                .push_point(x1, y1, c1)
                .push_point(x2, y2, c2)
                .push_point(x3, y3, c3)
                .build(),
        );
    }

    fn draw_ellipse(&mut self, x: f64, y: f64, rad_x: f64, rad_y: f64, c1: i32, c2: i32, alpha: f64, outline: bool) {
        let mut builder = ShapeBuilder::new(outline, self.white_pixel, alpha, self.depth);
        if !outline {
            builder.push_point(x, y, c1);
        }
        for i in 0..=self.circle_precision {
            let angle = f64::from(i) * 2.0 * PI / f64::from(self.circle_precision);
            builder.push_point(x + rad_x * angle.cos(), y + rad_y * angle.sin(), c2);
        }
        self.push_primitive(builder.build());
    }

    fn draw_roundrect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, c1: i32, c2: i32, alpha: f64, outline: bool) {
        let x2 = if x2 == x2.floor() { x2 + 0.01 } else { x2 };
        let y2 = if y2 == y2.floor() { y2 + 0.01 } else { y2 };
        let xcenter = (x1 + x2) / 2.0;
        let ycenter = (y1 + y2) / 2.0;
        let width = (x2 - x1).abs();
        let height = (y2 - y1).abs();
        let rad_x = width.min(10.0) / 2.0;
        let rad_y = height.min(10.0) / 2.0;
        let rect_half_w = (width / 2.0 - rad_x).max(0.0);
        let rect_half_h = (height / 2.0 - rad_y).max(0.0);
        let mut builder = ShapeBuilder::new(outline, self.white_pixel, alpha, self.depth);
        if !outline {
            builder.push_point(xcenter, ycenter, c1);
        }
        let quarter_circle = self.circle_precision / 4;
        for quad in 0..4 {
            let circle_x = xcenter + if quad == 0 || quad == 3 { rect_half_w } else { -rect_half_w };
            let circle_y = ycenter + if quad < 2 { rect_half_h } else { -rect_half_h };
            for i in quarter_circle * quad..=quarter_circle * (quad + 1) {
                let angle = f64::from(i) * 2.0 * PI / f64::from(self.circle_precision);
                builder.push_point(circle_x + rad_x * angle.cos(), circle_y + rad_y * angle.sin(), c2);
            }
        }
        self.push_primitive(builder.push_point(xcenter + rect_half_w + rad_x, ycenter + rect_half_h, c2).build());
    }

    fn set_circle_precision(&mut self, prec: i32) {
        self.circle_precision = (prec.max(4).min(64) >> 2) << 2;
    }

    fn get_circle_precision(&self) -> i32 {
        self.circle_precision
    }

    fn reset_primitive_2d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>) {
        let ar = atlas_ref.and_then(|ar| self.get_rect(ar).copied());
        self.primitive_2d = PrimitiveBuilder::new(
            ar.unwrap_or(self.white_pixel),
            ptype,
            ar.is_some(),
        );
    }

    fn vertex_2d(&mut self, x: f64, y: f64, xtex: f64, ytex: f64, col: i32, alpha: f64) {
        self.primitive_2d.push_vertex(
            [x as f32, y as f32, self.depth],
            [xtex as f32, ytex as f32],
            split_colour(col, alpha),
            [0.0, 0.0, 0.0],
        );
    }

    fn draw_primitive_2d(&mut self) {
        // I would use push_primitive but that causes borrowing issues.
        self.setup_queue(self.primitive_2d.get_atlas_id(), self.primitive_2d.get_shape());
        self.vertex_queue.extend_from_slice(self.primitive_2d.get_vertices());
    }

    fn get_primitive_2d(&self) -> PrimitiveBuilder {
        self.primitive_2d.clone()
    }

    fn set_primitive_2d(&mut self, prim: PrimitiveBuilder) {
        self.primitive_2d = prim;
    }

    fn reset_primitive_3d(&mut self, ptype: PrimitiveType, atlas_ref: Option<AtlasRef>) {
        let ar = atlas_ref.and_then(|ar| self.get_rect(ar).copied());
        self.primitive_3d = PrimitiveBuilder::new(
            ar.unwrap_or(self.white_pixel),
            ptype,
            ar.is_some(),
        );
    }

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
    ) {
        self.primitive_3d.push_vertex(
            [x as f32, y as f32, z as f32],
            [xtex as f32, ytex as f32],
            split_colour(col, alpha),
            [nx as f32, ny as f32, nz as f32],
        );
    }

    fn draw_primitive_3d(&mut self) {
        // See draw_primitive_2d.
        self.setup_queue(self.primitive_3d.get_atlas_id(), self.primitive_3d.get_shape());
        self.vertex_queue.extend_from_slice(self.primitive_3d.get_vertices());
    }

    fn get_primitive_3d(&self) -> PrimitiveBuilder {
        self.primitive_3d.clone()
    }

    fn set_primitive_3d(&mut self, prim: PrimitiveBuilder) {
        self.primitive_3d = prim;
    }

    fn extend_buffers(&self, buf: &mut VertexBuffer) {
        let verts = self.primitive_3d.get_vertices();
        match self.primitive_3d.get_shape() {
            PrimitiveShape::Point => buf.points.extend_from_slice(verts),
            PrimitiveShape::Line => buf.lines.extend_from_slice(&verts[..verts.len() / 2 * 2]),
            PrimitiveShape::Triangle => buf.tris.extend_from_slice(&verts[..verts.len() / 3 * 3]),
        }
    }

    fn draw_buffers(&mut self, atlas_ref: Option<AtlasRef>, buf: &VertexBuffer) {
        // TODO: bench this method vs copying the buffer onto the draw queue
        self.flush_queue();
        self.update_render_state();
        self.draw_buffer(
            atlas_ref.and_then(|ar| self.get_rect(ar).copied()).unwrap_or(self.white_pixel).atlas_id,
            PrimitiveShape::Point,
            &buf.points,
        );
        self.draw_buffer(
            atlas_ref.and_then(|ar| self.get_rect(ar).copied()).unwrap_or(self.white_pixel).atlas_id,
            PrimitiveShape::Line,
            &buf.lines,
        );
        self.draw_buffer(
            atlas_ref.and_then(|ar| self.get_rect(ar).copied()).unwrap_or(self.white_pixel).atlas_id,
            PrimitiveShape::Triangle,
            &buf.tris,
        );
    }

    fn get_alpha_blending(&self) -> bool {
        self.next_render_state.alpha_blending
    }

    fn set_alpha_blending(&mut self, alphablend: bool) {
        self.next_render_state.alpha_blending = alphablend;
        self.render_state_updated = true;
    }

    fn get_colour_blending(&self) -> bool {
        self.next_render_state.texture_blend.into()
    }

    fn set_colour_blending(&mut self, modulate: bool) {
        self.next_render_state.texture_blend = modulate.into();
        self.render_state_updated = true;
    }

    fn get_blend_mode(&self) -> (BlendType, BlendType) {
        self.next_render_state.blend_mode
    }

    fn set_blend_mode(&mut self, src: BlendType, dst: BlendType) {
        self.next_render_state.blend_mode = (src, dst);
        self.render_state_updated = true;
    }

    fn get_pixel_interpolation(&self) -> bool {
        self.next_render_state.interpolate_pixels.into()
    }

    fn set_pixel_interpolation(&mut self, lerping: bool) {
        // in DX (and therefore GM) this is set per texture unit, but in GL it's per texture
        // therefore, we need to apply the setting before every draw call
        self.next_render_state.interpolate_pixels = lerping.into();
        self.render_state_updated = true;
    }

    fn get_texture_repeat(&self) -> bool {
        self.next_render_state.texture_repeat.into()
    }

    fn set_texture_repeat(&mut self, repeat: bool) {
        self.next_render_state.texture_repeat = repeat.into();
        self.render_state_updated = true;
    }

    /// Does anything that's queued to be done.
    fn flush_queue(&mut self) {
        // move the queue out of self to satisfy the borrow checker
        let mut queue = std::mem::take(&mut self.vertex_queue);
        self.draw_buffer(self.current_atlas, self.queue_type, &queue);
        // clear it and put it back so we can reuse the memory
        queue.clear();
        self.vertex_queue = queue;
    }

    fn set_view_matrix(&mut self, view: [f32; 16]) {
        self.next_render_state.view_matrix = view;
        self.render_state_updated = true;
    }

    fn set_viewproj_matrix(&mut self, view: [f32; 16], proj: [f32; 16]) {
        self.next_render_state.view_matrix = view;
        self.next_render_state.proj_matrix = proj;
        self.render_state_updated = true;
    }

    fn get_model_matrix(&self) -> [f32; 16] {
        self.next_render_state.model_matrix
    }

    fn set_model_matrix(&mut self, model: [f32; 16]) {
        self.next_render_state.model_matrix = model;
        self.render_state_updated = true;
    }

    fn mult_model_matrix(&mut self, model: [f32; 16]) {
        self.next_render_state.model_matrix = mat4mult(self.next_render_state.model_matrix, model);
        self.render_state_updated = true;
    }

    fn set_projection_ortho(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64) {
        #[rustfmt::skip]
        let proj_matrix: [f32; 16] = {
            // Squish to screen, flip vertically, and constrain z to range 1 - 32000
            [
                2.0 / w as f32, 0.0,             0.0,            0.0,
                0.0,            -2.0 / h as f32, 0.0,            0.0,
                0.0,            0.0,             1.0 / 31999.0,  0.0,
                0.0,            0.0,             -1.0 / 31999.0, 1.0,
            ]
        };

        self.set_viewproj_matrix(make_view_matrix(x, y, -16000.0, w, h, angle), proj_matrix);
    }

    fn set_projection_perspective(&mut self, x: f64, y: f64, w: f64, h: f64, angle: f64) {
        #[rustfmt::skip]
        let proj_matrix: [f32; 16] = {
            // Squish to screen, flip vertically, and constrain z to range 1 - 32000
            [
                2.0, 0.0,                  0.0,                0.0,
                0.0, 2.0 * (w / h) as f32, 0.0,                0.0,
                0.0, 0.0,                  32000.0 / 31999.0,  1.0,
                0.0, 0.0,                  -32000.0 / 31999.0, 0.0,
            ]
        };

        self.set_viewproj_matrix(make_view_matrix(x, y, -w, w, h, angle), proj_matrix);
    }

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
    ) {
        self.flush_queue();
        // DX8's viewport function doesn't do anything if a surface is set as the draw target, so emulate that
        let mut fb_current = 0;
        unsafe {
            self.gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut fb_current);
            debug_assert_eq!(self.gl.GetError(), 0);
        }
        if fb_current == self.framebuffer.fbo as _ {
            // Set viewport (gl::Viewport, gl::Scissor)
            if port_x >= 0 && port_y >= 0 && port_w >= 0 && port_h >= 0 {
                unsafe {
                    self.gl.Viewport(port_x, port_y, port_w, port_h);
                    self.gl.Scissor(port_x, port_y, port_w, port_h);
                    debug_assert_eq!(self.gl.GetError(), 0);
                }
            }
        }
        if self.using_3d && self.perspective {
            self.set_projection_perspective(src_x.into(), src_y.into(), src_w.into(), src_h.into(), src_angle);
        } else {
            self.set_projection_ortho(src_x.into(), src_y.into(), src_w.into(), src_h.into(), src_angle);
        }
    }

    fn clear_view(&mut self, colour: Colour, alpha: f64) {
        self.flush_queue();
        unsafe {
            self.gl.ClearColor(colour.r as f32, colour.g as f32, colour.b as f32, alpha as f32);
            self.gl.Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
            debug_assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn clear_view_no_zbuf(&mut self, colour: Colour, alpha: f64) {
        self.flush_queue();
        unsafe {
            self.gl.ClearColor(colour.r as f32, colour.g as f32, colour.b as f32, alpha as f32);
            self.gl.Clear(gl::COLOR_BUFFER_BIT);
            debug_assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn clear_zbuf(&mut self) {
        if self.using_3d {
            self.flush_queue();
            unsafe {
                self.gl.Clear(gl::DEPTH_BUFFER_BIT);
            }
        }
    }

    fn get_3d(&self) -> bool {
        self.using_3d
    }

    fn set_3d(&mut self, use_3d: bool) {
        self.using_3d = use_3d;
        self.set_depth_test(use_3d);
        self.set_perspective(use_3d);
    }

    fn get_depth(&self) -> f32 {
        self.depth
    }

    fn set_depth(&mut self, depth: f32) {
        self.depth = if self.using_3d { depth.max(-16000.0).min(16000.0) } else { 0.0 };
    }

    fn get_depth_test(&self) -> bool {
        self.next_render_state.depth_test.into()
    }

    fn set_depth_test(&mut self, depth_test: bool) {
        let depth_test = depth_test && self.using_3d;
        self.next_render_state.depth_test = depth_test.into();
        self.render_state_updated = true;
    }

    fn get_write_depth(&self) -> bool {
        self.next_render_state.write_depth
    }

    fn set_write_depth(&mut self, write_depth: bool) {
        self.next_render_state.write_depth = write_depth;
        self.render_state_updated = true;
    }

    fn get_culling(&self) -> bool {
        self.next_render_state.culling
    }

    fn set_culling(&mut self, culling: bool) {
        self.next_render_state.culling = culling;
        self.render_state_updated = true;
    }

    fn get_perspective(&self) -> bool {
        self.perspective
    }

    fn set_perspective(&mut self, perspective: bool) {
        // don't need to flush_queue for this because this only affects set_view
        self.perspective = perspective;
    }

    fn get_fog(&self) -> Option<Fog> {
        let c = self.next_render_state.fog_colour;
        bool::from(self.next_render_state.fog_enabled).then(|| Fog {
            colour: u32::from(Colour::from((f64::from(c[0]), c[1].into(), c[2].into()))) as i32,
            begin: self.next_render_state.fog_begin,
            end: self.next_render_state.fog_end,
        })
    }

    fn set_fog(&mut self, fog: Option<Fog>) {
        self.next_render_state.fog_enabled = fog.is_some().into();
        if let Some(fog) = fog {
            self.next_render_state.fog_colour = split_colour(fog.colour, 1.0);
            self.next_render_state.fog_begin = fog.begin;
            self.next_render_state.fog_end = fog.end;
        }
        self.render_state_updated = true;
    }

    fn get_gouraud(&self) -> bool {
        self.next_render_state.gouraud.into()
    }

    fn set_gouraud(&mut self, gouraud: bool) {
        self.next_render_state.gouraud = gouraud.into();
        self.render_state_updated = true;
    }

    fn get_lighting_enabled(&self) -> bool {
        self.next_render_state.lighting.into()
    }

    fn set_lighting_enabled(&mut self, enabled: bool) {
        self.next_render_state.lighting = enabled.into();
        self.render_state_updated = true;
    }

    fn get_ambient_colour(&self) -> i32 {
        let col = &self.next_render_state.ambient_colour[0..3];
        u32::from(Colour::from((f64::from(col[0]), col[1].into(), col[2].into()))) as _
    }

    fn set_ambient_colour(&mut self, colour: i32) {
        self.next_render_state.ambient_colour = split_colour(colour, 1.0);
        self.render_state_updated = true;
    }

    fn get_lights(&self) -> [(bool, Light); 8] {
        let mut iter = self.next_render_state.lights.iter().map(|l| {
            let position = [l.pos[0], l.pos[1], l.pos[2]];
            let colour =
                u32::from(Colour::from((f64::from(l.colour[0]), l.colour[1].into(), l.colour[2].into()))) as i32;
            (
                l.enabled.into(),
                if l.is_point.into() {
                    Light::Point { position, range: l.range, colour }
                } else {
                    Light::Directional { direction: position, colour }
                },
            )
        });
        [
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
        ]
    }

    fn set_lights(&mut self, lights: [(bool, Light); 8]) {
        lights.iter().enumerate().for_each(|(i, &(enabled, light))| {
            self.set_light_enabled(i, enabled);
            self.set_light(i, light);
        })
    }

    fn set_light_enabled(&mut self, id: usize, enabled: bool) {
        self.next_render_state.lights[id].enabled = enabled.into();
        self.render_state_updated = true;
    }

    fn set_light(&mut self, id: usize, light: Light) {
        match light {
            Light::Directional { direction, colour } => {
                self.next_render_state.lights[id].is_point = false.into();
                self.next_render_state.lights[id].pos[0..3].copy_from_slice(&direction);
                self.next_render_state.lights[id].colour = split_colour(colour, 1.0);
            },
            Light::Point { position, range, colour } => {
                self.next_render_state.lights[id].is_point = true.into();
                self.next_render_state.lights[id].pos[0..3].copy_from_slice(&position);
                self.next_render_state.lights[id].colour = split_colour(colour, 1.0);
                self.next_render_state.lights[id].range = range;
            },
        }
        self.render_state_updated = true;
    }

    fn present(&mut self, window_width: u32, window_height: u32, scaling: Scaling) {
        if window_width == 0 || window_height == 0 {
            // if we continue, intel will dereference a null pointer
            return
        }
        unsafe {
            // Finish drawing frame
            self.flush_queue();

            // Get framebuffer size
            let fb_width = self.framebuffer.width as i32;
            let fb_height = self.framebuffer.height as i32;

            // yeah i know but they need to be converted anyway
            let (window_width_u, window_height_u) = (window_width, window_height);
            let (window_width, window_height) = (window_width as i32, window_height as i32);

            // Where the game image lands, shared with the input mapping and the touch controls.
            let (w_x, w_y, w_w, w_h) = crate::render::display_rect(
                window_width_u,
                window_height_u,
                fb_width as u32,
                fb_height as u32,
                scaling,
            );

            // Where the game image lands on screen, so the controls that belong over it (the dialogue
            // button) can be placed relative to the game instead of the window.
            #[cfg(target_os = "android")]
            if window_width > 0 && window_height > 0 {
                crate::touch_controls::set_viewport(
                    w_x as f32 / window_width as f32,
                    w_y as f32 / window_height as f32,
                    w_w as f32 / window_width as f32,
                    w_h as f32 / window_height as f32,
                );
                // The launcher keeps its own loading screen until the game is really there. The first
                // presented frame is not enough: the game paints a small "Loading…" screen of its own
                // before the music starts, so the signal is the first sound being heard (with a
                // generous fallback in case a device has no audio at all).
                static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if !READY.load(std::sync::atomic::Ordering::Relaxed) {
                    let frames = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if crate::game::audio::sound_started() || frames > 900 {
                        READY.store(true, std::sync::atomic::Ordering::Relaxed);
                        let _ = std::fs::write("/data/data/com.klisee.reloaded/files/game-ready", b"");
                    }
                }
            }

            // On Intel, glBlitFrameBuffer just does nothing if the scissor box is too big, which it
            // very well could be. So just disable the scissor test for now.
            self.gl.Disable(gl::SCISSOR_TEST);

            // Remember old framebuffer so we can rebind it after we're done
            let mut fb_old = 0;
            self.gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut fb_old);
            debug_assert_eq!(self.gl.GetError(), 0);

            // Draw framebuffer to screen
            self.current_texture = None;
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            debug_assert_eq!(self.gl.GetError(), 0);
            self.clear_view((0.0, 0.0, 0.0).into(), 1.0); // to avoid weird strobe lights (???)
            debug_assert_eq!(self.gl.GetError(), 0);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, self.framebuffer.fbo);
            debug_assert_eq!(self.gl.GetError(), 0);
            // `display_rect` gives a rectangle in window coordinates, top-left origin (the same one
            // the touches use). GL's origin is the bottom-left, so the vertical position is flipped
            // here. For a full-height or centred rectangle the two agree, so only the keyboard inset
            // - which anchors the game to the top - actually needs the flip.
            let blit_y = window_height - w_y - w_h;
            self.gl.BlitFramebuffer(
                0,
                fb_height,
                fb_width,
                0,
                w_x,
                blit_y,
                w_x + w_w,
                blit_y + w_h,
                gl::COLOR_BUFFER_BIT,
                if cfg!(target_os = "android") {
                    gl::NEAREST
                } else if self.next_render_state.interpolate_pixels.into() {
                    gl::LINEAR
                } else {
                    gl::NEAREST
                },
            );
            debug_assert_eq!(self.gl.GetError(), 0);
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, fb_old as u32);
            debug_assert_eq!(self.gl.GetError(), 0);

            self.gl.Enable(gl::SCISSOR_TEST);

            debug_assert_eq!(self.gl.GetError(), 0);

            // On-screen touch controls (Android): drawn on top of the presented frame.
            #[cfg(target_os = "android")]
            self.draw_touch_overlay(window_width_u, window_height_u);

            // Present buffer
            // Note: Game Maker always presents the backbuffer, even when the target is a surface.
            self.imp.swap_buffers();

            // On Nvidia/AMD cards, unless the emulator is running in admin mode, if it is screenshared on Discord,
            // SwapBuffers will return TRUE i.e. no error, but glGetError will return GL_INVALID_OPERATION.
            // This hack evades the error, but a less awful solution would be really nice to have.
            self.gl.GetError();
        }
    }

    fn present_surface(&mut self, surface: AtlasRef, window_width: u32, window_height: u32) {
        if window_width == 0 || window_height == 0 {
            return
        }
        let Some(rect) = self.get_rect(surface) else { return };
        let (sx, sy, sw, sh, atlas_id) = (rect.x, rect.y, rect.w, rect.h, rect.atlas_id);
        let Some(Some(fbo)) = self.fbo_ids.get(atlas_id as usize) else { return };
        let fbo = *fbo;
        unsafe {
            // Finish drawing the page into the surface.
            self.flush_queue();
            self.gl.Disable(gl::SCISSOR_TEST);
            let mut fb_old = 0;
            self.gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut fb_old);
            self.current_texture = None;
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            self.gl.Viewport(0, 0, window_width as i32, window_height as i32);
            self.gl.ClearColor(0.0, 0.0, 0.0, 1.0);
            self.gl.Clear(gl::COLOR_BUFFER_BIT);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, fbo);
            // The surface texture is bottom-up like the game's framebuffer, hence the flipped source.
            self.gl.BlitFramebuffer(
                sx,
                sy + sh,
                sx + sw,
                sy,
                0,
                0,
                window_width as i32,
                window_height as i32,
                gl::COLOR_BUFFER_BIT,
                gl::NEAREST,
            );
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, fb_old as u32);
            self.gl.Enable(gl::SCISSOR_TEST);
            self.imp.swap_buffers();
            self.gl.GetError();
        }
    }

    fn draw_stored(&mut self, x: i32, y: i32, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return
        }

        let framebuffer = match self.stored_framebuffer {
            Some(f) => f,
            None => return,
        };

        self.flush_queue();

        unsafe {
            // Get framebuffer size
            let fb_width = framebuffer.width as i32;
            let fb_height = framebuffer.height as i32;

            let (width, height) = (w as i32, h as i32);

            // Remember old framebuffer so we can rebind it after we're done
            let mut fb_old = 0;
            self.gl.GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut fb_old);
            debug_assert_eq!(self.gl.GetError(), 0);

            // Draw framebuffer to screen
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffer.fbo);
            self.gl.BindFramebuffer(gl::READ_FRAMEBUFFER, framebuffer.fbo);
            self.gl.BlitFramebuffer(
                0,
                0,
                fb_width,
                fb_height,
                x,
                y,
                x + width,
                y + height,
                gl::COLOR_BUFFER_BIT,
                gl::NEAREST,
            );
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, fb_old as u32);

            debug_assert_eq!(self.gl.GetError(), 0);
        }
    }

    fn stored_size(&self) -> (u32, u32) {
        let framebuffer = self.stored_framebuffer.unwrap_or(self.framebuffer);
        (framebuffer.width, framebuffer.height)
    }

    fn finish(&mut self, window_width: u32, window_height: u32, clear_colour: Colour) {
        // Present screen
        self.present(window_width, window_height, Scaling::Fixed(1.0));

        // Start next frame
        self.setup_frame(clear_colour)
    }
}
