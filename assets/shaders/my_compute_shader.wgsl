@group(0) @binding(0)
var texture: texture_storage_2d<rgba8unorm, read_write>;


@compute @workgroup_size(8, 8, 1)
fn init(
    @builtin(local_invocation_id) thread_idx: vec3<u32>, 
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let location = vec2<i32>(i32(thread_idx.x), i32(thread_idx.y));

    let color = vec4<f32>(vec2<f32>(thread_idx.xy) / 8.0, 0.0, 1.0);

    textureStore(texture, location, color);
}



@compute @workgroup_size(8, 8, 1)
fn update(
        @builtin(global_invocation_id) global_thread_idx: vec3<u32>,
        @builtin(local_invocation_id) thread_idx: vec3<u32>, 
        @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let location = vec2<i32>(i32(global_thread_idx.x), i32(global_thread_idx.y));

    let fLocation = vec2<f32>(f32(global_thread_idx.x) / (f32(num_workgroups.x) * 8.0), f32(global_thread_idx.y) / (f32(num_workgroups.y) * 8.0));

    let dist = distance(fLocation.xy, vec2<f32>(0.5, 0.5));
    let color = vec4<f32>(dist, dist, dist, 1.0);

    textureStore(texture, location, color);
}