//! Render-API backend for platforms without a Wayland compositor to proxy
//! (macOS). mpv draws into an offscreen framebuffer on a GDK GL context that
//! shares objects with GSK, and every frame is handed over as a `GdkGLTexture`.

use std::{
    ffi::{
        CStr,
        c_char,
        c_int,
        c_void,
    },
    ptr,
    sync::Arc,
};

use glow::HasContext;
use gtk::{
    gdk,
    prelude::*,
};
use libmpv2::Mpv;
use libmpv2_sys as sys;

use super::{
    RENDER_UPDATE,
    get_proc_address,
};

// Textures that are neither on screen nor in flight. Anything beyond this is
// deleted when it comes back, so a resize does not leave a pile of stale ones.
const MAX_IDLE_TARGETS: usize = 3;

struct Target {
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
    width: i32,
    height: i32,
}

// GDK hands textures back from its release callback, which must be `Send`.
// The fence is only touched again on the main thread, so its address is enough.
struct Released {
    target: Target,
    fence: usize,
}

/// libmpv2's `RenderContext` cannot pass `BLOCK_FOR_TARGET_TIME`, and without
/// it every render sleeps until the frame is due. On the main thread that
/// starves GTK's redraw source, so the context is driven through the C API.
struct RawRenderContext(*mut sys::mpv_render_context);

