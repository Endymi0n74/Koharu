//! Balloon-fill fast path for inpainting.
//!
//! When a mask sits inside a speech bubble with a near-uniform background,
//! the model can be skipped entirely: fill the masked pixels with the median
//! background colour of the balloon. This is purely image processing, so
//! every erase model (Lama, AoT) can use it as a pre-model pass.
//!
//! Effectiveness depends on the caller handing us one bubble at a time —
//! which is exactly what the Crop strategy does, since each crop corresponds
//! to a connected mask contour. On a whole-image forward (Resize strategy),
//! `extract_balloon_mask` usually fails to find a single containing contour
//! and we fall through to the model.

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use imageproc::{
    contours::find_contours, distance_transform::Norm, drawing::draw_polygon_mut, edges::canny,
    filter::gaussian_blur_f32, morphology::dilate, point::Point,
};

const BALLOON_CANNY_LOW: f32 = 70.0;
const BALLOON_CANNY_HIGH: f32 = 140.0;
const SIMPLE_BG_THRESHOLD_LOW_VARIANCE: f64 = 10.0;
const SIMPLE_BG_THRESHOLD_HIGH_VARIANCE: f64 = 7.0;
const SIMPLE_BG_CHANNEL_STD_SWITCH: f64 = 1.0;

type Xyxy = [u32; 4];

pub(crate) struct BalloonMasks {
    pub balloon_mask: GrayImage,
    pub non_text_mask: GrayImage,
}

/// Return an image with the masked pixels painted the balloon's median
/// background colour, iff a containing bubble with low background variance
/// can be identified. `None` means "no confident fast path; call the model".
pub fn try_fill_balloon(image: &RgbImage, mask: &GrayImage) -> Option<RgbImage> {
    let masks = extract_balloon_mask(image, mask)?;
    let average_bg_color = median_rgb(image, &masks.non_text_mask)?;
    let std_rgb = color_stddev(image, &masks.non_text_mask, average_bg_color);
    let inpaint_thresh = if stddev3(std_rgb) > SIMPLE_BG_CHANNEL_STD_SWITCH {
        SIMPLE_BG_THRESHOLD_HIGH_VARIANCE
    } else {
        SIMPLE_BG_THRESHOLD_LOW_VARIANCE
    };
    let std_max = std_rgb.into_iter().fold(0.0, f64::max);

    if std_max >= inpaint_thresh {
        return None;
    }

    let mut result = image.clone();
    let fill = [
        average_bg_color[0] as u8,
        average_bg_color[1] as u8,
        average_bg_color[2] as u8,
    ];
    for (x, y, pixel) in masks.balloon_mask.enumerate_pixels() {
        if pixel.0[0] > 0 {
            result.put_pixel(x, y, Rgb(fill));
        }
    }

    Some(result)
}

pub(crate) fn extract_balloon_mask(image: &RgbImage, mask: &GrayImage) -> Option<BalloonMasks> {
    if image.dimensions() != mask.dimensions() {
        return None;
    }

    let text_bbox = non_zero_bbox(mask)?;
    let text_sum = count_nonzero(mask);
    if text_sum == 0 {
        return None;
    }

    let gray = DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let blurred = gaussian_blur_f32(&gray, 1.0);
    let mut cannyed = canny(&blurred, BALLOON_CANNY_LOW, BALLOON_CANNY_HIGH);
    cannyed = dilate(&cannyed, Norm::LInf, 1);
    draw_binary_border(&mut cannyed);
    subtract_binary_mask(&mut cannyed, mask);

    let contours = find_contours::<i32>(&cannyed);
    let (width, height) = cannyed.dimensions();
    let mut best_mask = None;
    let mut best_area = f64::INFINITY;

    for contour in contours {
        let Some(polygon) = contour_polygon(&contour.points) else {
            continue;
        };
        let bbox = polygon_bbox(&polygon)?;
        if bbox[0] > text_bbox[0]
            || bbox[1] > text_bbox[1]
            || bbox[2] < text_bbox[2]
            || bbox[3] < text_bbox[3]
        {
            continue;
        }

        let mut candidate = GrayImage::new(width, height);
        draw_polygon_mut(&mut candidate, &polygon, Luma([255u8]));
        if count_overlap(&candidate, mask) < text_sum {
            continue;
        }

        let area = polygon_area(&polygon);
        if area < best_area {
            best_area = area;
            best_mask = Some(candidate);
        }
    }

    let balloon_mask = best_mask?;
    let mut non_text_mask = balloon_mask.clone();
    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel.0[0] > 0 {
            non_text_mask.put_pixel(x, y, Luma([0]));
        }
    }

    Some(BalloonMasks {
        balloon_mask,
        non_text_mask,
    })
}

fn contour_polygon(points: &[Point<i32>]) -> Option<Vec<Point<i32>>> {
    let mut polygon = points.to_vec();
    if polygon.len() < 3 {
        return None;
    }
    if polygon.first() == polygon.last() {
        polygon.pop();
    }
    if polygon.len() < 3 {
        return None;
    }
    Some(polygon)
}

