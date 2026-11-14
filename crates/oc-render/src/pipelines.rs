//! Bind group layouts and render pipelines. Depth is reversed (near = 1), so "in front"
//! is `GreaterEqual` and "behind" is `Less`.
use wgpu::{
    BlendState, ColorWrites, CompareFunction as Cmp, DepthBiasState, StencilFaceState,
    StencilOperation, StencilState, TextureFormat,
};

/// Format of the ID buffer: (pick word = kind << 24 | slot, index).
pub(crate) const ID_FORMAT: TextureFormat = TextureFormat::Rg32Uint;
pub(crate) const ID_DEPTH: TextureFormat = TextureFormat::Depth32Float;

const COMMON: &str = include_str!("shaders/common.wgsl");

pub(crate) struct Pipelines {
    pub frame_bgl: wgpu::BindGroupLayout,
    pub body_bgl: wgpu::BindGroupLayout,
    pub batch_bgl: wgpu::BindGroupLayout,
    pub label_bgl: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    // colour pass
    pub bg: wgpu::RenderPipeline,
    pub face: wgpu::RenderPipeline,
    pub face_blend: wgpu::RenderPipeline,
    pub preview: wgpu::RenderPipeline,
    pub preview_cut: wgpu::RenderPipeline,
    pub parity: wgpu::RenderPipeline,
    pub cap: wgpu::RenderPipeline,
    pub fill: wgpu::RenderPipeline,
    pub edge: wgpu::RenderPipeline,
    pub edge_hidden: wgpu::RenderPipeline,
    pub edge_top: wgpu::RenderPipeline,
    pub line: wgpu::RenderPipeline,
    pub line_top: wgpu::RenderPipeline,
    pub line_blend: wgpu::RenderPipeline,
    pub point_top: wgpu::RenderPipeline,
    pub label: wgpu::RenderPipeline,
    pub overlay: wgpu::RenderPipeline,
    // ID pass
    pub id_face: wgpu::RenderPipeline,
    pub id_edge: wgpu::RenderPipeline,
    pub id_vertex: wgpu::RenderPipeline,
    pub id_line: wgpu::RenderPipeline,
    pub id_point: wgpu::RenderPipeline,
    pub id_tri: wgpu::RenderPipeline,
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn buffer(uniform: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: if uniform {
            wgpu::BufferBindingType::Uniform
        } else {
            wgpu::BufferBindingType::Storage { read_only: true }
        },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

const BODY_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Snorm16x2, 2 => Uint32];
const TRI_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Uint32x2, 5 => Uint32
];
const LABEL_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
const OVERLAY_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

pub(crate) const TRI_STRIDE: u64 = 68;

