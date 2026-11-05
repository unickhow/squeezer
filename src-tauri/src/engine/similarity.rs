//! Windowed SSIM helpers shared by the adaptive JPEG and PNG encoders.

const WINDOW: usize = 8;
const STRIDE: usize = 4;

/// Appends the SSIM of every 8×8 window (stride 4) of one 8-bit plane.
pub fn window_scores(
    reference: &[u8],
    candidate: &[u8],
    width: usize,
    height: usize,
    out: &mut Vec<f32>,
) {
    const C1: f64 = 6.5025; // (0.01 * 255)^2
    const C2: f64 = 58.5225; // (0.03 * 255)^2
    let count = (WINDOW * WINDOW) as f64;
    let mut y = 0;
    while y + WINDOW <= height {
        let mut x = 0;
        while x + WINDOW <= width {
            let (mut sum_a, mut sum_b) = (0u32, 0u32);
            let (mut sum_aa, mut sum_bb, mut sum_ab) = (0u32, 0u32, 0u32);
            for row in 0..WINDOW {
                let start = (y + row) * width + x;
                let a = &reference[start..start + WINDOW];
                let b = &candidate[start..start + WINDOW];
                for (a, b) in a.iter().zip(b) {
                    let (a, b) = (u32::from(*a), u32::from(*b));
                    sum_a += a;
                    sum_b += b;
                    sum_aa += a * a;
                    sum_bb += b * b;
                    sum_ab += a * b;
                }
            }
            let mean_a = f64::from(sum_a) / count;
            let mean_b = f64::from(sum_b) / count;
            let var_a = f64::from(sum_aa) / count - mean_a * mean_a;
            let var_b = f64::from(sum_bb) / count - mean_b * mean_b;
            let covariance = f64::from(sum_ab) / count - mean_a * mean_b;
            let ssim = ((2.0 * mean_a * mean_b + C1) * (2.0 * covariance + C2))
                / ((mean_a * mean_a + mean_b * mean_b + C1) * (var_a + var_b + C2));
            out.push(ssim as f32);
            x += STRIDE;
        }
        y += STRIDE;
    }
}

/// `0.6 × mean + 0.4 × mean of the worst 10 %`, so a few badly damaged regions cannot hide
/// behind large flat areas. An empty list (image smaller than one window) scores 1.0.
pub fn blend(mut scores: Vec<f32>) -> f64 {
    if scores.is_empty() {
        return 1.0;
    }
    let mean = scores.iter().map(|value| f64::from(*value)).sum::<f64>() / scores.len() as f64;
    let tail_len = (scores.len() / 10).max(1);
    scores.select_nth_unstable_by(tail_len - 1, |a, b| a.total_cmp(b));
    let tail = scores[..tail_len]
        .iter()
        .map(|value| f64::from(*value))
        .sum::<f64>()
        / tail_len as f64;
    0.6 * mean + 0.4 * tail
}

/// Blended score of a single plane.
pub fn plane_score(reference: &[u8], candidate: &[u8], width: usize, height: usize) -> f64 {
    if width < WINDOW || height < WINDOW {
        return if reference == candidate { 1.0 } else { 0.0 };
    }
    let mut scores = Vec::with_capacity(((width / STRIDE) * (height / STRIDE)).max(1));
    window_scores(reference, candidate, width, height, &mut scores);
    blend(scores)
}

#[cfg(test)]
mod tests {
    use super::plane_score;

    #[test]
    fn identical_planes_score_one_and_noise_scores_lower() {
        let flat = vec![120u8; 64 * 64];
        assert!((plane_score(&flat, &flat, 64, 64) - 1.0).abs() < 1e-9);
        let noisy: Vec<u8> = (0..64 * 64).map(|i| ((i * 37) % 251) as u8).collect();
        assert!(plane_score(&flat, &noisy, 64, 64) < 0.5);
    }
}
