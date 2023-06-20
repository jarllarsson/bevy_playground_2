// Bevy code commonly triggers these lints and they may be important signals
// about code quality. They are sometimes hard to avoid though, and the CI
// workflow treats them as errors, so this allows them throughout the project.
// Feel free to delete this line.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_graph::{self, RenderGraph},
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        RenderApp, RenderSet,
    },
    window::WindowPlugin,
};
// Moo. "clone on write", ie keep a ref until change is needed, then clone (https://doc.rust-lang.org/std/borrow/enum.Cow.html)
use std::borrow::Cow;

// Compute shader dimensions

// Total threads X*Y
const SIZE: (u32, u32) = (640, 480);
// Threads per group X*X
const WORKGROUP_SIZE: u32 = 8;

// Types

// Custom struct for tracking the render target
// Derives clone so its internals are deep copied,
// Deref to get the Image from handle (struct must be single-item for this!)
// and ExtractResource in order to be able to extract the image from bevy's main/game "world" to its render "world"
#[derive(Resource, Clone, Deref, ExtractResource)]
struct MyComputeShaderRenderTarget(Handle<Image>);

// Custom struct containing bind group of resources for our shader.
#[derive(Resource)]
struct  MyComputeShaderRenderTargetBindGroup(BindGroup);




fn main() {
    App::new()
        //.insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                // uncomment for unthrottled FPS
                // present_mode: bevy::window::PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugin(MyComputeShaderPlugin)
        .add_startup_system(setup)
        .run();
}

fn setup(
    mut commands: Commands, 
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn(Camera2dBundle::default());
    commands.spawn(SpriteBundle {
        texture: asset_server.load("icon.png"),
        ..Default::default()
    });

    // Create main presentation texture and compute render target resource...
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE.0,
            height: SIZE.1,
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

    // Setup the image to be rendered as a sprite to screen
    commands.spawn(SpriteBundle {
        sprite: Sprite {
            custom_size: Some(Vec2::new(SIZE.0 as f32, SIZE.1 as f32)),
            ..default()
        },
        texture: image.clone(),
        ..default()
    });

    // Add image handle as a resource (of our type) to track
    commands.insert_resource(MyComputeShaderRenderTarget(image));
}

// ----------------------------------------------------------------------------
// Compute shader plugin
// Here is where we encapsulate all our compute shader stuff.
// It instantiates our pipeline object and adds our render
// node to the graph.
//
//               [Resources]          
//                    |
//  [Shader]  [Shader bindings]
//     |              |
//     └──────────────└─[Pipeline(s)]
//                           |
//                           └─[Pipeline Resource] --> [Render Node]
//                                                           |
//                                                           └─[Render Graph]
//
//
//  Draw Render Graph -> Draw Render Node -> Draw Pipeline -> Draw Shader
// ----------------------------------------------------------------------------

pub struct MyComputeShaderPlugin;

impl Plugin for MyComputeShaderPlugin {
    // Plugin setup on app startup
    fn build(&self, app: &mut App) {
        // Extract the render target on which the compute shader needs access to.
        // From main world to render world.
        app.add_plugin(ExtractResourcePlugin::<MyComputeShaderRenderTarget>::default());

        // Create our custom render pipeline and a bind group stage
        // Pipeline describes stages (shaders) of a custom graphics pipeline.
        // Bind groups binds resources to the shaders.
        let render_app = app.sub_app_mut(RenderApp); // fetch sub app "RenderApp"
        render_app
            .init_resource::<MyComputeShaderPipeline>()
            .add_system(queue_bind_group.in_set(RenderSet::Queue));

        // Create render graph node for our shader.
        // It defines the dependencies our shader and its resources has to others, and schedules it.
        let mut render_graph = render_app.world.resource_mut::<RenderGraph>();
        const MY_COMPUTE_NODE_NAME: &str = "my_compute_node";
        // Make the node
        render_graph.add_node(MY_COMPUTE_NODE_NAME, MyComputeShaderNode::default());
        // Schedule node to run before the camera node, check for OK with unwrap (panics if not)
        render_graph.add_node_edge(MY_COMPUTE_NODE_NAME, bevy::render::main_graph::node::CAMERA_DRIVER);
    }
}

// -------------------------------------------------------------
// Bind group queueing
// Bindings for shader resources.
// -------------------------------------------------------------

// Our bind group enqueueing function/system that is added to the Bevy "Queue" render stage in the plugin setup.
// Queues the bind group that exist in the pipeline
fn queue_bind_group(
    mut commands: Commands,
    pipeline: Res<MyComputeShaderPipeline>,
    gpu_images: Res<RenderAssets<Image>>,
    render_target: Res<MyComputeShaderRenderTarget>,
    device: Res<RenderDevice>,
) {
    // Fetch gpu view of our render target.
    // We can use * on render_target to get the handle to borrow as MyComputeShaderRenderTarget derives Deref (otherwise use .0).
    let view = &gpu_images[&*render_target];
    // Bind the view to a new bind group (I assume if we have more resources we add them to the same group as make sense based on lifetimes)
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("my_rendertexture_bindgroup"),
        layout: &pipeline.texture_bind_group_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&view.texture_view),
        }],
    });
    commands.insert_resource(MyComputeShaderRenderTargetBindGroup(bind_group))
}

// -------------------------------------------------------------
// Pipeline object
// Contains information on what shaders to run and their bindings.
// -------------------------------------------------------------