fn layout(stride: u64, attributes: &'static [wgpu::VertexAttribute]) -> Option<wgpu::VertexBufferLayout<'static>> {
    Some(wgpu::VertexBufferLayout {
        array_stride: stride,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes,
    })
}

/// Depth behaviour of one pipeline.
#[derive(Clone)]
struct Depth {
    compare: Cmp,
    write: bool,
    bias: DepthBiasState,
    stencil: StencilState,
}

const NO_BIAS: DepthBiasState = DepthBiasState {
    constant: 0,
    slope_scale: 0.0,
    clamp: 0.0,
};

impl Depth {
    fn test() -> Depth {
        Depth {
            compare: Cmp::GreaterEqual,
            write: true,
            bias: NO_BIAS,
            stencil: StencilState::default(),
        }
    }
    fn no_write(mut self) -> Depth {
        self.write = false;
        self
    }
    fn always() -> Depth {
        Depth {
            compare: Cmp::Always,
            ..Depth::test().no_write()
        }
    }
    fn bias(mut self, constant: i32, slope: f32) -> Depth {
        self.bias = DepthBiasState {
            constant,
            slope_scale: slope,
            clamp: 0.0,
        };
        self
    }
}

struct Target {
    format: TextureFormat,
    depth: TextureFormat,
    samples: u32,
}

impl Pipelines {
    pub fn new(
        device: &wgpu::Device,
        color: TextureFormat,
        depth: TextureFormat,
        samples: u32,
    ) -> Pipelines {
        let module = |name: &str, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(format!("{COMMON}\n{src}").into()),
            })
        };
        let body_m = module("body", include_str!("shaders/body.wgsl"));
        let batch_m = module("batch", include_str!("shaders/batch.wgsl"));
        let misc_m = module("misc", include_str!("shaders/misc.wgsl"));
        let label_m = module("label", include_str!("shaders/label.wgsl"));

        let bgl = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let frame_bgl = bgl("frame", &[entry(0, buffer(true))]);
        let body_bgl = bgl(
            "body",
            &[
                entry(0, buffer(true)),
                entry(1, buffer(false)),
                entry(2, buffer(false)),
                entry(3, buffer(false)),
                entry(4, buffer(false)),
                entry(5, buffer(false)),
            ],
        );
        let batch_bgl = bgl("batch", &[entry(0, buffer(false)), entry(1, buffer(false))]);
        let label_bgl = bgl(
            "label",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("label"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let pl = |groups: &[&wgpu::BindGroupLayout]| {
            let g: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(|g| Some(*g)).collect();
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &g,
                immediate_size: 0,
            })
        };
        let body_l = pl(&[&frame_bgl, &body_bgl]);
        let batch_l = pl(&[&frame_bgl, &batch_bgl]);
        let misc_l = pl(&[&frame_bgl]);
        let label_l = pl(&[&frame_bgl, &label_bgl]);

        let main = Target {
            format: color,
            depth,
            samples,
        };
        let id = Target {
            format: ID_FORMAT,
            depth: ID_DEPTH,
            samples: 1,
        };
        #[allow(clippy::too_many_arguments)]
        let make = |name: &str,
                    lay: &wgpu::PipelineLayout,
                    m: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    t: &Target,
                    d: Depth,
                    blend: Option<BlendState>,
                    writes: ColorWrites| {
            let stencil = if t.depth.has_stencil_aspect() {
                d.stencil
            } else {
                StencilState::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(lay),
                vertex: wgpu::VertexState {
                    module: m,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: t.depth,
                    depth_write_enabled: Some(d.write),
                    depth_compare: Some(d.compare),
                    stencil,
                    bias: d.bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: t.samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: m,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: t.format,
                        blend,
                        write_mask: writes,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let all = ColorWrites::ALL;
        let alpha = Some(BlendState::ALPHA_BLENDING);
        let body_v = [layout(oc_mesh::VERTEX_STRIDE as u64, &BODY_ATTRS)];
        let tri_v = [layout(TRI_STRIDE, &TRI_ATTRS)];
        let label_v = [layout(20, &LABEL_ATTRS)];
        let overlay_v = [layout(24, &OVERLAY_ATTRS)];
        // Faces sit slightly behind their edges (three.js polygonOffset 1, 1; 2, 2 for
        // picking). Reverse depth: away from the eye is negative.
        let face_d = Depth::test().bias(-2, -2.0);
        let invert = StencilFaceState {
            compare: Cmp::Always,
            fail_op: StencilOperation::Keep,
            depth_fail_op: StencilOperation::Keep,
            pass_op: StencilOperation::Invert,
        };
        let parity_d = Depth {
            stencil: StencilState {
                front: invert,
                back: invert,
                read_mask: 0xff,
                write_mask: 0xff,
            },
            ..Depth::always()
        };
        let nonzero = StencilFaceState {
            compare: Cmp::NotEqual,
            ..StencilFaceState::IGNORE
        };
        let cap_d = Depth {
            stencil: StencilState {
                front: nonzero,
                back: nonzero,
                read_mask: 0xff,
                write_mask: 0,
            },
            ..Depth::test()
        };
        let (b, bt, mi, lb) = (&body_l, &batch_l, &misc_l, &label_l);
        Pipelines {
            bg: make("bg", mi, &misc_m, "vs_bg", "fs_bg", &[], &main, Depth::always(), None, all),
            face: make("face", b, &body_m, "vs_face", "fs_face", &body_v, &main, face_d.clone(), None, all),
            face_blend: make("face_blend", b, &body_m, "vs_face", "fs_face", &body_v, &main, face_d.clone().no_write(), alpha, all),
            preview: make("preview", b, &body_m, "vs_face", "fs_face", &body_v, &main, Depth::test().no_write().bias(2, 2.0), alpha, all),
            preview_cut: make("preview_cut", b, &body_m, "vs_face", "fs_face", &body_v, &main, Depth::always(), alpha, all),
            parity: make("parity", b, &body_m, "vs_face", "fs_clip_only", &body_v, &main, parity_d, None, ColorWrites::empty()),
            cap: make("cap", mi, &misc_m, "vs_tri", "fs_tri", &tri_v, &main, cap_d, None, all),
            fill: make("fill", mi, &misc_m, "vs_tri", "fs_tri", &tri_v, &main, Depth::test().no_write(), alpha, all),
            edge: make("edge", b, &body_m, "vs_edge", "fs_edge", &[], &main, Depth::test(), None, all),
            edge_hidden: make("edge_hidden", b, &body_m, "vs_edge_hidden", "fs_edge_hidden", &[], &main, Depth { compare: Cmp::Less, ..Depth::test().no_write() }, None, all),
            edge_top: make("edge_top", b, &body_m, "vs_edge", "fs_edge", &[], &main, Depth::always(), alpha, all),
            line: make("line", bt, &batch_m, "vs_line", "fs_line", &[], &main, Depth::test(), None, all),
            line_top: make("line_top", bt, &batch_m, "vs_line", "fs_line", &[], &main, Depth::always(), alpha, all),
            line_blend: make("line_blend", bt, &batch_m, "vs_line", "fs_line", &[], &main, Depth::test().no_write(), alpha, all),
            point_top: make("point_top", bt, &batch_m, "vs_point", "fs_point", &[], &main, Depth::always(), None, all),
            label: make("label", lb, &label_m, "vs_label", "fs_label", &label_v, &main, Depth::test().no_write(), alpha, all),
            overlay: make("overlay", mi, &misc_m, "vs_overlay", "fs_overlay", &overlay_v, &main, Depth::always(), alpha, all),
            id_face: make("id_face", b, &body_m, "vs_face", "fs_face_id", &body_v, &id, Depth::test().bias(-4, -3.0), None, all),
            id_edge: make("id_edge", b, &body_m, "vs_edge_pick", "fs_edge_id", &[], &id, Depth::test(), None, all),
            id_vertex: make("id_vertex", b, &body_m, "vs_vertex_pick", "fs_vertex_id", &[], &id, Depth::test(), None, all),
            id_line: make("id_line", bt, &batch_m, "vs_line_pick", "fs_line_id", &[], &id, Depth::test(), None, all),
            id_point: make("id_point", bt, &batch_m, "vs_point", "fs_point_id", &[], &id, Depth::test(), None, all),
            id_tri: make("id_tri", mi, &misc_m, "vs_tri", "fs_tri_id", &tri_v, &id, Depth::test(), None, all),
            frame_bgl,
            body_bgl,
            batch_bgl,
            label_bgl,
            sampler,
        }
    }
}
