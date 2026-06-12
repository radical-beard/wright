//! Offscreen wgpu scene: terrain chunks + water plane rendered into a
//! texture that egui displays as the viewport image. egui's own render pass
//! has no depth attachment, so a depth-tested island can't be drawn with a
//! paint callback — render-to-texture is the standard answer.

use eframe::egui_wgpu::wgpu::util::DeviceExt;
use eframe::egui_wgpu::{RenderState, wgpu};
use std::collections::HashMap;
use wright_field::{ChunkMesh, Heightfield, Masks, Mesher, Region, Vertex};

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    brush: [f32; 4],
    brush_color: [f32; 4],
    misc: [f32; 4],
}

/// Per-frame scene parameters from the mode.
pub struct SceneParams {
    pub view_proj: glam::Mat4,
    pub eye: glam::Vec3,
    /// World pos + radius of the brush cursor; radius 0 hides the ring.
    pub brush: glam::Vec4,
    pub brush_color: [f32; 4],
    pub time: f32,
}

struct GpuChunk {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Capacity in vertices, so rewrites can reuse the allocation.
    vertex_capacity: usize,
}

pub struct SceneRenderer {
    render_state: RenderState,
    terrain_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    water_vbuf: wgpu::Buffer,
    chunks: HashMap<(usize, usize), GpuChunk>,
    color: Option<wgpu::Texture>,
    depth: Option<wgpu::TextureView>,
    size: (u32, u32),
    pub texture_id: Option<eframe::egui::TextureId>,
}

impl SceneRenderer {
    pub fn new(render_state: RenderState) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wright scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wright scene"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x3],
        };

        let depth_state = wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };

        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_terrain"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_terrain"),
                compilation_options: Default::default(),
                targets: &[Some(TARGET_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(depth_state.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let water_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_water"),
                compilation_options: Default::default(),
                buffers: &[water_vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_water"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(), // no cull: visible from below
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(false), // translucent
                ..depth_state
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // one huge sea quad; vertex shader doesn't scale it, so make it vast
        let s = 100_000.0f32;
        let water_verts: [[f32; 3]; 6] = [
            [-s, 0.0, -s],
            [-s, 0.0, s],
            [s, 0.0, -s],
            [s, 0.0, -s],
            [-s, 0.0, s],
            [s, 0.0, s],
        ];
        let water_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water"),
            contents: bytemuck::cast_slice(&water_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            render_state,
            terrain_pipeline,
            water_pipeline,
            globals_buf,
            globals_bind,
            water_vbuf,
            chunks: HashMap::new(),
            color: None,
            depth: None,
            size: (0, 0),
            texture_id: None,
        }
    }

    /// Rebuild every chunk (document open / new / resolution change).
    pub fn upload_all(&mut self, field: &Heightfield, masks: &Masks) {
        self.chunks.clear();
        let mesher = Mesher::new(field);
        for coord in mesher.all_chunks() {
            let mesh = mesher.build_chunk(field, masks, coord);
            self.upload_chunk(mesh);
        }
    }

    /// Re-mesh and re-upload only the chunks a dirty region touches.
    pub fn upload_region(&mut self, field: &Heightfield, masks: &Masks, region: Region) {
        let mesher = Mesher::new(field);
        for coord in mesher.chunks_for_region(region) {
            let mesh = mesher.build_chunk(field, masks, coord);
            self.upload_chunk(mesh);
        }
    }

    fn upload_chunk(&mut self, mesh: ChunkMesh) {
        let device = &self.render_state.device;
        let queue = &self.render_state.queue;
        match self.chunks.get_mut(&mesh.coord) {
            Some(gpu) if gpu.vertex_capacity >= mesh.vertices.len() => {
                queue.write_buffer(&gpu.vertices, 0, bytemuck::cast_slice(&mesh.vertices));
                // index topology only changes with resolution; vertices
                // suffice for sculpt updates
            }
            _ => {
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("chunk verts"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("chunk idx"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                self.chunks.insert(
                    mesh.coord,
                    GpuChunk {
                        vertices,
                        indices,
                        index_count: mesh.indices.len() as u32,
                        vertex_capacity: mesh.vertices.len(),
                    },
                );
            }
        }
    }

    /// Ensure the offscreen target matches the viewport panel, (re)registering
    /// the egui texture on change.
    fn ensure_target(&mut self, width: u32, height: u32) {
        if self.size == (width, height) && self.color.is_some() {
            return;
        }
        let device = &self.render_state.device;
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());

        let mut renderer = self.render_state.renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                device,
                &color_view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id = Some(renderer.register_native_texture(
                    device,
                    &color_view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        self.color = Some(color);
        self.depth = Some(depth.create_view(&Default::default()));
        self.size = (width, height);
    }

    pub fn render(&mut self, width: u32, height: u32, params: &SceneParams) {
        if width == 0 || height == 0 {
            return;
        }
        self.ensure_target(width, height);
        let queue = &self.render_state.queue;

        let globals = Globals {
            view_proj: params.view_proj.to_cols_array_2d(),
            eye: params.eye.extend(0.0).to_array(),
            sun_dir: [0.45, 0.75, 0.3, 0.0],
            brush: params.brush.to_array(),
            brush_color: params.brush_color,
            misc: [0.82, params.time, 0.0, 0.0],
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        let color_view = self
            .color
            .as_ref()
            .unwrap()
            .create_view(&Default::default());
        let mut encoder =
            self.render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("wright viewport"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.40,
                            g: 0.58,
                            b: 0.72,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.depth.as_ref().unwrap(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_pipeline(&self.terrain_pipeline);
            for chunk in self.chunks.values() {
                pass.set_vertex_buffer(0, chunk.vertices.slice(..));
                pass.set_index_buffer(chunk.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }

            pass.set_pipeline(&self.water_pipeline);
            pass.set_vertex_buffer(0, self.water_vbuf.slice(..));
            pass.draw(0..6, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}
