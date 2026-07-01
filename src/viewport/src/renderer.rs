use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Pos2, Rect, Vec2};
use eframe::{egui_wgpu, wgpu};

use super::{
    BUFFER_SIZE_MIN, CachedObjectMesh, OverlayVertex, RECOMMENDED_MSAA_SAMPLES, RenderRequest,
    ViewUniform, ViewportScene, ViewportState, ViewportVertex,
};

const VIEWPORT_SHADER: &str = include_str!("shaders/viewport.wgsl");
const OVERLAY_SHADER: &str = include_str!("shaders/overlay.wgsl");

pub(crate) fn render_view_rgba(
    render_state: &egui_wgpu::RenderState,
    scene: &ViewportScene,
    state: &ViewportState,
    width: u32,
    height: u32,
    show_axes: bool,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("capture size must be non-zero".to_owned());
    }

    let device = &render_state.device;
    let queue = &render_state.queue;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let msaa = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_msaa"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: RECOMMENDED_MSAA_SAMPLES as u32,
        dimension: wgpu::TextureDimension::D2,
        format: render_state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let msaa_view = msaa.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: RECOMMENDED_MSAA_SAMPLES as u32,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width as f32, height as f32));
    let request = RenderRequest::from_scene(scene, state, rect, show_axes);
    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: 1.0,
    };
    let mut renderer = WgpuViewport::new(device, render_state.target_format);
    renderer.prepare(device, queue, &screen_descriptor, &request);

    let bytes_per_pixel = 4_u32;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| "export image row is too large".to_owned())?;
    let padded_row_bytes = align_to(row_bytes, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer_size = u64::from(padded_row_bytes)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "export image buffer is too large".to_owned())?;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gds3d_export_readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gds3d_export_encoder"),
    });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gds3d_export_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view,
                resolve_target: Some(&target_view),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.paint(&mut render_pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|err| format!("wait for export render: {err}"))?;
    receiver
        .recv()
        .map_err(|err| format!("wait for export readback: {err}"))?
        .map_err(|err| format!("map export readback: {err}"))?;

    let texture_format = render_state.target_format;
    let mapped = readback.get_mapped_range(..);
    let rgba_size = usize::try_from(u64::from(row_bytes) * u64::from(height))
        .map_err(|_| "export image is too large".to_owned())?;
    let row_stride =
        usize::try_from(padded_row_bytes).map_err(|_| "invalid export row stride".to_owned())?;
    let row_len = usize::try_from(row_bytes).map_err(|_| "invalid export row size".to_owned())?;
    let mut rgba = Vec::with_capacity(rgba_size);
    for row in mapped.chunks(row_stride) {
        push_texture_row_as_rgba(&mut rgba, &row[..row_len], texture_format)?;
    }
    drop(mapped);
    readback.unmap();

    Ok(rgba)
}

pub(crate) struct ViewportCallback {
    pub(crate) renderer: Arc<Mutex<Option<WgpuViewport>>>,
    pub(crate) request: RenderRequest,
}

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Ok(mut guard) = self.renderer.lock()
            && let Some(renderer) = guard.as_mut()
        {
            renderer.prepare(device, queue, screen_descriptor, &self.request);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let viewport = info.viewport_in_pixels();
        if viewport.width_px == 0 || viewport.height_px == 0 {
            return;
        }
        if let Ok(guard) = self.renderer.lock()
            && let Some(renderer) = guard.as_ref()
        {
            renderer.paint(render_pass);
        }
    }
}

pub(crate) struct WgpuViewport {
    pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    overlay_vertex_buffer: wgpu::Buffer,
    overlay_index_buffer: wgpu::Buffer,
    vertex_capacity_bytes: u64,
    index_capacity_bytes: u64,
    overlay_vertex_capacity_bytes: u64,
    overlay_index_capacity_bytes: u64,
    index_count: u32,
    overlay_index_count: u32,
    mesh_revision: Option<u64>,
    object_meshes: HashMap<String, CachedObjectMesh>,
}

