pub trait ToWgpu<T> {
    fn to_wgpu(&self) -> T;
}

impl ToWgpu<wgpu::PrimitiveState> for interstice_abi::PrimitiveState {
    fn to_wgpu(&self) -> wgpu::PrimitiveState {
        wgpu::PrimitiveState {
            topology: match self.topology {
                interstice_abi::PrimitiveTopology::TriangleList => {
                    wgpu::PrimitiveTopology::TriangleList
                }
                interstice_abi::PrimitiveTopology::TriangleStrip => {
                    wgpu::PrimitiveTopology::TriangleStrip
                }
                interstice_abi::PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
                // interstice_abi::PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
                // interstice_abi::PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
            },
            strip_index_format: None,
            front_face: match self.front_face {
                interstice_abi::FrontFace::Ccw => wgpu::FrontFace::Ccw,
                interstice_abi::FrontFace::Cw => wgpu::FrontFace::Cw,
            },
            cull_mode: self.cull_mode.clone().map(|c| match c {
                interstice_abi::CullMode::Front => wgpu::Face::Front,
                interstice_abi::CullMode::Back => wgpu::Face::Back,
            }),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        }
    }
}

impl ToWgpu<wgpu::TextureFormat> for interstice_abi::TextureFormat {
    fn to_wgpu(&self) -> wgpu::TextureFormat {
        match self {
            interstice_abi::TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            interstice_abi::TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
            interstice_abi::TextureFormat::Depth24Plus => wgpu::TextureFormat::Depth24Plus,
            interstice_abi::TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            interstice_abi::TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
            interstice_abi::TextureFormat::Depth32Float => wgpu::TextureFormat::Depth32Float,
        }
    }
}

impl ToWgpu<wgpu::MultisampleState> for interstice_abi::MultisampleState {
    fn to_wgpu(&self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.count,
            mask: self.mask,
            alpha_to_coverage_enabled: self.alpha_to_coverage_enabled,
        }
    }
}

impl ToWgpu<wgpu::BindGroupLayoutEntry> for interstice_abi::BindGroupLayoutEntry {
    fn to_wgpu(&self) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding: self.binding,
            visibility: wgpu::ShaderStages::from_bits_truncate(self.visibility.bits()),
            ty: self.ty.to_wgpu(),
            count: None,
        }
    }
}

impl ToWgpu<wgpu::IndexFormat> for interstice_abi::IndexFormat {
    fn to_wgpu(&self) -> wgpu::IndexFormat {
        match self {
            interstice_abi::IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            interstice_abi::IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        }
    }
}

impl ToWgpu<wgpu::VertexFormat> for interstice_abi::VertexFormat {
    fn to_wgpu(&self) -> wgpu::VertexFormat {
        use interstice_abi::VertexFormat::*;
        match self {
            Float32 => wgpu::VertexFormat::Float32,
            Float32x2 => wgpu::VertexFormat::Float32x2,
            Float32x3 => wgpu::VertexFormat::Float32x3,
            Float32x4 => wgpu::VertexFormat::Float32x4,
            Uint32 => wgpu::VertexFormat::Uint32,
            Uint32x2 => wgpu::VertexFormat::Uint32x2,
            Uint32x3 => wgpu::VertexFormat::Uint32x3,
            Uint32x4 => wgpu::VertexFormat::Uint32x4,
            // Sint32 => wgpu::VertexFormat::Sint32,
            // Sint32x2 => wgpu::VertexFormat::Sint32x2,
            // Sint32x3 => wgpu::VertexFormat::Sint32x3,
            // Sint32x4 => wgpu::VertexFormat::Sint32x4,
        }
    }
}

impl ToWgpu<wgpu::BlendFactor> for interstice_abi::BlendFactor {
    fn to_wgpu(&self) -> wgpu::BlendFactor {
        use interstice_abi::BlendFactor::*;
        match self {
            Zero => wgpu::BlendFactor::Zero,
            One => wgpu::BlendFactor::One,
            Src => wgpu::BlendFactor::Src,
            OneMinusSrc => wgpu::BlendFactor::OneMinusSrc,
            SrcAlpha => wgpu::BlendFactor::SrcAlpha,
            OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
            // Dst => wgpu::BlendFactor::Dst,
            // OneMinusDst => wgpu::BlendFactor::OneMinusDst,
            DstAlpha => wgpu::BlendFactor::DstAlpha,
            OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        }
    }
}

impl ToWgpu<wgpu::BlendOperation> for interstice_abi::BlendOperation {
    fn to_wgpu(&self) -> wgpu::BlendOperation {
        match self {
            interstice_abi::BlendOperation::Add => wgpu::BlendOperation::Add,
            interstice_abi::BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
            interstice_abi::BlendOperation::ReverseSubtract => {
                wgpu::BlendOperation::ReverseSubtract
            } // interstice_abi::BlendOperation::Min => wgpu::BlendOperation::Min,
              // interstice_abi::BlendOperation::Max => wgpu::BlendOperation::Max,
        }
    }
}

