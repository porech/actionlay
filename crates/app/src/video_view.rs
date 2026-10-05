//! GPU presentation of NV12 frames inside an egui layout, with the telemetry overlay
//! composited over them (premultiplied alpha, same viewport as the video).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use actionlay_media::{
    color::{ColorInfo, yuv_to_rgb},
    frame::Nv12Frame,
};
use actionlay_render::tiny_skia::Pixmap;
use eframe::egui;
use egui_wgpu::wgpu;

pub fn fit_rect(available: egui::Rect, video_w: u32, video_h: u32) -> egui::Rect {
    let aspect = video_w as f32 / video_h as f32;
    let mut size = available.size();
    if size.x / size.y > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    egui::Rect::from_center_size(available.center(), size)
}

/// [`fit_rect`] snapped to physical pixels: the paint callback's viewport and the
/// overlay texture then have exactly the same size, so the overlay is drawn 1:1.
pub fn video_rect_px(available: egui::Rect, video_w: u32, video_h: u32, ppp: f32) -> egui::Rect {
    use egui::emath::GuiRounding as _;
    fit_rect(available, video_w, video_h).round_to_pixels(ppp)
}

/// Blend state of the overlay draw: the renderer's pixels are premultiplied.
pub const OVERLAY_BLEND: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

/// CPU mirror of [`OVERLAY_BLEND`] on a non-sRGB target, one 8-bit pixel (tests).
#[cfg(test)]
pub fn premultiplied_over(src: [u8; 4], dst: [u8; 3]) -> [u8; 3] {
    let inv = 255 - u32::from(src[3]);
    std::array::from_fn(|i| {
        (u32::from(src[i]) + (u32::from(dst[i]) * inv + 127) / 255).min(255) as u8
    })
}

struct OverlayTexture {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

type PendingOverlay = Arc<Mutex<Option<Pixmap>>>;
type Recycled = Arc<Mutex<Vec<Pixmap>>>;

/// The overlay mutexes are only held for a swap; a panic elsewhere must not poison them
/// for the UI.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    rows: [[f32; 4]; 3],
}

struct Textures {
    width: u32,
    height: u32,
    y: wgpu::Texture,
    uv: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    textures: Option<Textures>,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_layout: wgpu::BindGroupLayout,
    overlay: Option<OverlayTexture>,
}

type Pending = Arc<Mutex<Option<(Nv12Frame, ColorInfo)>>>;

pub struct VideoView {
    pending: Pending,
    size: Option<(u32, u32)>,
    overlay: PendingOverlay,
    recycled: Recycled,
    /// Set by [`VideoView::clear_overlay`]: the next paint drops the overlay texture.
    clear_overlay: Arc<AtomicBool>,
}

impl VideoView {
    pub fn new(rs: &egui_wgpu::RenderState) -> Self {
        log::info!(
            "video target format: {:?} (srgb: {})",
            rs.target_format,
            rs.target_format.is_srgb()
        );
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv"),
            source: wgpu::ShaderSource::Wgsl(include_str!("yuv.wgsl").into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if rs.target_format.is_srgb() {
                    "fs_main_srgb"
                } else {
                    "fs_main"
                }),
                targets: &[Some(rs.target_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[
                tex_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay"),
                bind_group_layouts: &[Some(&overlay_layout)],
                immediate_size: 0,
            });
        // the overlay bytes are sRGB-encoded: on an sRGB target (which blends in linear
        // light) the shader linearizes them, like the video's fs_main_srgb
        let overlay_entry = if rs.target_format.is_srgb() {
            "fs_main_srgb"
        } else {
            "fs_main"
        };
        log::info!("overlay fragment shader: {overlay_entry}");
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some(overlay_entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: rs.target_format,
                    blend: Some(OVERLAY_BLEND),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuv"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv-params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        rs.renderer.write().callback_resources.insert(Resources {
            pipeline,
            layout,
            sampler,
            params,
            textures: None,
            overlay_pipeline,
            overlay_layout,
            overlay: None,
        });
        Self {
            pending: Arc::new(Mutex::new(None)),
            size: None,
            overlay: Arc::new(Mutex::new(None)),
            recycled: Arc::new(Mutex::new(Vec::new())),
            clear_overlay: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn upload(&mut self, frame: Nv12Frame, color: ColorInfo) {
        self.size = Some((frame.width, frame.height));
        *self.pending.lock().unwrap() = Some((frame, color));
    }

    /// Overlay image for the next paint; a previous one not yet uploaded is recycled.
    pub fn upload_overlay(&mut self, pixmap: Pixmap) {
        if let Some(old) = lock(&self.overlay).replace(pixmap) {
            lock(&self.recycled).push(old);
        }
    }

    /// Forgets the overlay (another video was opened): the pending image is recycled
    /// and the GPU texture dropped at the next paint, so it can never be drawn again.
    pub fn clear_overlay(&mut self) {
        if let Some(old) = lock(&self.overlay).take() {
            lock(&self.recycled).push(old);
        }
        self.clear_overlay.store(true, Ordering::Release);
    }

    /// Pixmaps already copied to the GPU, to hand back to the overlay worker.
    pub fn take_recycled(&self) -> Vec<Pixmap> {
        std::mem::take(&mut *lock(&self.recycled))
    }

    /// Where the video is drawn inside `available`, snapped to physical pixels (None
    /// before the first frame). Pass the result to [`VideoView::show`] and use it for the
    /// overlay size.
    pub fn video_rect(&self, available: egui::Rect, ppp: f32) -> Option<egui::Rect> {
        self.size.map(|(w, h)| video_rect_px(available, w, h, ppp))
    }

    /// Fills `rect` with black and draws the video in `video` (from
    /// [`VideoView::video_rect`]) and, when `show_overlay`, the last overlay uploaded
    /// over the same rect (it lags a window resize by a few frames).
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        video: Option<egui::Rect>,
        show_overlay: bool,
    ) {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        let Some(target) = video else {
            return;
        };
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            target,
            Paint {
                pending: self.pending.clone(),
                overlay: self.overlay.clone(),
                recycled: self.recycled.clone(),
                clear_overlay: self.clear_overlay.clone(),
                show_overlay,
            },
        ));
    }
}

