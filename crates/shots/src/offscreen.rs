//! 不可視ウィンドウ + FBO による egui のオフスクリーン描画。
//!
//! DAW もマウス操作も介さずに GUI を PNG へ焼くための唯一の経路。描画そのものは
//! 各 crate の `editor::draw` が行い、ここは GL 文脈と読み出しだけを持つ。

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext};
use glutin::display::{Display, DisplayApiPreference};
use glutin::prelude::*;
use glutin::surface::GlSurface;
use glutin_winit::GlWindow;
use nih_plug_egui::egui;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// 1 フレーム分の描画内容を供給する。`Renderer::frame` から呼ばれる。
pub trait Scene {
    /// egui の `Context` が新しく作られた直後に 1 度だけ呼ばれる。
    fn build(&mut self, ctx: &egui::Context);
    /// フレームごとの UI 構築。
    fn draw(&mut self, ctx: &egui::Context);
}

/// `Renderer` を作って `job` を走らせる。イベントループの都合で
/// クロージャ経由でしか GL 文脈を渡せない。
pub fn with_renderer<F>(width: u32, height: u32, job: F)
where
    F: FnOnce(&mut Renderer),
{
    struct App<F> {
        width: u32,
        height: u32,
        job: Option<F>,
    }

    impl<F: FnOnce(&mut Renderer)> ApplicationHandler for App<F> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let Some(job) = self.job.take() else {
                return;
            };
            let mut renderer = Renderer::new(event_loop, self.width, self.height);
            job(&mut renderer);
            event_loop.exit();
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
        }
    }

    let mut app = App {
        width,
        height,
        job: Some(job),
    };
    let event_loop = EventLoop::new().expect("EventLoop::new");
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app).expect("run_app");
}

/// 不可視ウィンドウに紐づく GL 文脈と、そこへ egui を描く painter。
pub struct Renderer {
    width: u32,
    height: u32,
    gl: Arc<glow::Context>,
    painter: egui_glow::Painter,
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    // 破棄順のために保持する。GL 文脈より先に消してはならない。
    _context: glutin::context::PossiblyCurrentContext,
    _surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    _window: Window,
}

impl Renderer {
    fn new(event_loop: &ActiveEventLoop, width: u32, height: u32) -> Self {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(width, height));
        let window = event_loop.create_window(attrs).expect("create_window");

        let display_handle = event_loop
            .display_handle()
            .expect("display_handle")
            .as_raw();
        let window_handle = window.window_handle().expect("window_handle").as_raw();

        let preference = DisplayApiPreference::WglThenEgl(Some(window_handle));
        let gl_display = unsafe { Display::new(display_handle, preference) }.expect("Display::new");

        let template = ConfigTemplateBuilder::new().build();
        let config = unsafe { gl_display.find_configs(template) }
            .expect("find_configs")
            .next()
            .expect("no GL config");

        let context_attributes = ContextAttributesBuilder::new().build(Some(window_handle));
        let not_current = unsafe { gl_display.create_context(&config, &context_attributes) }
            .expect("create_context");

        let surface_attrs = window
            .build_surface_attributes(Default::default())
            .expect("build_surface_attributes");
        let surface = unsafe { gl_display.create_window_surface(&config, &surface_attrs) }
            .expect("create_window_surface");
        let context = not_current.make_current(&surface).expect("make_current");

        surface.resize(
            &context,
            NonZeroU32::new(width).expect("width"),
            NonZeroU32::new(height).expect("height"),
        );

        let gl = Arc::new(unsafe {
            glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
        });

        let (tex, fbo) = unsafe { Self::make_target(&gl, width, height) };

        let painter =
            // dithering はプラグイン本体 (egui-baseview の既定) と同じ true にする。
            // 揃えないとグラデーション上に ±1 の差が出る。
            egui_glow::Painter::new(gl.clone(), "", None, true).expect("egui_glow::Painter::new");