// Custom struct defining the pipeline, contains references to the bind groups that binds the resources needed
// and the pipelines for initializing and updating.
#[derive(Resource)]
pub struct MyComputeShaderPipeline {
    texture_bind_group_layout: BindGroupLayout,
    init_pipeline_id: CachedComputePipelineId,
    update_pipeline_id: CachedComputePipelineId,
}

// implement the FromWorld trait on our pipeline, which allows it to
// initialize from a given world context when created as a resource to the RenderApp
impl FromWorld for MyComputeShaderPipeline {
    // Override the from_world function to do setups when given world context
    // Returns an instance of self: an initialized MyComputeShaderPipeline.
    fn from_world(world: &mut World) -> Self {
        // Setup members of struct
        // Define the layout of the bind group, ie. the members to bind to the shader.
        // This layout is referenced when queuing the bind group to the shader.
        let texture_bind_group_layout =
            world
                .resource::<RenderDevice>()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("my_rendertexture_bindgroup_layout"),
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::StorageTexture {
                            access: StorageTextureAccess::ReadWrite,
                            format: TextureFormat::Rgba8Unorm,
                            view_dimension: TextureViewDimension::D2,
                        },
                        count: None,
                    }],
                });
        // Load the shader
        let shader = world
                .resource::<AssetServer>()
                .load("shaders/my_compute_shader.wgsl");
        // Create sub pipelines for our pipeline. They are created through the pipeline cache resource, keeping them cached, for efficient rendering.
        let pipeline_cache = world.resource::<PipelineCache>();
        let init_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::from("my_compute_pipeline_init")),
            layout: vec![texture_bind_group_layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: shader.clone(),
            shader_defs: vec![],
            entry_point: Cow::from("init"),
        });
        let update_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::from("my_compute_pipeline_update")),
            layout: vec![texture_bind_group_layout.clone()],
            push_constant_ranges: Vec::new(),
            shader,
            shader_defs: vec![],
            entry_point: Cow::from("update"),
        });

        // Construct pipeline object and return
        MyComputeShaderPipeline {
            texture_bind_group_layout,
            init_pipeline_id,
            update_pipeline_id,
        }
    }
}


// -------------------------------------------------------------
// Render node
// Ties the pipeline into the Bevy render pipeline.
// The rendernode executes our stuff and is part of
// the application's render graph.
// -------------------------------------------------------------

// State of shader program
enum MyComputeShaderState {
    Loading,
    Init,
    Update,
}

struct MyComputeShaderNode {
    state: MyComputeShaderState,
}

impl Default for MyComputeShaderNode {
    fn default() -> Self {
        Self {
            state: MyComputeShaderState::Loading,
        }
    }
}

impl render_graph::Node for MyComputeShaderNode {

    // Update function of node, used to update states if the shader asset becomes loaded or has been first run-inited.
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<MyComputeShaderPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        // Handle states, we do this to make sure shaders are run when they have been loaded.
        // Match matches the pattern with the list of scrutinees, 
        // can be used as switch statement or more advanced pattern matching
        match self.state {
            MyComputeShaderState::Loading => {
                // In the loading state we check if the current cached init pipeline matches
                // the requirements of an Ok one.
                // This is done by supplying the Ok-enum of CachedPipelineState as a pattern.  (_ is used to wildcard pipeline type)
                // If it matches with the cached pipeline we query, ie. if the cached pipeline (of our type) is also the Ok value...
                // ... we change state to to Init.
                if let CachedPipelineState::Ok(_) =
                     pipeline_cache.get_compute_pipeline_state(pipeline.init_pipeline_id)
                // if pipeline_cache.get_compute_pipeline_state(pipeline.init_pipeline_id) == CachedPipelineState::Ok(_)
                {
                    self.state = MyComputeShaderState::Init;
                }
            }
            // Keep us in init state until the update pipeline is confirmed loaded as well
            MyComputeShaderState::Init => {
                if let CachedPipelineState::Ok(_) =
                     pipeline_cache.get_compute_pipeline_state(pipeline.update_pipeline_id)
                {
                    self.state = MyComputeShaderState::Update;
                }
            }
            MyComputeShaderState::Update => {} // No change from this state
        }
    }

    // Run/Dispatch shaders depending on state of node
    fn run(
            &self,
            _graph: &mut render_graph::RenderGraphContext,
            render_context: &mut RenderContext,
            world: &World,
        ) -> Result<(), render_graph::NodeRunError> {
        let texture_bind_group = &world.resource::<MyComputeShaderRenderTargetBindGroup>().0;
        let pipeline = world.resource::<MyComputeShaderPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let mut pass = render_context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("my_compute_pass")
            });
        pass.set_bind_group(0, texture_bind_group, &[]);

        // Select pipeline based on the state
        match self.state {
            MyComputeShaderState::Loading => {} // Nothing to run when loading cache...
            MyComputeShaderState::Init => {
                /*
                // Fetch the init pipeline from the cache
                let init_pipeline = pipeline_cache
                    .get_compute_pipeline(pipeline.init_pipeline_id)
                    .unwrap();
                pass.set_pipeline(init_pipeline);
                pass.dispatch_workgroups(SIZE.0 / WORKGROUP_SIZE, SIZE.1 / WORKGROUP_SIZE, 1);
            */
            }
            MyComputeShaderState::Update => {
                // Fetch the update pipeline from the cache
                let update_pipeline = pipeline_cache
                    .get_compute_pipeline(pipeline.update_pipeline_id)
                    .unwrap();
                pass.set_pipeline(update_pipeline);
                pass.dispatch_workgroups(SIZE.0 / WORKGROUP_SIZE, SIZE.1 / WORKGROUP_SIZE, 1);
            }
        }
        Ok(())
    }
}