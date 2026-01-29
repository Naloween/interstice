use wgpu::{SurfaceTexture, TextureView};
use winit::window::Window;

pub struct GraphicsState<'a> {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'a>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub current_frame: Option<(SurfaceTexture, TextureView)>,
    pub window: &'a Window,
}

impl<'a> GraphicsState<'a> {
    pub async fn new(window: &'a Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        Self {
            instance,
            surface,
            device,
            queue,
            config,
            current_frame: None,
            window,
        }
    }
    pub fn graphics_begin_frame(&mut self) {
        let frame = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire frame");

        let view = frame.texture.create_view(&Default::default());

        self.current_frame = Some((frame, view));
    }

    pub fn graphics_end_frame(&mut self) {
        if let Some((frame, _)) = self.current_frame.take() {
            frame.present();
        }
    }
}