struct Paint {
    pending: Pending,
    overlay: PendingOverlay,
    recycled: Recycled,
    clear_overlay: Arc<AtomicBool>,
    show_overlay: bool,
}

impl egui_wgpu::CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let res: &mut Resources = resources.get_mut().unwrap();
        if let Some((frame, color)) = self.pending.lock().unwrap().take() {
            let needs_new = res
                .textures
                .as_ref()
                .is_none_or(|t| t.width != frame.width || t.height != frame.height);
            if needs_new {
                res.textures = Some(create_textures(device, res, frame.width, frame.height));
            }
            let t = res.textures.as_ref().unwrap();
            let (cw, ch) = (frame.width.div_ceil(2), frame.height.div_ceil(2));
            write_plane(queue, &t.y, &frame.y, frame.width, frame.height, 1);
            write_plane(queue, &t.uv, &frame.uv, cw, ch, 2);
            queue.write_buffer(
                &res.params,
                0,
                bytemuck::bytes_of(&Params {
                    rows: yuv_to_rgb(color),
                }),
            );
        }
        if self.clear_overlay.swap(false, Ordering::AcqRel) {
            res.overlay = None;
        }
        if let Some(pixmap) = lock(&self.overlay).take() {
            let (w, h) = (pixmap.width(), pixmap.height());
            // same size: write into the existing texture, no reallocation
            if res
                .overlay
                .as_ref()
                .is_none_or(|o| o.width != w || o.height != h)
            {
                res.overlay = Some(create_overlay_texture(device, res, w, h));
            }
            let o = res.overlay.as_ref().unwrap();
            write_plane(queue, &o.texture, pixmap.data(), w, h, 4);
            lock(&self.recycled).push(pixmap);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &Resources = resources.get().unwrap();
        let Some(t) = &res.textures else { return };
        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, &t.bind_group, &[]);
        pass.draw(0..6, 0..1);
        if self.show_overlay
            && let Some(o) = &res.overlay
        {
            pass.set_pipeline(&res.overlay_pipeline);
            pass.set_bind_group(0, &o.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

fn create_textures(device: &wgpu::Device, res: &Resources, width: u32, height: u32) -> Textures {
    let make = |label, w, h, format| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let y = make("y", width, height, wgpu::TextureFormat::R8Unorm);
    let uv = make(
        "uv",
        width.div_ceil(2),
        height.div_ceil(2),
        wgpu::TextureFormat::Rg8Unorm,
    );
    let yv = y.create_view(&Default::default());
    let uvv = uv.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yuv"),
        layout: &res.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&yv),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&uvv),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&res.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: res.params.as_entire_binding(),
            },
        ],
    });
    Textures {
        width,
        height,
        y,
        uv,
        bind_group,
    }
}

fn create_overlay_texture(
    device: &wgpu::Device,
    res: &Resources,
    width: u32,
    height: u32,
) -> OverlayTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // the bytes are sRGB-encoded but must be sampled as-is (see overlay.wgsl)
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay"),
        layout: &res.overlay_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&res.sampler),
            },
        ],
    });
    OverlayTexture {
        width,
        height,
        texture,
        bind_group,
    }
}

