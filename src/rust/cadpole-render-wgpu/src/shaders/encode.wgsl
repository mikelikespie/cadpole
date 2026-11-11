// The final resolve (`aa::resolve`): display-linear -> sRGB8 with a 2x2
// ordered dither of +-0.5 LSB. Pixels that are pure background (no sample
// covered and no contact shadow) are written as `to_srgb8(background)` with no
// dither, so user backgrounds and contact sheets stay bit-exact -- which is the
// CPU renderer's rule, and what keeps a GPU render diffable against it.

@group(1) @binding(0) var display: texture_2d<f32>;

// Bayer 2x2 thresholds, centred: offsets in [-0.375, 0.375] LSB.
const BAYER: array<f32, 4> = array<f32, 4>(-0.375, 0.125, 0.375, -0.125);

fn dither_srgb8(v: f32, offset: f32) -> f32 {
    let s = to_srgb(v);
    return floor(clamp(s * 255.0 + 0.5 + offset, 0.0, 255.0)) / 255.0;
}

@fragment
fn fs_encode(in: FsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    let px = textureLoad(display, c, 0);
    if (px.w <= 0.0) {
        let bg = G.background.xyz;
        return vec4<f32>(
            floor(clamp(to_srgb(bg.x) * 255.0 + 0.5, 0.0, 255.0)) / 255.0,
            floor(clamp(to_srgb(bg.y) * 255.0 + 0.5, 0.0, 255.0)) / 255.0,
            floor(clamp(to_srgb(bg.z) * 255.0 + 0.5, 0.0, 255.0)) / 255.0,
            1.0,
        );
    }
    let o = BAYER[(c.y & 1) * 2 + (c.x & 1)];
    return vec4<f32>(
        dither_srgb8(px.x, o),
        dither_srgb8(px.y, o),
        dither_srgb8(px.z, o),
        1.0,
    );
}

// Straight copy of the encoded image into a surface's texture. The surface is
// always viewed through a non-sRGB format (see src/surface.rs): `fs_encode`
// has already applied the transfer function, and letting the swapchain apply
// it again would correct twice.
@fragment
fn fs_blit(in: FsOut) -> @location(0) vec4<f32> {
    return textureLoad(display, vec2<i32>(i32(in.pos.x), i32(in.pos.y)), 0);
}