impl WgpuViewport {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gds3d_viewport_shader"),
            source: wgpu::ShaderSource::Wgsl(VIEWPORT_SHADER.into()),
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gds3d_viewport_uniform"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gds3d_viewport_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gds3d_viewport_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gds3d_viewport_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = create_viewport_pipeline(
            device,
            &shader,
            &pipeline_layout,
            target_format,
            "gds3d_viewport_pipeline",
            true,
        );
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gds3d_viewport_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gds3d_viewport_overlay_pipeline_layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gds3d_viewport_overlay_pipeline"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[OverlayVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: RECOMMENDED_MSAA_SAMPLES as u32,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_vertex_buffer",
            wgpu::BufferUsages::VERTEX,
            BUFFER_SIZE_MIN,
        );
        let index_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_index_buffer",
            wgpu::BufferUsages::INDEX,
            BUFFER_SIZE_MIN,
        );
        let overlay_vertex_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_overlay_vertex_buffer",
            wgpu::BufferUsages::VERTEX,
            BUFFER_SIZE_MIN,
        );
        let overlay_index_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_overlay_index_buffer",
            wgpu::BufferUsages::INDEX,
            BUFFER_SIZE_MIN,
        );

        Self {
            pipeline,
            overlay_pipeline,
            uniform_buffer,
            bind_group,
            vertex_buffer,
            index_buffer,
            overlay_vertex_buffer,
            overlay_index_buffer,
            vertex_capacity_bytes: BUFFER_SIZE_MIN,
            index_capacity_bytes: BUFFER_SIZE_MIN,
            overlay_vertex_capacity_bytes: BUFFER_SIZE_MIN,
            overlay_index_capacity_bytes: BUFFER_SIZE_MIN,
            index_count: 0,
            overlay_index_count: 0,
            mesh_revision: None,
            object_meshes: HashMap::new(),
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        request: &RenderRequest,
    ) {
        let uniform = ViewUniform::from_request(request);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        if self.mesh_revision != Some(request.scene_revision) {
            let mesh = request.mesh(&mut self.object_meshes);
            self.index_count = upload_viewport_mesh(
                device,
                queue,
                bytemuck::cast_slice(&mesh.vertices),
                bytemuck::cast_slice(&mesh.indices),
                mesh.indices.len() as u32,
                ViewportMeshBuffers {
                    vertex_buffer: &mut self.vertex_buffer,
                    vertex_capacity_bytes: &mut self.vertex_capacity_bytes,
                    index_buffer: &mut self.index_buffer,
                    index_capacity_bytes: &mut self.index_capacity_bytes,
                    vertex_label: "gds3d_viewport_vertex_buffer",
                    index_label: "gds3d_viewport_index_buffer",
                },
            );
            self.mesh_revision = Some(request.scene_revision);
        }

        let overlay = request.overlay_mesh(screen_descriptor.pixels_per_point);
        self.overlay_index_count = upload_viewport_mesh(
            device,
            queue,
            bytemuck::cast_slice(&overlay.vertices),
            bytemuck::cast_slice(&overlay.indices),
            overlay.indices.len() as u32,
            ViewportMeshBuffers {
                vertex_buffer: &mut self.overlay_vertex_buffer,
                vertex_capacity_bytes: &mut self.overlay_vertex_capacity_bytes,
                index_buffer: &mut self.overlay_index_buffer,
                index_capacity_bytes: &mut self.overlay_index_capacity_bytes,
                vertex_label: "gds3d_viewport_overlay_vertex_buffer",
                index_label: "gds3d_viewport_overlay_index_buffer",
            },
        );
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        if self.index_count > 0 {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
        if self.overlay_index_count > 0 {
            render_pass.set_pipeline(&self.overlay_pipeline);
            render_pass.set_vertex_buffer(0, self.overlay_vertex_buffer.slice(..));
            render_pass.set_index_buffer(
                self.overlay_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.draw_indexed(0..self.overlay_index_count, 0, 0..1);
        }
    }
}

fn create_viewport_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    label: &'static str,
    depth_write_enabled: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[ViewportVertex::layout()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: Some(depth_write_enabled),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: RECOMMENDED_MSAA_SAMPLES as u32,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

struct ViewportMeshBuffers<'a> {
    vertex_buffer: &'a mut wgpu::Buffer,
    vertex_capacity_bytes: &'a mut u64,
    index_buffer: &'a mut wgpu::Buffer,
    index_capacity_bytes: &'a mut u64,
    vertex_label: &'static str,
    index_label: &'static str,
}

fn upload_viewport_mesh(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertex_bytes: &[u8],
    index_bytes: &[u8],
    index_count: u32,
    buffers: ViewportMeshBuffers<'_>,
) -> u32 {
    if vertex_bytes.is_empty() || index_bytes.is_empty() {
        return 0;
    }

    let vertex_size = vertex_bytes.len() as u64;
    if vertex_size > *buffers.vertex_capacity_bytes {
        *buffers.vertex_capacity_bytes = next_buffer_size(vertex_size);
        *buffers.vertex_buffer = create_copy_buffer(
            device,
            buffers.vertex_label,
            wgpu::BufferUsages::VERTEX,
            *buffers.vertex_capacity_bytes,
        );
    }
    queue.write_buffer(buffers.vertex_buffer, 0, vertex_bytes);

    let index_size = index_bytes.len() as u64;
    if index_size > *buffers.index_capacity_bytes {
        *buffers.index_capacity_bytes = next_buffer_size(index_size);
        *buffers.index_buffer = create_copy_buffer(
            device,
            buffers.index_label,
            wgpu::BufferUsages::INDEX,
            *buffers.index_capacity_bytes,
        );
    }
    queue.write_buffer(buffers.index_buffer, 0, index_bytes);

    index_count
}

fn create_copy_buffer(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    size: u64,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn next_buffer_size(size: u64) -> u64 {
    size.next_power_of_two().max(BUFFER_SIZE_MIN)
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment.is_power_of_two());
    let mask = alignment - 1;
    (value + mask) & !mask
}

fn push_texture_row_as_rgba(
    out: &mut Vec<u8>,
    row: &[u8],
    format: wgpu::TextureFormat,
) -> Result<(), String> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
            out.extend_from_slice(row);
            Ok(())
        }
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
            for pixel in row.chunks_exact(4) {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
            Ok(())
        }
        other => Err(format!("unsupported export texture format: {other:?}")),
    }
}
