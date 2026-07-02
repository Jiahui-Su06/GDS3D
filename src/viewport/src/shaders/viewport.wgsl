struct ViewUniform {
    eye: vec4<f32>,
    center: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> view: ViewUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let rel = input.position - view.center.xyz;
    let eye_rel = input.position - view.eye.xyz;
    let half_width = max(view.params.x, 0.001);
    let half_height = max(view.params.y, 0.001);
    let near = view.params.z;
    let far = max(view.params.w, near + 1.0);
    let depth = (dot(eye_rel, view.forward.xyz) - near) / (far - near);

    let light = normalize(vec3<f32>(0.35, -0.45, 0.82));
    let intensity = 0.58 + max(dot(normalize(input.normal), light), 0.0) * 0.42;

    var out: VertexOutput;
    out.position = vec4<f32>(
        dot(rel, view.right.xyz) / half_width,
        dot(rel, view.up.xyz) / half_height,
        depth,
        1.0,
    );
    out.color = vec4<f32>(input.color.rgb * intensity, input.color.a);
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