        Self {
            width,
            height,
            gl,
            painter,
            fbo,
            tex,
            _context: context,
            _surface: surface,
            _window: window,
        }
    }


    /// FBO とカラーテクスチャを作る。
    ///
    /// # Safety
    ///
    /// GL 文脈が current であること。
    unsafe fn make_target(
        gl: &glow::Context,
        width: u32,
        height: u32,
    ) -> (glow::Texture, glow::Framebuffer) {
            let tex = gl.create_texture().expect("create_texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );

            let fbo = gl.create_framebuffer().expect("create_framebuffer");
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                panic!("framebuffer incomplete: {status:#x}");
            }
            gl.viewport(0, 0, width as i32, height as i32);
            (tex, fbo)
    }

    /// 描画先のサイズを変える。FBO とテクスチャを作り直す。
    ///
    /// タイル合成のように 1 回の実行で複数のサイズを描くために要る
    /// (GL 文脈は 1 つで足りるので、Renderer ごと作り直さない)。
    pub fn resize(&mut self, width: u32, height: u32) {
        if (self.width, self.height) == (width, height) {
            return;
        }
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.tex);
        }
        let (tex, fbo) = unsafe { Self::make_target(&self.gl, width, height) };
        self.width = width;
        self.height = height;
        self.tex = tex;
        self.fbo = fbo;
    }

    /// `scene` を描いて RGBA8 の画像 (上下反転済み) を返す。
    ///
    /// 背景テクスチャは最初のフレームで生成され、その回の描画には間に合わない。
    /// 2 フレーム走らせて 2 枚目を採る (毎回同じ枚数なので決定論は保たれる)。
    pub fn frame(&mut self, scene: &mut dyn Scene) -> image::RgbaImage {
        self.frame_at(scene, 1.0)
    }

    /// 背景を**透明**にして描く。他の画へ重ねる層を作るときだけ使う
    /// (`tile::NamesScene`)。既定の [`frame`](Self::frame) は不透明の黒で塗るため、
    /// そのまま重ねると下の画を消してしまう。
    pub fn frame_transparent(&mut self, scene: &mut dyn Scene) -> image::RgbaImage {
        self.frame_inner(scene, 1.0, [0.0, 0.0, 0.0, 0.0])
    }

    /// `pixels_per_point` を指定して描く。プラグイン本体は OS の DPI 倍率で描くので、
    /// その条件を再現するときに使う。
    pub fn frame_at(&mut self, scene: &mut dyn Scene, pixels_per_point: f32) -> image::RgbaImage {
        self.frame_inner(scene, pixels_per_point, [0.0, 0.0, 0.0, 1.0])
    }

    fn frame_inner(
        &mut self,
        scene: &mut dyn Scene,
        pixels_per_point: f32,
        clear: [f32; 4],
    ) -> image::RgbaImage {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        scene.build(&ctx);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    self.width as f32 / pixels_per_point,
                    self.height as f32 / pixels_per_point,
                ),
            )),
            ..Default::default()
        };

        for _ in 0..2 {
            let full_output = ctx.run(raw_input.clone(), |ctx| scene.draw(ctx));
            let primitives = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
            unsafe {
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            }
            self.painter.clear([self.width, self.height], clear);
            self.painter.paint_and_update_textures(
                [self.width, self.height],
                pixels_per_point,
                &primitives,
                &full_output.textures_delta,
            );
        }

        let mut pixels = vec![0u8; (self.width * self.height * 4) as usize];
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(pixels.as_mut_slice())),
            );
            let err = self.gl.get_error();
            if err != glow::NO_ERROR {
                panic!("glReadPixels: {err:#x}");
            }
        }

        let img = image::RgbaImage::from_raw(self.width, self.height, pixels)
            .expect("RgbaImage::from_raw");
        image::imageops::flip_vertical(&img)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.painter.destroy();
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.tex);
        }
    }
}

/// PNG を書き出す。親ディレクトリは必要なら作る。
pub fn save_png(img: &image::RgbaImage, out: &Path) {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).expect("create_dir_all");
        }
    }
    img.save(out)
        .unwrap_or_else(|e| panic!("save {}: {e}", out.display()));
}