impl ToWgpu<wgpu::BlendState> for interstice_abi::BlendState {
    fn to_wgpu(&self) -> wgpu::BlendState {
        wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: self.color.src_factor.to_wgpu(),
                dst_factor: self.color.dst_factor.to_wgpu(),
                operation: self.color.operation.to_wgpu(),
            },
            alpha: wgpu::BlendComponent {
                src_factor: self.alpha.src_factor.to_wgpu(),
                dst_factor: self.alpha.dst_factor.to_wgpu(),
                operation: self.alpha.operation.to_wgpu(),
            },
        }
    }
}

impl ToWgpu<wgpu::CompareFunction> for interstice_abi::CompareFunction {
    fn to_wgpu(&self) -> wgpu::CompareFunction {
        use interstice_abi::CompareFunction::*;
        match self {
            // Never => wgpu::CompareFunction::Never,
            Less => wgpu::CompareFunction::Less,
            LessEqual => wgpu::CompareFunction::LessEqual,
            Greater => wgpu::CompareFunction::Greater,
            // GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            // Equal => wgpu::CompareFunction::Equal,
            // NotEqual => wgpu::CompareFunction::NotEqual,
            Always => wgpu::CompareFunction::Always,
        }
    }
}

impl ToWgpu<wgpu::BindingType> for interstice_abi::BindingType {
    fn to_wgpu(&self) -> wgpu::BindingType {
        match self {
            interstice_abi::BindingType::Texture {
                sample_type,
                view_dimension,
                multisampled,
            } => wgpu::BindingType::Texture {
                sample_type: sample_type.to_wgpu(),
                view_dimension: view_dimension.to_wgpu(),
                multisampled: *multisampled,
            },
            interstice_abi::BindingType::Sampler { comparison } => {
                wgpu::BindingType::Sampler(if *comparison {
                    wgpu::SamplerBindingType::Comparison
                } else {
                    wgpu::SamplerBindingType::Filtering
                })
            }
            interstice_abi::BindingType::UniformBuffer => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            interstice_abi::BindingType::StorageBuffer { read_only } => {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: *read_only },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            }
            interstice_abi::BindingType::StorageTexture { format } => {
                wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::ReadWrite,
                    format: format.to_wgpu(),
                    view_dimension: wgpu::TextureViewDimension::D2,
                }
            }
        }
    }
}

impl ToWgpu<wgpu::TextureSampleType> for interstice_abi::TextureSampleType {
    fn to_wgpu(&self) -> wgpu::TextureSampleType {
        match self {
            interstice_abi::TextureSampleType::Float { filterable } => {
                wgpu::TextureSampleType::Float {
                    filterable: *filterable,
                }
            }
            interstice_abi::TextureSampleType::Depth => wgpu::TextureSampleType::Depth,
            interstice_abi::TextureSampleType::Sint => wgpu::TextureSampleType::Sint,
            interstice_abi::TextureSampleType::Uint => wgpu::TextureSampleType::Uint,
        }
    }
}

impl ToWgpu<wgpu::VertexStepMode> for interstice_abi::VertexStepMode {
    fn to_wgpu(&self) -> wgpu::VertexStepMode {
        match self {
            interstice_abi::VertexStepMode::Vertex => wgpu::VertexStepMode::Vertex,
            interstice_abi::VertexStepMode::Instance => wgpu::VertexStepMode::Instance,
        }
    }
}

