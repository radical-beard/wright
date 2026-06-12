// Viewport preview shading. Deliberately echoes bestow's island_baked.slang
// semantics so what you sculpt is what the game ships: painted rockness
// blends grass→rock, the autoshader mask hands the choice to slope instead
// (steep = rock, flat = grass), tint multiplies on top. Plus editor-only
// extras: sun lambert, distance fade, and the brush ring decal.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,        // xyz = camera, w unused
    sun_dir: vec4<f32>,    // xyz = direction TO sun
    brush: vec4<f32>,      // xyz = world pos, w = radius (0 = hidden)
    brush_color: vec4<f32>,// rgb ring color, a = ring alpha
    misc: vec4<f32>,       // x = water alpha, y = time, z/w unused
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) material: vec4<f32>,
    @location(3) tint: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) material: vec4<f32>,
    @location(3) tint: vec3<f32>,
};

@vertex
fn vs_terrain(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.world = in.position;
    out.normal = in.normal;
    out.material = in.material;
    out.tint = in.tint;
    return out;
}

// Cheap value noise to break up flat color (procedural stand-in for the
// tiled grass/rock detail textures).
fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

@fragment
fn fs_terrain(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);

    // material contest — mirrors island_baked.slang:
    // slope rockness when autoshader is on, painted rockness otherwise
    let grass_slope = 0.82;
    let rock_slope = 0.62;
    let slope_rock = clamp((grass_slope - n.y) / (grass_slope - rock_slope), 0.0, 1.0);
    let rockness = mix(in.material.x, slope_rock, in.material.y);

    let detail = 0.85 + 0.3 * vnoise(in.world.xz * 0.35);
    let grass = vec3<f32>(0.30, 0.52, 0.22) * detail;
    let rock = vec3<f32>(0.45, 0.42, 0.40) * (0.8 + 0.4 * vnoise(in.world.xz * 0.8));
    // sand band just above the waterline, fading out by +1.5 m
    let sandiness = clamp(1.0 - abs(in.world.y - 0.4) / 1.1, 0.0, 1.0) * (1.0 - rockness);
    let sand = vec3<f32>(0.76, 0.70, 0.50) * detail;

    var albedo = mix(grass, rock, rockness);
    albedo = mix(albedo, sand, sandiness * in.material.y);
    albedo *= in.tint;

    let sun = max(dot(n, normalize(globals.sun_dir.xyz)), 0.0);
    let sky = 0.35 + 0.25 * max(n.y, 0.0);
    var color = albedo * (sky + sun * 0.9);

    // brush ring decal
    if globals.brush.w > 0.0 {
        let d = distance(in.world.xz, globals.brush.xz);
        let ring = smoothstep(1.5, 0.4, abs(d - globals.brush.w));
        let fill = smoothstep(globals.brush.w, 0.0, d) * 0.12;
        color = mix(color, globals.brush_color.rgb, min(ring * globals.brush_color.a + fill, 0.85));
    }

    // gentle distance haze toward the horizon color
    let dist = distance(globals.eye.xyz, in.world);
    let haze = clamp(dist / 6000.0, 0.0, 0.55);
    color = mix(color, vec3<f32>(0.65, 0.78, 0.88), haze);

    return vec4<f32>(color, 1.0);
}

// ── lines (bone skeletons, grids, gizmos) ───────────────────────────────

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(in: LineIn) -> LineOut {
    var out: LineOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

// ── water ────────────────────────────────────────────────────────────────

struct WaterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_water(@location(0) position: vec3<f32>) -> WaterOut {
    var out: WaterOut;
    out.clip = globals.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    return out;
}

@fragment
fn fs_water(in: WaterOut) -> @location(0) vec4<f32> {
    let t = globals.misc.y;
    let ripple = vnoise(in.world.xz * 0.08 + vec2<f32>(t * 0.35, t * 0.22))
        + vnoise(in.world.xz * 0.21 - vec2<f32>(t * 0.27, t * 0.31)) * 0.5;
    var deep = vec3<f32>(0.07, 0.26, 0.42);
    var shallow = vec3<f32>(0.16, 0.45, 0.58);
    var color = mix(deep, shallow, clamp(ripple * 0.6, 0.0, 1.0));

    // fresnel-ish: more reflective (lighter) at grazing angles
    let view = normalize(globals.eye.xyz - in.world);
    let fresnel = pow(1.0 - max(view.y, 0.0), 3.0);
    color = mix(color, vec3<f32>(0.65, 0.78, 0.88), fresnel * 0.7);

    // brush ring shows on the sea too (pulling land out of the water)
    if globals.brush.w > 0.0 {
        let d = distance(in.world.xz, globals.brush.xz);
        let ring = smoothstep(1.5, 0.4, abs(d - globals.brush.w));
        color = mix(color, globals.brush_color.rgb, ring * globals.brush_color.a);
    }

    return vec4<f32>(color, globals.misc.x);
}