fn polygon_bbox(points: &[Point<i32>]) -> Option<Xyxy> {
    let first = points.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points.iter().skip(1) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }

    Some([
        min_x.max(0) as u32,
        min_y.max(0) as u32,
        max_x.max(min_x).saturating_add(1) as u32,
        max_y.max(min_y).saturating_add(1) as u32,
    ])
}

fn polygon_area(points: &[Point<i32>]) -> f64 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let current = points[index];
        let next = points[(index + 1) % points.len()];
        area += f64::from(current.x) * f64::from(next.y) - f64::from(next.x) * f64::from(current.y);
    }
    area.abs() * 0.5
}

fn draw_binary_border(image: &mut GrayImage) {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return;
    }

    for x in 0..width {
        image.put_pixel(x, 0, Luma([255]));
        image.put_pixel(x, height - 1, Luma([255]));
    }
    for y in 0..height {
        image.put_pixel(0, y, Luma([255]));
        image.put_pixel(width - 1, y, Luma([255]));
    }
}

fn subtract_binary_mask(image: &mut GrayImage, mask: &GrayImage) {
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        if mask.get_pixel(x, y).0[0] > 0 {
            pixel.0[0] = 0;
        }
    }
}

fn non_zero_bbox(mask: &GrayImage) -> Option<Xyxy> {
    let (width, height) = mask.dimensions();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel.0[0] == 0 {
            continue;
        }
        found = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    found.then_some([
        min_x,
        min_y,
        max_x.saturating_add(1),
        max_y.saturating_add(1),
    ])
}

fn count_nonzero(mask: &GrayImage) -> u32 {
    mask.pixels().filter(|pixel| pixel.0[0] > 0).count() as u32
}

fn count_overlap(left: &GrayImage, right: &GrayImage) -> u32 {
    left.pixels()
        .zip(right.pixels())
        .filter(|(l, r)| l.0[0] > 0 && r.0[0] > 0)
        .count() as u32
}

fn median_rgb(image: &RgbImage, mask: &GrayImage) -> Option<[f64; 3]> {
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        channels[0].push(pixel.0[0]);
        channels[1].push(pixel.0[1]);
        channels[2].push(pixel.0[2]);
    }

    Some([
        median_channel(&channels[0])?,
        median_channel(&channels[1])?,
        median_channel(&channels[2])?,
    ])
}

fn median_channel(values: &[u8]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let mut values = values.to_vec();
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((f64::from(values[mid - 1]) + f64::from(values[mid])) / 2.0)
    } else {
        Some(f64::from(values[mid]))
    }
}

fn color_stddev(image: &RgbImage, mask: &GrayImage, median: [f64; 3]) -> [f64; 3] {
    let mut sum_sq = [0.0; 3];
    let mut count = 0.0;

    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        count += 1.0;
        for channel in 0..3 {
            let diff = f64::from(pixel.0[channel]) - median[channel];
            sum_sq[channel] += diff * diff;
        }
    }

    if count == 0.0 {
        return [f64::INFINITY; 3];
    }

    [
        (sum_sq[0] / count).sqrt(),
        (sum_sq[1] / count).sqrt(),
        (sum_sq[2] / count).sqrt(),
    ]
}

fn stddev3(values: [f64; 3]) -> f64 {
    let mean = values.iter().sum::<f64>() / 3.0;
    let variance = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / 3.0;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use imageproc::drawing::draw_hollow_rect_mut;
    use imageproc::rect::Rect;

    #[test]
    fn extract_balloon_mask_prefers_smallest_covering_contour() {
        let mut image = RgbImage::from_pixel(80, 80, Rgb([255, 255, 255]));
        draw_hollow_rect_mut(&mut image, Rect::at(4, 4).of_size(72, 72), Rgb([0, 0, 0]));
        draw_hollow_rect_mut(&mut image, Rect::at(20, 20).of_size(28, 20), Rgb([0, 0, 0]));

        let mut mask = GrayImage::new(80, 80);
        for y in 24..36 {
            for x in 24..44 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let masks = extract_balloon_mask(&image, &mask).expect("balloon should be detected");
        let balloon_pixels = count_nonzero(&masks.balloon_mask);

        assert!(
            balloon_pixels < 900,
            "expected inner contour fill, got {balloon_pixels}"
        );
        assert!(
            balloon_pixels > 250,
            "expected meaningful bubble area, got {balloon_pixels}"
        );
    }

    #[test]
    fn simple_balloon_chooses_fill_but_textured_balloon_does_not() {
        let mut flat = RgbImage::from_pixel(64, 64, Rgb([240, 240, 240]));
        draw_hollow_rect_mut(&mut flat, Rect::at(8, 8).of_size(48, 32), Rgb([0, 0, 0]));

        let mut mask = GrayImage::new(64, 64);
        for y in 18..30 {
            for x in 18..46 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        assert!(try_fill_balloon(&flat, &mask).is_some());

        let mut textured = flat.clone();
        for y in 9..39 {
            for x in 9..55 {
                let noise = ((x + y) % 23) as u8;
                textured.put_pixel(
                    x,
                    y,
                    Rgb([200 + noise, 210 + (noise / 2), 220 - (noise / 3)]),
                );
            }
        }

        assert!(try_fill_balloon(&textured, &mask).is_none());
    }
}
