//! GPU presentation of NV12 frames inside an egui layout.
use std::sync::{Arc, Mutex};

use actionlay_media::{
    color::{ColorInfo, yuv_to_rgb},
    frame::Nv12Frame,
};
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
}

type Pending = Arc<Mutex<Option<(Nv12Frame, ColorInfo)>>>;

pub struct VideoView {
    pending: Pending,
    size: Option<(u32, u32)>,
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
        });
        Self {
            pending: Arc::new(Mutex::new(None)),
            size: None,
        }
    }

    pub fn upload(&mut self, frame: &Nv12Frame, color: ColorInfo) {
        self.size = Some((frame.width, frame.height));
        *self.pending.lock().unwrap() = Some((frame.clone(), color));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        let Some((w, h)) = self.size else { return };
        let target = fit_rect(rect, w, h);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            target,
            Paint {
                pending: self.pending.clone(),
            },
        ));
    }
}

struct Paint {
    pending: Pending,
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
        let Some((frame, color)) = self.pending.lock().unwrap().take() else {
            return Vec::new();
        };
        let res: &mut Resources = resources.get_mut().unwrap();
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
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &Resources = resources.get().unwrap();
        if let Some(t) = &res.textures {
            pass.set_pipeline(&res.pipeline);
            pass.set_bind_group(0, &t.bind_group, &[]);
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
    use egui::{Rect, pos2};

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
