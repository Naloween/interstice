use serde::{Deserialize, Serialize};

pub type GpuId = u32;

#[derive(Serialize, Deserialize, Debug)]
pub enum GpuCall {
    // ----- Info -----
    GetSurfaceFormat,
    GetLimits,

    // ----- Resources -----
    CreateBuffer(CreateBuffer),
    DestroyBuffer {
        id: GpuId,
    },
    WriteBuffer(WriteBuffer),

    CreateTexture(CreateTexture),
    DestroyTexture {
        id: GpuId,
    },
    WriteTexture(WriteTexture),
    CreateTextureView(CreateTextureView),

    CreateShaderModule(CreateShaderModule),

    CreateBindGroupLayout(CreateBindGroupLayout),
    CreateBindGroup(CreateBindGroup),
    CreatePipelineLayout(CreatePipelineLayout),

    CreateRenderPipeline(CreateRenderPipeline),
    CreateComputePipeline(CreateComputePipeline),

    // ----- Command Encoding -----
    CreateCommandEncoder,
    Submit {
        encoder: GpuId,
    },

    // Render pass
    BeginRenderPass(BeginRenderPass),
    EndRenderPass {
        pass: GpuId,
    },

    SetRenderPipeline {
        pass: GpuId,
        pipeline: GpuId,
    },
    SetBindGroup {
        pass: GpuId,
        index: u32,
        bind_group: GpuId,
    },

    SetVertexBuffer(SetVertexBuffer),
    SetIndexBuffer(SetIndexBuffer),

    Draw(Draw),
    DrawIndexed(DrawIndexed),

    // Compute pass
    BeginComputePass {
        encoder: GpuId,
    },
    EndComputePass {
        pass: GpuId,
    },
    SetComputePipeline {
        pass: GpuId,
        pipeline: GpuId,
    },
    Dispatch {
        pass: GpuId,
        x: u32,
        y: u32,
        z: u32,
    },

    // Copies
    CopyBufferToBuffer(CopyBufferToBuffer),
    CopyBufferToTexture(CopyBufferToTexture),
    CopyTextureToBuffer(CopyTextureToBuffer),

