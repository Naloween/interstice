use interstice_abi::GpuId;

use super::{ActivePass, ComputePassState, EncoderCommand, GpuState};

impl GpuState {
    pub fn create_compute_pipeline(
        &mut self,
        desc: interstice_abi::CreateComputePipeline,
    ) -> GpuId {
        let layout = self.pipeline_layouts.get(&desc.layout).unwrap();
        let shader = self.shaders.get(&desc.module).unwrap();

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(layout),
                module: shader,
                entry_point: Some(&desc.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

        let id = self.alloc_id();
        self.compute_pipelines.insert(id, pipeline);
        return id;
    }

    pub fn destroy_compute_pipeline(&mut self, id: GpuId) {
        self.compute_pipelines.remove(&id);
    }

    pub fn begin_compute_pass(&mut self, encoder: GpuId) {
        let enc = self.encoders.get_mut(&encoder).unwrap();
        assert!(enc.active_pass.is_none());

        enc.active_pass = Some(ActivePass::Compute(ComputePassState {
            desc_encoder: encoder,
            pipeline: None,
            bind_groups: Vec::new(),
            dispatches: Vec::new(),
        }));
    }

    pub fn set_compute_pipeline(&mut self, pass: GpuId, pipeline: GpuId) {
        let enc = self.encoders.get_mut(&pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Compute(cp) => cp.pipeline = Some(pipeline),
            _ => panic!("Not compute pass"),
        }
    }

    pub fn dispatch(&mut self, pass: GpuId, x: u32, y: u32, z: u32) {
        let enc = self.encoders.get_mut(&pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Compute(cp) => cp.dispatches.push([x, y, z]),
            _ => panic!("Not compute pass"),
        }
    }

    pub fn execute_compute_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cp: ComputePassState,
    ) {
        let mut pass = encoder.begin_compute_pass(&Default::default());

        if let Some(pipeline_id) = cp.pipeline {
            let pipeline = self.compute_pipelines.get(&pipeline_id).unwrap();
            pass.set_pipeline(pipeline);
        }

        for (index, bg_id) in cp.bind_groups {
            let bg = self.bind_groups.get(&bg_id).unwrap();
            pass.set_bind_group(index, bg, &[]);
        }

        for [x, y, z] in cp.dispatches {
            pass.dispatch_workgroups(x, y, z);
        }
    }

    pub fn end_compute_pass(&mut self, encoder_id: GpuId) {
        let enc = self.encoders.get_mut(&encoder_id).unwrap();

        match enc.active_pass.take() {
            Some(ActivePass::Compute(cp)) => {
                enc.commands.push(EncoderCommand::ComputePass(cp));
            }
            _ => panic!("No compute pass active"),
        }
    }
}
