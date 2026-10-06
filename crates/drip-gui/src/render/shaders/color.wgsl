// Linear RGB transforms derived from BT.709/Rec.2020 primaries and D65
// [2]–[4] (THIRD_PARTY.md). Matrices are column-major.
const TO_SRGB = mat3x3<f32>(
    vec3<f32>(1.66049100, -0.12455047, -0.01815076),
    vec3<f32>(-0.58764114, 1.13289990, -0.10057890),
    vec3<f32>(-0.07284986, -0.00834942, 1.11872966),
);
const TO_REC2020 = mat3x3<f32>(
    vec3<f32>(0.62740390, 0.06909729, 0.01639144),
    vec3<f32>(0.32928304, 0.91954040, 0.08801331),
    vec3<f32>(0.04331307, 0.01136232, 0.89559525),
);

// Extend the sRGB transfer by symmetry to preserve out-of-gamut coordinates.
fn encode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let gamma = select(1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - 0.055, a * 12.92, a <= vec3<f32>(0.0031308));
    return sign(v) * gamma;
}

fn decode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let linear = select(pow((a + 0.055) / 1.055, vec3<f32>(2.4)), a / 12.92, a <= vec3<f32>(0.04045));
    return sign(v) * linear;
}

