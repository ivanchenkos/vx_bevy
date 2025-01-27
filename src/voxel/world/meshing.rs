use std::cell::RefCell;

use super::{
    chunks::{ChunkEntities, ChunkLoadingStage, DirtyChunks},
    Chunk, ChunkShape, Voxel, CHUNK_LENGTH,
};
use crate::voxel::{
    render::{mesh_buffer, MeshBuffers, VoxelTerrainMeshBundle},
    storage::ChunkMap,
};
use bevy::{
    prelude::*,
    render::{primitives::Aabb, render_resource::PrimitiveTopology},
    tasks::{AsyncComputeTaskPool, Task},
};
use futures_lite::future;
use once_cell::sync::Lazy;
use thread_local::ThreadLocal;

/// Attaches to the newly inserted chunk entities components required for rendering.
pub fn prepare_chunks(
    chunks: Query<(Entity, &Chunk), Added<Chunk>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cmds: Commands,
) {
    for (chunk, chunk_key) in chunks.iter() {
        cmds.entity(chunk).insert_bundle(VoxelTerrainMeshBundle {
            mesh: meshes.add(Mesh::new(PrimitiveTopology::TriangleList)),
            transform: Transform::from_translation(chunk_key.0.as_vec3()),
            visibility: Visibility { is_visible: false },
            aabb: Aabb::from_min_max(Vec3::ZERO, Vec3::splat(CHUNK_LENGTH as f32)),
            ..Default::default()
        });
    }
}

// a pool of mesh buffers shared between meshing tasks.
static SHARED_MESH_BUFFERS: Lazy<ThreadLocal<RefCell<MeshBuffers<Voxel, ChunkShape>>>> =
    Lazy::new(|| ThreadLocal::default());

/// Queues meshing tasks for the chunks in need of a remesh.
fn queue_mesh_tasks(
    mut commands: Commands,
    dirty_chunks: Res<DirtyChunks>,
    chunk_entities: Res<ChunkEntities>,
    chunks: Res<ChunkMap<Voxel, ChunkShape>>,
) {
    let task_pool = AsyncComputeTaskPool::get();
    
    dirty_chunks
        .iter_dirty()
        .filter_map(|key| {
            chunk_entities
                .entity(*key)
                .and_then(|entity| Some((key, entity)))
        })
        .filter_map(|(key, entity)| {
            chunks
                .buffer_at(*key)
                .and_then(|buffer| Some((buffer.clone(), entity)))
        })
        .map(|(buffer, entity)| {
            (
                entity,
                ChunkMeshingTask(task_pool.spawn(async move {
                    let mut mesh_buffers = SHARED_MESH_BUFFERS
                        .get_or(|| {
                            RefCell::new(MeshBuffers::<Voxel, ChunkShape>::new(ChunkShape {}))
                        })
                        .borrow_mut();

                    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList);
                    mesh_buffer(&buffer, &mut mesh_buffers, &mut mesh, 1.0);

                    mesh
                })),
            )
        })
        .for_each(|(entity, task)| {
            commands.entity(entity).insert(task);
        });
}

/// Polls and process the generated chunk meshes
fn process_mesh_tasks(
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunk_query: Query<
        (
            Entity,
            &Handle<Mesh>,
            &mut ChunkMeshingTask,
            &mut Visibility,
        ),
        With<Chunk>,
    >,
    mut commands: Commands,
) {
    chunk_query.for_each_mut(|(entity, handle, mut mesh_task, mut visibility)| {
        if let Some(mesh) = future::block_on(future::poll_once(&mut mesh_task.0)) {
            *meshes.get_mut(handle).unwrap() = mesh;
            visibility.is_visible = true;
            commands.entity(entity).remove::<ChunkMeshingTask>();
        }
    });
}

/// A stage existing solely for enabling the use of change detection.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, StageLabel)]
pub struct ChunkMeshingPrepareStage;

/// Label for the stage housing the chunk meshing systems.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, StageLabel)]
pub struct ChunkMeshingStage;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, SystemLabel)]
pub enum ChunkRenderingSystem {
    /// Queues meshing tasks for the chunks in need of a remesh.
    QueueMeshTasks,

    /// Polls and process the generated chunk meshes.
    ProcessMeshTasks,
}

/// Handles the meshing of the chunks.
pub struct VoxelWorldMeshingPlugin;

impl Plugin for VoxelWorldMeshingPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.add_stage_after(
            ChunkLoadingStage,
            ChunkMeshingPrepareStage,
            SystemStage::single(prepare_chunks),
        )
        .add_stage_after(
            ChunkMeshingPrepareStage,
            ChunkMeshingStage,
            SystemStage::parallel()
                .with_system(queue_mesh_tasks.label(ChunkRenderingSystem::QueueMeshTasks))
                .with_system(
                    process_mesh_tasks
                        .label(ChunkRenderingSystem::ProcessMeshTasks)
                        .after(ChunkRenderingSystem::QueueMeshTasks),
                ),
        );
    }
}

#[derive(Component)]
pub struct ChunkMeshingTask(Task<Mesh>);
