use super::{
    pipelines::Pipelines,
    util::{birds_eye_transforms, seam_segment_color, seam_transforms},
    SeamInfo, SeamViewScene, Vertex,
};
use crate::geo::Point3f;
use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

pub struct SeamViewSceneBundle<'a> {
    scene: &'a SeamViewScene,
    transform_bind_group: wgpu::BindGroup,
    seam_vertex_buffer: (usize, wgpu::Buffer),
}

impl<'a> SeamViewSceneBundle<'a> {
    pub fn build(
        scene: &'a SeamViewScene,
        device: &wgpu::Device,
        transform_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let (proj_matrix, view_matrix) = seam_transforms(
            &scene.camera,
            &scene.viewport,
            scene.seam.seam.edge1.projection_axis,
            scene.seam.seam.edge1.orientation,
        );

        let proj_matrix_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: cast_slice(proj_matrix.as_slice()),
            usage: wgpu::BufferUsage::UNIFORM,
        });
        let view_matrix_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: cast_slice(view_matrix.as_slice()),
            usage: wgpu::BufferUsage::UNIFORM,
        });
        let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &transform_bind_group_layout,
            entries: &[
                // u_Proj
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: proj_matrix_buffer.as_entire_binding(),
                },
                // u_View
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: view_matrix_buffer.as_entire_binding(),
                },
            ],
        });

        let seam_vertices = get_seam_vertices(&scene.seam);
        let seam_vertex_buffer = (
            seam_vertices.len(),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: cast_slice(&seam_vertices),
                usage: wgpu::BufferUsage::VERTEX,
            }),
        );

        Self {
            scene,
            transform_bind_group,
            seam_vertex_buffer,
        }
    }

    pub fn draw<'p>(
        &'p self,
        render_pass: &mut wgpu::RenderPass<'p>,
        pipelines: &'p Pipelines,
        output_size: (u32, u32),
    ) {
        let mut viewport = self.scene.viewport.clone();
        viewport.width = viewport.width.min(output_size.0 as f32 - viewport.x);
        viewport.height = viewport.height.min(output_size.1 as f32 - viewport.y);

        render_pass.set_viewport(
            viewport.x,
            viewport.y,
            viewport.width,
            viewport.height,
            0.0,
            1.0,
        );
        render_pass.set_scissor_rect(
            viewport.x as u32,
            viewport.y as u32,
            viewport.width as u32,
            viewport.height as u32,
        );

        render_pass.set_bind_group(0, &self.transform_bind_group, &[]);

        render_pass.set_pipeline(&pipelines.seam);
        render_pass.set_vertex_buffer(0, self.seam_vertex_buffer.1.slice(..));
        render_pass.draw(0..self.seam_vertex_buffer.0 as u32, 0..1);
    }
}

fn get_seam_vertices(seam_info: &SeamInfo) -> Vec<Vertex> {
    let seam = &seam_info.seam;
    let endpoint1 = Point3f::new(
        seam.endpoints.0[0] as f32,
        seam.endpoints.0[1] as f32,
        seam.endpoints.0[2] as f32,
    );
    let endpoint2 = Point3f::new(
        seam.endpoints.1[0] as f32,
        seam.endpoints.1[1] as f32,
        seam.endpoints.1[2] as f32,
    );

    vec![
        Vertex {
            pos: [endpoint1.x, endpoint1.y, endpoint1.z],
            color: [1.0, 1.0, 1.0, 1.0],
        },
        Vertex {
            pos: [endpoint2.x, endpoint2.y, endpoint2.z],
            color: [1.0, 1.0, 1.0, 1.0],
        },
        Vertex {
            pos: [endpoint1.x, endpoint1.y + 100.0, endpoint1.z],
            color: [1.0, 1.0, 1.0, 1.0],
        },
    ]
}