fn write_plane(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    data: &[u8],
    w: u32,
    h: u32,
    bytes_per_px: u32,
) {
    queue.write_texture(
        tex.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * bytes_per_px),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actionlay_render::tiny_skia::{
        Color, FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Transform,
    };
    use egui::{Rect, pos2};

    #[test]
    fn overlay_blend_is_premultiplied_over() {
        use egui_wgpu::wgpu::{BlendFactor, BlendOperation};
        for c in [OVERLAY_BLEND.color, OVERLAY_BLEND.alpha] {
            assert_eq!(c.src_factor, BlendFactor::One);
            assert_eq!(c.dst_factor, BlendFactor::OneMinusSrcAlpha);
            assert_eq!(c.operation, BlendOperation::Add);
        }
    }

    #[test]
    fn renderer_pixels_composite_like_straight_alpha_over() {
        // a 50% red square from tiny-skia is stored premultiplied: (128, 0, 0, 128)
        let mut pm = Pixmap::new(4, 4).unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba(1.0, 0.0, 0.0, 0.5).unwrap());
        let rect = PathBuilder::from_rect(SkRect::from_xywh(0.0, 0.0, 2.0, 4.0).unwrap());
        pm.fill_path(
            &rect,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let p = pm.pixel(0, 0).unwrap();
        let src = [p.red(), p.green(), p.blue(), p.alpha()];
        assert_eq!(src, [128, 0, 0, 128]);
        // straight-alpha reference: dst × (1 − a) + colour × a
        assert_eq!(premultiplied_over(src, [255, 255, 255]), [255, 127, 127]);
        assert_eq!(premultiplied_over(src, [0, 0, 0]), [128, 0, 0]);
        // transparent pixels leave the video untouched, opaque ones replace it
        let clear = pm.pixel(3, 0).unwrap();
        assert_eq!(
            premultiplied_over(
                [clear.red(), clear.green(), clear.blue(), clear.alpha()],
                [10, 20, 30]
            ),
            [10, 20, 30]
        );
        assert_eq!(
            premultiplied_over([200, 100, 50, 255], [10, 20, 30]),
            [200, 100, 50]
        );
    }

    #[test]
    fn overlay_shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(include_str!("overlay.wgsl")).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validates");
        let entries: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        for name in ["vs_main", "fs_main", "fs_main_srgb"] {
            assert!(entries.contains(&name), "{name} missing: {entries:?}");
        }
    }

    /// Size of the viewport egui-wgpu gives the paint callback of `rect`.
    fn viewport_px(rect: Rect, ppp: f32) -> (u32, u32) {
        let v = egui::epaint::ViewportInPixels::from_points(&rect, ppp, [100_000, 100_000]);
        (v.width_px as u32, v.height_px as u32)
    }

    #[test]
    fn overlay_size_matches_the_paint_viewport() {
        let mut checked = 0;
        for ppp in [1.0_f32, 1.5, 2.0] {
            // fractional origins too: the central panel starts wherever the UI ends
            for (x0, y0) in [(0.0, 0.0), (0.5, 0.25), (7.3, 3.7)] {
                for w in (301..=1301).step_by(7) {
                    for h in (203..=903).step_by(50) {
                        let available =
                            Rect::from_min_size(pos2(x0, y0), egui::vec2(w as f32, h as f32));
                        for (vw, vh) in [(1920, 1080), (1920, 1440), (1080, 1920), (2704, 2028)] {
                            let r = video_rect_px(available, vw, vh, ppp);
                            let size =
                                crate::overlay::overlay_size(r.width(), r.height(), ppp, 100_000);
                            assert_eq!(
                                size,
                                Some(viewport_px(r, ppp)),
                                "ppp {ppp} available {available:?} video {vw}x{vh} rect {r:?}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked}");
    }

    #[test]
    fn fit_rect_letterboxes_and_pillarboxes() {
        let wide = Rect::from_min_max(pos2(0.0, 0.0), pos2(1600.0, 900.0));
        let r = fit_rect(wide, 1920, 1440); // 4:3 inside 16:9 -> pillarbox
        assert_eq!((r.width(), r.height()), (1200.0, 900.0));
        assert_eq!(r.center(), wide.center());
        let tall = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 900.0));
        let r = fit_rect(tall, 1920, 1080); // 16:9 inside a tall area -> letterbox
        assert_eq!((r.width(), r.height()), (800.0, 450.0));
    }
}