    // Surface
    GetCurrentSurfaceTexture,
    Present,
    BeginFrame,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateBuffer {
    pub size: u64,
    pub usage: BufferUsage, // your own bitflags enum
    pub mapped_at_creation: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WriteBuffer {
    pub buffer: GpuId,
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTexture {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub sample_count: u32,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUsage,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WriteTexture {
    pub texture: GpuId,
    pub data: Vec<u8>,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTextureView {
    pub texture: GpuId,
    pub format: Option<TextureFormat>,
    pub dimension: Option<TextureViewDimension>,
    pub base_mip_level: u32,
    pub mip_level_count: Option<u32>,
    pub base_array_layer: u32,
    pub array_layer_count: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateShaderModule {
    pub wgsl_source: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateBindGroupLayout {
    pub entries: Vec<BindGroupLayoutEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateBindGroup {
    pub layout: GpuId,
    pub entries: Vec<BindGroupEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreatePipelineLayout {
    pub bind_group_layouts: Vec<GpuId>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRenderPipeline {
    pub layout: GpuId,
    pub vertex: VertexState,
    pub fragment: Option<FragmentState>,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub multiview: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VertexState {
    pub module: GpuId,
    pub entry_point: String,
    pub buffers: Vec<VertexBufferLayout>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FragmentState {
    pub module: GpuId,
    pub entry_point: String,
    pub targets: Vec<ColorTargetState>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BeginRenderPass {
    pub encoder: GpuId,
    pub color_attachments: Vec<RenderPassColorAttachment>,
    pub depth_stencil: Option<RenderPassDepthStencilAttachment>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RenderPassColorAttachment {
    pub view: GpuId,
    pub resolve_target: Option<GpuId>,
    pub load: LoadOp,
    pub store: StoreOp,
    pub clear_color: [f32; 4],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SetVertexBuffer {
    pub pass: GpuId,
    pub slot: u32,
    pub buffer: GpuId,
    pub offset: u64,
    pub size: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SetIndexBuffer {
    pub pass: GpuId,
    pub buffer: GpuId,
    pub index_format: IndexFormat,
    pub offset: u64,
    pub size: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Draw {
    pub pass: GpuId,
    pub vertices: u32,
    pub instances: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DrawIndexed {
    pub pass: GpuId,
    pub indices: u32,
    pub instances: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CopyBufferToBuffer {
    pub encoder: GpuId,
    pub src: GpuId,
    pub src_offset: u64,
    pub dst: GpuId,
    pub dst_offset: u64,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateComputePipeline {
    pub layout: GpuId,
    pub module: GpuId,
    pub entry_point: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CopyBufferToTexture {
    pub encoder: GpuId,
    pub src_buffer: GpuId,
    pub src_offset: u64,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,

    pub dst_texture: GpuId,
    pub mip_level: u32,
    pub origin: [u32; 3],

    pub extent: [u32; 3],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CopyTextureToBuffer {
    pub encoder: GpuId,
    pub src_texture: GpuId,
    pub mip_level: u32,
    pub origin: [u32; 3],

    pub dst_buffer: GpuId,
    pub dst_offset: u64,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,

    pub extent: [u32; 3],
}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct BufferUsage: u32 {
        const MAP_READ      = 1 << 0;
        const MAP_WRITE     = 1 << 1;
        const COPY_SRC      = 1 << 2;
        const COPY_DST      = 1 << 3;
        const INDEX         = 1 << 4;
        const VERTEX        = 1 << 5;
        const UNIFORM       = 1 << 6;
        const STORAGE       = 1 << 7;
        const INDIRECT      = 1 << 8;
    }
}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TextureUsage: u32 {
        const COPY_SRC          = 1 << 0;
        const COPY_DST          = 1 << 1;
        const TEXTURE_BINDING   = 1 << 2;
        const STORAGE_BINDING   = 1 << 3;
        const RENDER_ATTACHMENT = 1 << 4;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum TextureDimension {
    D1,
    D2,
    D3,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum TextureViewDimension {
    D1,
    D2,
    D2Array,
    Cube,
    CubeArray,
    D3,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum TextureFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
    Depth24Plus,
    Depth32Float,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum IndexFormat {
    Uint16,
    Uint32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStage,
    pub ty: BindingType,
}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct ShaderStage: u32 {
        const VERTEX   = 1 << 0;
        const FRAGMENT = 1 << 1;
        const COMPUTE  = 1 << 2;
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BindingType {
    UniformBuffer,
    StorageBuffer {
        read_only: bool,
    },
    Sampler {
        comparison: bool,
    },
    Texture {
        sample_type: TextureSampleType,
        view_dimension: TextureViewDimension,
        multisampled: bool,
    },
    StorageTexture {
        format: TextureFormat,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TextureSampleType {
    Float { filterable: bool },
    Depth,
    Sint,
    Uint,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub resource: BindingResource,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BindingResource {
    Buffer {
        buffer: GpuId,
        offset: u64,
        size: Option<u64>,
    },
    TextureView(GpuId),
    Sampler(GpuId),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PrimitiveState {
    pub topology: PrimitiveTopology,
    pub cull_mode: Option<CullMode>,
    pub front_face: FrontFace,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PrimitiveTopology {
    TriangleList,
    TriangleStrip,
    LineList,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CullMode {
    Front,
    Back,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FrontFace {
    Ccw,
    Cw,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DepthStencilState {
    pub format: TextureFormat,
    pub depth_write_enabled: bool,
    pub depth_compare: CompareFunction,
    pub stencil: StencilState,
    pub bias: DepthBiasState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StencilState {
    /// Front face mode.
    pub front: StencilFaceState,
    /// Back face mode.
    pub back: StencilFaceState,
    /// Stencil values are AND'd with this mask when reading and writing from the stencil buffer. Only low 8 bits are used.
    pub read_mask: u32,
    /// Stencil values are AND'd with this mask when writing to the stencil buffer. Only low 8 bits are used.
    pub write_mask: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StencilFaceState {
    /// Comparison function that determines if the fail_op or pass_op is used on the stencil buffer.
    pub compare: CompareFunction,
    /// Operation that is performed when stencil test fails.
    pub fail_op: StencilOperation,
    /// Operation that is performed when depth test fails but stencil test succeeds.
    pub depth_fail_op: StencilOperation,
    /// Operation that is performed when stencil test success.
    pub pass_op: StencilOperation,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum StencilOperation {
    /// Keep stencil value unchanged.
    Keep = 0,
    /// Set stencil value to zero.
    Zero = 1,
    /// Replace stencil value with value provided in most recent call to
    /// [`RenderPass::set_stencil_reference`][RPssr].
    ///
    Replace = 2,
    /// Bitwise inverts stencil value.
    Invert = 3,
    /// Increments stencil value by one, clamping on overflow.
    IncrementClamp = 4,
    /// Decrements stencil value by one, clamping on underflow.
    DecrementClamp = 5,
    /// Increments stencil value by one, wrapping on overflow.
    IncrementWrap = 6,
    /// Decrements stencil value by one, wrapping on underflow.
    DecrementWrap = 7,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DepthBiasState {
    /// Constant depth biasing factor, in basic units of the depth format.
    pub constant: i32,
    /// Slope depth biasing factor.
    pub slope_scale: f32,
    /// Depth bias clamp value (absolute).
    pub clamp: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CompareFunction {
    Less,
    LessEqual,
    Greater,
    Always,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MultisampleState {
    pub count: u32,
    pub mask: u64,
    pub alpha_to_coverage_enabled: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VertexBufferLayout {
    pub array_stride: u64,
    pub step_mode: VertexStepMode,
    pub attributes: Vec<VertexAttribute>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VertexAttribute {
    pub format: VertexFormat,
    pub offset: u64,
    pub shader_location: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ColorTargetState {
    pub format: TextureFormat,
    pub blend: Option<BlendState>,
    pub write_mask: ColorWrites,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RenderPassDepthStencilAttachment {
    pub view: GpuId,
    pub depth_load: LoadOp,
    pub depth_store: StoreOp,
    pub depth_clear: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum LoadOp {
    Load,
    Clear,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum StoreOp {
    Store,
    Discard,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint32x2,
    Uint32x3,
    Uint32x4,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct BlendState {
    pub color: BlendComponent,
    pub alpha: BlendComponent,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct BlendComponent {
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
    pub operation: BlendOperation,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum BlendFactor {
    Zero,
    One,
    Src,
    OneMinusSrc,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum BlendOperation {
    Add,
    Subtract,
    ReverseSubtract,
}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct ColorWrites: u32 {
        const RED   = 1 << 0;
        const GREEN = 1 << 1;
        const BLUE  = 1 << 2;
        const ALPHA = 1 << 3;
        const ALL   = Self::RED.bits | Self::GREEN.bits | Self::BLUE.bits | Self::ALPHA.bits;
    }
}