impl RawRenderContext {
    fn new(mpv: &Mpv) -> Result<Self, c_int> {
        unsafe extern "C" fn proc_address(_: *mut c_void, name: *const c_char) -> *mut c_void {
            let name = unsafe { CStr::from_ptr(name) };
            name.to_str()
                .map_or(ptr::null_mut(), |name| get_proc_address(name) as *mut c_void)
        }

        let mut init = sys::mpv_opengl_init_params {
            get_proc_address: Some(proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = [
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let mut ctx = ptr::null_mut();
        let ret = unsafe {
            sys::mpv_render_context_create(&mut ctx, mpv.ctx.as_ptr(), params.as_mut_ptr())
        };
        if ret < 0 {
            return Err(ret);
        }

        unsafe extern "C" fn on_update(_: *mut c_void) {
            let _ = RENDER_UPDATE.tx.send(true);
        }
        unsafe { sys::mpv_render_context_set_update_callback(ctx, Some(on_update), ptr::null_mut()) };

        Ok(Self(ctx))
    }

    fn update(&self) -> u64 {
        unsafe { sys::mpv_render_context_update(self.0) }
    }

    fn render(&self, fbo: c_int, width: c_int, height: c_int) -> Result<(), c_int> {
        let mut fbo = sys::mpv_opengl_fbo {
            fbo,
            w: width,
            h: height,
            internal_format: glow::RGBA8 as c_int,
        };
        // GDK textures are stored top row first, which is mpv's natural order
        // for an FBO, so no flip.
        let mut flip_y: c_int = 0;
        let mut block: c_int = 0;
        let mut params = [
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let ret = unsafe { sys::mpv_render_context_render(self.0, params.as_mut_ptr()) };
        if ret < 0 { Err(ret) } else { Ok(()) }
    }
}

impl Drop for RawRenderContext {
    fn drop(&mut self) {
        unsafe { sys::mpv_render_context_free(self.0) };
    }
}

pub struct GlRenderer {
    // Field order matters: mpv requires the render context to be freed before
    // the core it was created from.
    render: RawRenderContext,
    _mpv: Arc<Mpv>,
    context: gdk::GLContext,
    gl: glow::Context,
    idle: Vec<Target>,
    released_tx: flume::Sender<Released>,
    released_rx: flume::Receiver<Released>,
}

impl GlRenderer {
    pub fn new(mpv: Arc<Mpv>) -> Result<Self, glib::Error> {
        let display = gdk::Display::default().expect("Could not connect to display");
        let context = display.create_gl_context()?;
        context.realize()?;
        context.make_current();

        let gl = unsafe { glow::Context::from_loader_function(get_proc_address) };

        let render = RawRenderContext::new(&mpv).map_err(|code| {
            glib::Error::new(
                gdk::GLError::NotAvailable,
                &format!(
                    "failed to create mpv render context: {}",
                    sys::mpv_error_str(code)
                ),
            )
        })?;

        gdk::GLContext::clear_current();

        let (released_tx, released_rx) = flume::unbounded();

        Ok(Self {
            render,
            _mpv: mpv,
            context,
            gl,
            idle: Vec::new(),
            released_tx,
            released_rx,
        })
    }

    /// Whether mpv has a new frame for us since the last call.
    pub fn has_new_frame(&self) -> bool {
        self.render.update() & sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 != 0
    }

    /// Draws the current frame at the given size in physical pixels.
    pub fn render(&mut self, width: i32, height: i32) -> Option<gdk::Texture> {
        if width <= 0 || height <= 0 {
            return None;
        }

        self.context.make_current();
        self.reclaim(width, height);

        let target = match self.idle.pop() {
            Some(target) => target,
            None => match self.create_target(width, height) {
                Ok(target) => target,
                Err(e) => {
                    tracing::error!(target: "mutsumi::mpv", "failed to allocate frame: {e}");
                    gdk::GLContext::clear_current();
                    return None;
                }
            },
        };

        if let Err(code) = self
            .render
            .render(target.framebuffer.0.get() as c_int, width, height)
        {
            let e = sys::mpv_error_str(code);
            tracing::error!(target: "mutsumi::mpv", "mpv render failed: {e}");
            self.idle.push(target);
            gdk::GLContext::clear_current();
            return None;
        }

        let fence = unsafe {
            let fence = self.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0).ok();
            self.gl.flush();
            fence
        };
        let fence_ptr = fence.map_or(std::ptr::null(), |f| f.0 as *const c_void);

        let builder = gdk::GLTextureBuilder::new()
            .set_context(Some(&self.context))
            .set_id(target.texture.0.get())
            .set_width(width)
            .set_height(height)
            .set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied)
            .set_sync(Some(fence_ptr));

        let released_tx = self.released_tx.clone();
        let fence = fence_ptr as usize;
        let texture = unsafe {
            builder.build_with_release_func(move || {
                let _ = released_tx.send(Released { target, fence });
            })
        };

        gdk::GLContext::clear_current();
        Some(texture)
    }

    /// Recycles textures GSK has finished with. Must be called with the
    /// context current.
    fn reclaim(&mut self, width: i32, height: i32) {
        for Released { target, fence } in self.released_rx.drain() {
            unsafe {
                if fence != 0 {
                    self.gl.delete_sync(glow::NativeFence(fence as _));
                }
            }

            if target.width == width
                && target.height == height
                && self.idle.len() < MAX_IDLE_TARGETS
            {
                self.idle.push(target);
            } else {
                self.delete_target(target);
            }
        }

        let (keep, stale): (Vec<_>, Vec<_>) = std::mem::take(&mut self.idle)
            .into_iter()
            .partition(|t| t.width == width && t.height == height);
        self.idle = keep;
        for target in stale {
            self.delete_target(target);
        }
    }

    fn create_target(&self, width: i32, height: i32) -> Result<Target, String> {
        let gl = &self.gl;
        unsafe {
            let texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);

            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            let target = Target {
                framebuffer,
                texture,
                width,
                height,
            };
            if status != glow::FRAMEBUFFER_COMPLETE {
                self.delete_target(target);
                return Err(format!("framebuffer incomplete: {status:#x}"));
            }
            Ok(target)
        }
    }

    fn delete_target(&self, target: Target) {
        unsafe {
            self.gl.delete_framebuffer(target.framebuffer);
            self.gl.delete_texture(target.texture);
        }
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        self.context.make_current();
        for target in std::mem::take(&mut self.idle) {
            self.delete_target(target);
        }
        // Textures still on screen are leaked rather than deleted under GSK.
    }
}