impl ToWgpu<wgpu::TextureUsages> for interstice_abi::TextureUsage {
    fn to_wgpu(&self) -> wgpu::TextureUsages {
        let mut usage = wgpu::TextureUsages::empty();

        if self.contains(interstice_abi::TextureUsage::COPY_SRC) {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        if self.contains(interstice_abi::TextureUsage::COPY_DST) {
            usage |= wgpu::TextureUsages::COPY_DST;
        }
        if self.contains(interstice_abi::TextureUsage::TEXTURE_BINDING) {
            usage |= wgpu::TextureUsages::TEXTURE_BINDING;
        }
        if self.contains(interstice_abi::TextureUsage::STORAGE_BINDING) {
            usage |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        if self.contains(interstice_abi::TextureUsage::RENDER_ATTACHMENT) {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }

        usage
    }
}

impl ToWgpu<wgpu::TextureDimension> for interstice_abi::TextureDimension {
    fn to_wgpu(&self) -> wgpu::TextureDimension {
        match self {
            interstice_abi::TextureDimension::D1 => wgpu::TextureDimension::D1,
            interstice_abi::TextureDimension::D2 => wgpu::TextureDimension::D2,
            interstice_abi::TextureDimension::D3 => wgpu::TextureDimension::D3,
        }
    }
}

impl ToWgpu<wgpu::TextureViewDimension> for interstice_abi::TextureViewDimension {
    fn to_wgpu(&self) -> wgpu::TextureViewDimension {
        use interstice_abi::TextureViewDimension::*;
        match self {
            D1 => wgpu::TextureViewDimension::D1,
            D2 => wgpu::TextureViewDimension::D2,
            D2Array => wgpu::TextureViewDimension::D2Array,
            Cube => wgpu::TextureViewDimension::Cube,
            CubeArray => wgpu::TextureViewDimension::CubeArray,
            D3 => wgpu::TextureViewDimension::D3,
        }
    }
}

impl ToWgpu<wgpu::ColorTargetState> for interstice_abi::ColorTargetState {
    fn to_wgpu(&self) -> wgpu::ColorTargetState {
        wgpu::ColorTargetState {
            format: self.format.to_wgpu(),
            blend: self.blend.as_ref().map(|b| b.to_wgpu()),
            write_mask: wgpu::ColorWrites::from_bits_truncate(self.write_mask.bits()),
        }
    }
}

impl ToWgpu<wgpu::DepthStencilState> for interstice_abi::DepthStencilState {
    fn to_wgpu(&self) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: self.format.to_wgpu(),
            depth_write_enabled: self.depth_write_enabled,
            depth_compare: self.depth_compare.to_wgpu(),
            stencil: wgpu::StencilState {
                front: self.stencil.front.to_wgpu(),
                back: self.stencil.back.to_wgpu(),
                read_mask: self.stencil.read_mask,
                write_mask: self.stencil.write_mask,
            },
            bias: wgpu::DepthBiasState {
                constant: self.bias.constant,
                slope_scale: self.bias.slope_scale,
                clamp: self.bias.clamp,
            },
        }
    }
}

impl ToWgpu<wgpu::StencilFaceState> for interstice_abi::StencilFaceState {
    fn to_wgpu(&self) -> wgpu::StencilFaceState {
        wgpu::StencilFaceState {
            compare: self.compare.to_wgpu(),
            fail_op: self.fail_op.to_wgpu(),
            depth_fail_op: self.depth_fail_op.to_wgpu(),
            pass_op: self.pass_op.to_wgpu(),
        }
    }
}

impl ToWgpu<wgpu::StencilOperation> for interstice_abi::StencilOperation {
    fn to_wgpu(&self) -> wgpu::StencilOperation {
        match self {
            interstice_abi::StencilOperation::Keep => wgpu::StencilOperation::Keep,
            interstice_abi::StencilOperation::Zero => wgpu::StencilOperation::Zero,
            interstice_abi::StencilOperation::Replace => wgpu::StencilOperation::Replace,
            interstice_abi::StencilOperation::Invert => wgpu::StencilOperation::Invert,
            interstice_abi::StencilOperation::IncrementClamp => {
                wgpu::StencilOperation::IncrementClamp
            }
            interstice_abi::StencilOperation::DecrementClamp => {
                wgpu::StencilOperation::DecrementClamp
            }
            interstice_abi::StencilOperation::IncrementWrap => {
                wgpu::StencilOperation::IncrementWrap
            }
            interstice_abi::StencilOperation::DecrementWrap => {
                wgpu::StencilOperation::DecrementWrap
            }
        }
    }
}

impl ToWgpu<wgpu::VertexBufferLayout<'static>> for interstice_abi::VertexBufferLayout {
    fn to_wgpu(&self) -> wgpu::VertexBufferLayout<'static> {
        let attributes: Vec<wgpu::VertexAttribute> = self
            .attributes
            .iter()
            .map(|a| wgpu::VertexAttribute {
                format: a.format.to_wgpu(),
                offset: a.offset,
                shader_location: a.shader_location,
            })
            .collect();

        // Leak to extend lifetime — acceptable because pipelines live for app lifetime
        let attributes = Box::leak(attributes.into_boxed_slice());

        wgpu::VertexBufferLayout {
            array_stride: self.array_stride,
            step_mode: self.step_mode.to_wgpu(),
            attributes,
        }
    }
}

pub fn texture_format_from_wgpu(f: wgpu::TextureFormat) -> interstice_abi::TextureFormat {
    match f {
        wgpu::TextureFormat::Rgba8Unorm => interstice_abi::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb => interstice_abi::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm => interstice_abi::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb => interstice_abi::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Depth24Plus => interstice_abi::TextureFormat::Depth24Plus,
        wgpu::TextureFormat::Depth32Float => interstice_abi::TextureFormat::Depth32Float,
        _ => panic!("Unsupported texture format"),
    }
}
