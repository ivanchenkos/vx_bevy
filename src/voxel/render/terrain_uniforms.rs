use std::default;

use bevy::{
    ecs::system::lifetimeless::SRes,
    prelude::{info, Color, Commands, Entity, FromWorld, Plugin, Res, ResMut},
    render::{
        render_phase::EntityRenderCommand,
        render_resource::{
            BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, ShaderStages, ShaderType,
            StorageBuffer,
        },
        renderer::{RenderDevice, RenderQueue},
        Extract, RenderApp, RenderStage,
    },
};

use crate::voxel::{material::VoxelMaterialRegistry, ChunkLoadRadius};

/// A resource wrapping buffer references and bind groups for the different uniforms used for rendering terrains
pub struct TerrainUniforms {
    pub bind_group_layout: BindGroupLayout,
    materials_buffer: StorageBuffer<GpuTerrainMaterials>,
    render_distance_params: StorageBuffer<GpuTerrainRenderSettings>,
    pub bind_group: Option<BindGroup>,
}

impl FromWorld for TerrainUniforms {
    fn from_world(world: &mut bevy::prelude::World) -> Self {
        let render_device = world.get_resource::<RenderDevice>().unwrap();
        TerrainUniforms {
            bind_group_layout: render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("voxel_engine_material_array_layout"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        ty: BindingType::Buffer {
                            has_dynamic_offset: false,
                            ty: bevy::render::render_resource::BufferBindingType::Storage {
                                read_only: true,
                            },
                            min_binding_size: None,
                        },
                        visibility: ShaderStages::VERTEX_FRAGMENT,
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        ty: BindingType::Buffer {
                            has_dynamic_offset: false,
                            ty: bevy::render::render_resource::BufferBindingType::Storage {
                                read_only: true,
                            },
                            min_binding_size: Some(GpuTerrainRenderSettings::min_size()),
                        },
                        count: None,
                        visibility: ShaderStages::VERTEX_FRAGMENT,
                    },
                ],
            }),
            materials_buffer: StorageBuffer::default(),
            render_distance_params: StorageBuffer::default(),
            bind_group: None,
        }
    }
}

/// Prepares the the bind group
fn prepare_terrain_uniforms(
    mut terrain_uniforms: ResMut<TerrainUniforms>,
    render_device: Res<RenderDevice>,
) {
    let bind_group = render_device.create_bind_group(&BindGroupDescriptor {
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: terrain_uniforms
                    .materials_buffer
                    .buffer()
                    .unwrap()
                    .as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: terrain_uniforms.render_distance_params.binding().unwrap(),
            },
        ],
        label: None,
        layout: &terrain_uniforms.bind_group_layout,
    });

    terrain_uniforms.bind_group = Some(bind_group);
}

// Materials uniform

#[derive(ShaderType, Clone, Copy, Default)]
pub struct GpuVoxelMaterial {
    pub base_color: Color,
    pub flags: u32,
    pub emissive: Color,
    pub perceptual_roughness: f32,
    pub metallic: f32,
    pub reflectance: f32,
}

#[derive(ShaderType, Clone)]
struct GpuTerrainMaterials {
    pub materials: [GpuVoxelMaterial; 256],
}

impl Default for GpuTerrainMaterials {
    fn default() -> Self {
        Self {
            materials: [Default::default(); 256],
        }
    }
}

fn extract_voxel_materials(mut commands: Commands, materials: Extract<Res<VoxelMaterialRegistry>>) {
    if materials.is_changed() {

        let mut gpu_mats = GpuTerrainMaterials {
            materials: [GpuVoxelMaterial {
                base_color: Color::WHITE,
                flags: 0,
                ..Default::default()
            }; 256],
        };

        materials
            .iter_mats()
            .enumerate()
            .for_each(|(index, material)| {
                gpu_mats.materials[index].base_color = material.base_color;
                gpu_mats.materials[index].flags = material.flags.bits();
                gpu_mats.materials[index].emissive = material.emissive;
                gpu_mats.materials[index].perceptual_roughness = material.perceptual_roughness;
                gpu_mats.materials[index].metallic = material.metallic;
                gpu_mats.materials[index].reflectance = material.reflectance;
            });

        commands.insert_resource(gpu_mats);
    }
}

fn upload_voxel_materials(
    render_queue: Res<RenderQueue>,
    render_device: Res<RenderDevice>,
    mut material_meta: ResMut<TerrainUniforms>,
    materials: Res<GpuTerrainMaterials>,
) {
    if materials.is_changed() {
        material_meta.materials_buffer.set(materials.clone());
        material_meta
            .materials_buffer
            .write_buffer(&render_device, &render_queue);
    }
}

fn extract_terrain_render_settings_uniform(
    mut commands: Commands,
    render_distance: Extract<Res<ChunkLoadRadius>>,
) {
    if render_distance.is_changed() {
        commands.insert_resource(GpuTerrainRenderSettings {
            render_distance: render_distance.horizontal as u32,
        })
    }
}

fn upload_render_distance_uniform(
    uniform: Res<GpuTerrainRenderSettings>,
    mut material_meta: ResMut<TerrainUniforms>,
    render_queue: Res<RenderQueue>,
    render_device: Res<RenderDevice>,
) {
    if uniform.is_changed() {
        material_meta.render_distance_params.set(uniform.clone());
        material_meta
            .render_distance_params
            .write_buffer(&render_device, &render_queue);
    }
}

// terrain render settings uniform
#[derive(ShaderType, Default, Clone)]
struct GpuTerrainRenderSettings {
    // current render distance radius
    pub render_distance: u32,
}

/// Binds the terrain uniforms for use in shaders.
pub struct SetTerrainUniformsBindGroup<const I: usize>;
impl<const I: usize> EntityRenderCommand for SetTerrainUniformsBindGroup<I> {
    type Param = SRes<TerrainUniforms>;

    fn render<'w>(
        _view: Entity,
        _item: Entity,
        param: bevy::ecs::system::SystemParamItem<'w, '_, Self::Param>,
        pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
    ) -> bevy::render::render_phase::RenderCommandResult {
        pass.set_bind_group(I, param.into_inner().bind_group.as_ref().unwrap(), &[]);
        bevy::render::render_phase::RenderCommandResult::Success
    }
}

/// Handles the management of uniforms and bind groups for rendering terrain.
pub struct VoxelTerrainUniformsPlugin;

impl Plugin for VoxelTerrainUniformsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.sub_app_mut(RenderApp)
            .init_resource::<TerrainUniforms>()
            .add_system_to_stage(RenderStage::Extract, extract_voxel_materials)
            .add_system_to_stage(RenderStage::Queue, prepare_terrain_uniforms)
            .add_system_to_stage(RenderStage::Prepare, upload_voxel_materials)
            .add_system_to_stage(RenderStage::Prepare, upload_render_distance_uniform)
            .add_system_to_stage(
                RenderStage::Extract,
                extract_terrain_render_settings_uniform,
            );

        info!("type size: {}", GpuVoxelMaterial::min_size().get() * 256);
    }
}
