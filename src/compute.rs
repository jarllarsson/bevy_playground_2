use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_graph::{RenderGraph, self},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
        RenderApp, RenderSet,
    },
};
// Moo. "clone on write", ie keep a ref until change is needed, then clone (https://doc.rust-lang.org/std/borrow/enum.Cow.html)
use std::borrow::Cow;

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

// Custom struct for tracking the render target
// Derives clone so its internals are deep copied,
// Deref to get the Image from handle (struct must be single-item for this!)
// and ExtractResource in order to be able to extract the image from bevy's main/game "world" to its render "world"
#[derive(Resource, Clone, Deref, ExtractResource)]
pub struct MyComputeShaderRenderTarget(pub Handle<Image>);


// Uniform test struct
#[derive(Resource)]
struct ComputeUniform {
    buffer: Buffer,
}

// Custom struct containing bind group of resources for our shader.
#[derive(Resource)]
struct MyComputeShaderBindGroup(BindGroup);

// Compute shader dimensions

// Total threads X*Y
pub const COMPUTE_IMG_SIZE: (u32, u32) = (640, 480);
// Threads per group X*X
const WORKGROUP_SIZE: u32 = 8;

impl Plugin for MyComputeShaderPlugin {
    // Plugin setup on app startup
    fn build(&self, app: &mut App) {
        // Extract the render target on which the compute shader needs access to.
        // From main world to render world.
        app.add_plugin(ExtractResourcePlugin::<MyComputeShaderRenderTarget>::default());

        // Fetch device
        let render_device =
            app.world.resource::<RenderDevice>();

        // Set up uniform buffer layouts
        let buffer = render_device.create_buffer(
            &BufferDescriptor {
                label: Some("time uniform buffer"),
                size: std::mem::size_of::<f32>() as u64,
                usage: BufferUsages::UNIFORM
                    | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );

        // Create our custom render pipeline and a bind group stage
        // Pipeline describes stages (shaders) of a custom graphics pipeline.
        // Bind groups binds resources to the shaders.
        let render_app = app.sub_app_mut(RenderApp); // fetch sub app "RenderApp"
        render_app
            .init_resource::<MyComputeShaderPipeline>()
            .insert_resource(ComputeUniform {
                buffer,
            })
            .add_system(queue_bind_group.in_set(RenderSet::Queue))
            .add_system(prepare_uniform.in_set(RenderSet::Prepare));

        // Create render graph node for our shader.
        // It defines the dependencies our shader and its resources has to others, and schedules it.
        let mut render_graph = render_app.world.resource_mut::<RenderGraph>();
        const MY_COMPUTE_NODE_NAME: &str = "my_compute_node";
        // Make the node
        render_graph.add_node(MY_COMPUTE_NODE_NAME, MyComputeShaderNode::default());
        // Schedule node to run before the camera node, check for OK with unwrap (panics if not)
        render_graph.add_node_edge(
            MY_COMPUTE_NODE_NAME,
            bevy::render::main_graph::node::CAMERA_DRIVER,
        );
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
    compute_uniform: Res<ComputeUniform>,
    render_target: Res<MyComputeShaderRenderTarget>,
    device: Res<RenderDevice>,
) {

    let uniform_entry = BindGroupEntry {
        binding: 0,
        resource: compute_uniform
            .buffer
            .as_entire_binding(),
    };

    // Fetch gpu view of our render target.
    // We can use * on render_target to get the handle to borrow as MyComputeShaderRenderTarget derives Deref (otherwise use .0).
    let rendertarget_gpuview = &gpu_images[&*render_target];

    let texture_entry = BindGroupEntry {
        binding: 1,
        resource: BindingResource::TextureView(&rendertarget_gpuview.texture_view),
    };

    // Bind the view to a new bind group (I assume if we have more resources we add them to the same group as make sense based on lifetimes)
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("my_rendertexture_bindgroup"),
        layout: &pipeline.texture_bind_group_layout,
        entries: &[uniform_entry, texture_entry],
    });
    commands.insert_resource(MyComputeShaderBindGroup(bind_group))
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
        let uniform = BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: BufferSize::new(std::mem::size_of::<f32>() as u64),
            },
            count: None,
        };

        let texture = BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::StorageTexture {
                access: StorageTextureAccess::ReadWrite,
                format: TextureFormat::Rgba8Unorm,
                view_dimension: TextureViewDimension::D2,
            },
            count: None,
        };
        // Define the layout of the bind group, ie. the members to bind to the shader.
        // This layout is referenced when queuing the bind group to the shader.
        let bind_group_layout =
            world
                .resource::<RenderDevice>()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("my_rendertexture_bindgroup_layout"),
                    entries: &[uniform, texture],
                });
        // Load the shader
        let shader = world
            .resource::<AssetServer>()
            .load("shaders/my_compute_shader.wgsl");
        // Create sub pipelines for our pipeline. They are created through the pipeline cache resource, keeping them cached, for efficient rendering.
        let pipeline_cache = world.resource::<PipelineCache>();
        let init_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::from("my_compute_pipeline_init")),
            layout: vec![bind_group_layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: shader.clone(),
            shader_defs: vec![],
            entry_point: Cow::from("init"),
        });
        let update_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::from("my_compute_pipeline_update")),
            layout: vec![bind_group_layout.clone()],
            push_constant_ranges: Vec::new(),
            shader,
            shader_defs: vec![],
            entry_point: Cow::from("update"),
        });

        // Construct pipeline object and return
        MyComputeShaderPipeline {
            texture_bind_group_layout: bind_group_layout,
            init_pipeline_id: init_pipeline_id,
            update_pipeline_id: update_pipeline_id,
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
        let compute_bind_group = &world.resource::<MyComputeShaderBindGroup>().0;
        let pipeline = world.resource::<MyComputeShaderPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let mut pass =
            render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("my_compute_pass"),
                });
        pass.set_bind_group(0, compute_bind_group, &[]);

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
                pass.dispatch_workgroups(
                    COMPUTE_IMG_SIZE.0 / WORKGROUP_SIZE,
                    COMPUTE_IMG_SIZE.1 / WORKGROUP_SIZE,
                    1,
                );
            }
        }
        Ok(())
    }
}

// Write our value to the uniform buffer
fn prepare_uniform(
    uniform: Res<ComputeUniform>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(
        &uniform.buffer,
        0,
        bevy::core::cast_slice(&[
            0.5_f32
        ]),
    );
}