// Bevy code commonly triggers these lints and they may be important signals
// about code quality. They are sometimes hard to avoid though, and the CI
// workflow treats them as errors, so this allows them throughout the project.
// Feel free to delete this line.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use bevy::{prelude::*, reflect::TypeUuid, render::render_resource::*, window::WindowPlugin};

mod compute;

// Includes from project modules
use compute::{MyComputeShaderPlugin, MyComputeShaderRenderTarget, COMPUTE_IMG_SIZE};

// Types

#[derive(Component)]
struct MainCamera;

fn main() {
    App::new()
        //.insert_resource(ClearColor(Color::BLACK))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes: true, // Enable hot-reload
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        // uncomment for unthrottled FPS
                        // present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugin(MaterialPlugin::<CustomMaterial>::default())
        .add_plugin(MyComputeShaderPlugin)
        .add_startup_system(setup)
        .add_system(rotate_camera)
        .run();
}

fn setup(
    mut commands: Commands,
    // asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut custom_materials: ResMut<Assets<CustomMaterial>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
) {
    // Create main presentation texture and compute render target resource...
    let mut image = Image::new_fill(
        Extent3d {
            width: COMPUTE_IMG_SIZE.0,
            height: COMPUTE_IMG_SIZE.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8Unorm,
    );
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
    // ...and add it to our image asset server
    let image = images.add(image);

    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Plane::from_size(5.0).into()),
        material: standard_materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
        ..default()
    });
    commands.spawn(PointLightBundle {
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });

    commands.spawn(MaterialMeshBundle {
        mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
        transform: Transform::from_xyz(0.0, 0.5, 0.0),
        material: custom_materials.add(CustomMaterial {
            color: Color::BLUE,
            texture: image.clone(),
        }),
        ..default()
    });

    // Add image handle as a resource (of our type) to track
    commands.insert_resource(MyComputeShaderRenderTarget(image));

    // camera
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(4.0, 2.5, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        MainCamera,
    ));
}

fn rotate_camera(mut camera: Query<&mut Transform, With<MainCamera>>, time: Res<Time>) {
    let cam_transform = camera.single_mut().into_inner();

    cam_transform.rotate_around(
        Vec3::ZERO,
        Quat::from_axis_angle(Vec3::Y, 45f32.to_radians() * time.delta_seconds()),
    );
    cam_transform.look_at(Vec3::ZERO, Vec3::Y);
}

// ----------------------------------------------------------------------------
// Custom material plugin
// ----------------------------------------------------------------------------
#[derive(AsBindGroup, Debug, Clone, TypeUuid)]
#[uuid = "0fe9fd06-cbcd-4d98-9a65-d0504dbf8f09"]
pub struct CustomMaterial {
    #[uniform(0)]
    color: Color,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
}

impl Material for CustomMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/custom_material_screenspace_texture.wgsl".into()
    }
}
