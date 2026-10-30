// The ground: the standard PBR material, with the map's marks (`marks.rs`) applied to its
// colour before lighting. Structure after Bevy's `extended_material` example.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct GroundMarks {
    area: vec4<f32>,
    dirt: vec4<f32>,
    scorch: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> marks: GroundMarks;
@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var marks_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var marks_sampler: sampler;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Sampled everywhere (derivatives need uniform control flow), applied inside the
    // playable area only: clamped edge texels would streak across the apron.
    let uv = (in.world_position.xz - marks.area.xy) / marks.area.zw;
    let m = textureSample(marks_texture, marks_sampler, uv);
    let inside = select(0.0, 1.0, all(uv >= vec2(0.0)) && all(uv <= vec2(1.0)));
    var c = pbr_input.material.base_color.rgb;
    let grain = dot(c, vec3(0.2126, 0.7152, 0.0722));
    c = mix(c, marks.dirt.rgb * grain, m.g * inside);
    c = c * mix(1.0, m.r * 2.0, inside);
    c = mix(c, c * marks.scorch.rgb, m.b * inside);
    pbr_input.material.base_color = vec4(c, pbr_input.material.base_color.a);

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}

    return out;
}
