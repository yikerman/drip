//! Hot pixel removal: a foreign node that repairs isolated bright Bayer
//! samples on the host, one CFA plane at a time.
use crate::{Result, node::data::*, ports::*, registry::tag};
use rayon::prelude::*;
pub use remove::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// How far a sample may exceed its brightest same-colour neighbour before
    /// it is replaced, in normalized sensor response (1 is the white level).
    #[param(default = 0.05, range = 0.0..=1.0)]
    pub threshold: f32,
}

/// Replace hot pixels in a Bayer mosaic. A sample that exceeds the maximum of
/// its eight same-colour neighbours (the 3×3 neighbourhood of its CFA plane,
/// two sites apart) by more than the threshold becomes the median of those
/// neighbours. Edge samples compare with the neighbours that exist. `image`
/// and `output` are Bayer under one description, at full detail. A low
/// threshold also flattens small specular highlights and stars.
#[crate::node(id = "hot-pixels", name = "Hot pixel removal", tags = [tag::BAYER_RAW, tag::RECOVERY], foreign = memory)]
pub fn remove(p: &Settings, image: Read<'_, Bayer>, output: Publish<'_, Bayer>) -> Result<()> {
    let repaired = repair(image.data.host()?, &image.desc.extent, p.threshold);
    output.set(image.desc.clone(), repaired.into())
}

fn repair(samples: &[f32], extent: &Extent, threshold: f32) -> Vec<f32> {
    let (width, height) = (extent.width as usize, extent.height as usize);
    let mut repaired = samples.to_vec();
    repaired.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, sample) in row.iter_mut().enumerate() {
            let mut neighbours = [0.0; 8];
            let mut count = 0;
            for (dx, dy) in OFFSETS {
                let (nx, ny) = (x.wrapping_add_signed(dx), y.wrapping_add_signed(dy));
                if nx < width && ny < height {
                    neighbours[count] = samples[ny * width + nx];
                    count += 1;
                }
            }
            let neighbours = &mut neighbours[..count];
            let brightest = neighbours.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            if count > 0 && *sample - brightest > threshold {
                neighbours.sort_unstable_by(f32::total_cmp);
                *sample = (neighbours[(count - 1) / 2] + neighbours[count / 2]) / 2.0;
            }
        }
    });
    repaired
}

/// Same-colour neighbours in a Bayer mosaic: two sites away on each axis.
const OFFSETS: [(isize, isize); 8] =
    [(-2, -2), (0, -2), (2, -2), (-2, 0), (2, 0), (-2, 2), (0, 2), (2, 2)];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eval::Evaluator, graph::Dag, node, runtime::RuntimeContext};

    #[test]
    fn a_hot_sample_takes_its_neighbours_median_and_they_stay() {
        let extent = Extent { width: 8, height: 8 };
        let mut samples: Vec<f32> = (0..64).map(|i| 0.1 + (i % 5) as f32 * 0.01).collect();
        let hot = 3 * 8 + 4;
        samples[hot] = 0.9;
        let response =
            Response { camera: Camera { make: "Test".into(), model: "Hot".into() }, balance: None };
        let desc = BayerDesc { extent: extent.clone(), phase: BayerPhase::Rggb, response };
        let mut dag = Dag::new();
        let source = node::source::<Bayer>(&mut dag, desc.clone(), samples.clone()).unwrap();
        let hot_pixels = add(&mut dag, Settings::default()).unwrap();
        dag.connect(source, hot_pixels.image).unwrap();
        let result = Evaluator::new(RuntimeContext::host())
            .evaluate::<Bayer>(&dag, &Default::default(), hot_pixels.output)
            .unwrap();
        assert_eq!(result.desc, desc);
        let repaired = result.data.host().unwrap();
        // Same-colour neighbours of (4, 3): columns 2, 4, 6 of rows 1, 3, 5.
        let mut neighbours: Vec<f32> =
            [1, 3, 5].iter().flat_map(|y| [2, 4, 6].map(|x| samples[y * 8 + x])).collect();
        neighbours.remove(4);
        neighbours.sort_by(f32::total_cmp);
        assert_eq!(repaired[hot], (neighbours[3] + neighbours[4]) / 2.0);
        for (i, (before, after)) in samples.iter().zip(repaired.iter()).enumerate() {
            if i != hot {
                assert_eq!(before, after, "sample {i} changed");
            }
        }
    }
}
