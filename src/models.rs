//! glTF/GLB model loading for planes and mission objects.
//!
//! A plane config or a map object can name a glTF/GLB file. Loading is async, so
//! the entity gets a [`PendingModel`]; once the asset is ready we insert a
//! [`WorldAssetRoot`] pointing at the file's default scene, which spawns the
//! model as a child.

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_pending_models);
    }
}

/// An entity waiting for a glTF model to finish loading.
#[derive(Component)]
pub struct PendingModel {
    /// Asset path, relative to the client's `assets/` directory.
    pub path: String,
}

/// Queue `path` to be spawned as a child of `root` once it loads. Empty paths
/// and already-queued entities are ignored.
pub fn queue_model(commands: &mut Commands, root: Entity, path: &str) {
    if path.trim().is_empty() {
        return;
    }
    commands.entity(root).insert(PendingModel {
        path: path.to_string(),
    });
}

fn spawn_pending_models(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    gltf_assets: Res<Assets<Gltf>>,
    pending: Query<(Entity, &PendingModel)>,
) {
    for (entity, model) in &pending {
        let handle = asset_server.load::<Gltf>(model.path.clone());
        let Some(gltf) = gltf_assets.get(&handle) else {
            continue; // still loading
        };
        let scene = gltf
            .default_scene
            .clone()
            .or_else(|| gltf.scenes.first().cloned());
        match scene {
            Some(scene) => {
                commands.entity(entity).insert(WorldAssetRoot(scene));
            }
            None => warn!("glTF '{}' contains no scene", model.path),
        }
        commands.entity(entity).remove::<PendingModel>();
    }
}
