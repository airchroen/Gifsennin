use super::frame::Frame;
use rayon::prelude::*;
use std::sync::Arc;

/// 缩放重采样滤镜（共识 Q6）：默认 CatmullRom，Nearest 保像素风
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Lanczos3,
}

impl ResizeFilter {
    pub fn to_image_filter(self) -> image::imageops::FilterType {
        match self {
            Self::Nearest => image::imageops::FilterType::Nearest,
            Self::Triangle => image::imageops::FilterType::Triangle,
            Self::CatmullRom => image::imageops::FilterType::CatmullRom,
            Self::Lanczos3 => image::imageops::FilterType::Lanczos3,
        }
    }
}

/// 把裁剪矩形夹取到画布范围内，保证 w/h ≥ 1
pub fn clamp_crop(canvas: (u32, u32), x: u32, y: u32, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let x = x.min(canvas.0.saturating_sub(1));
    let y = y.min(canvas.1.saturating_sub(1));
    let w = w.clamp(1, canvas.0 - x);
    let h = h.clamp(1, canvas.1 - y);
    (x, y, w, h)
}

fn frame_from_image(img: image::RgbaImage, duration_ms: u32) -> Frame {
    let (w, h) = img.dimensions();
    Frame::from_rgba(img.into_raw(), w, h, duration_ms)
        .expect("imageops preserves width*height*4 invariant")
}

/// 单帧裁剪（矩形已由调用方 clamp）
pub fn crop_frame(frame: &Frame, x: u32, y: u32, w: u32, h: u32) -> Frame {
    let img = frame.to_image();
    let cropped = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    frame_from_image(cropped, frame.duration_ms)
}

/// 单帧缩放
pub fn resize_frame(frame: &Frame, width: u32, height: u32, filter: ResizeFilter) -> Frame {
    let img = frame.to_image();
    let resized = image::imageops::resize(&img, width, height, filter.to_image_filter());
    frame_from_image(resized, frame.duration_ms)
}

#[derive(Debug, Clone, Copy)]
pub enum RotDir {
    Left,
    Right,
}

/// 单帧 90° 旋转（画布 w/h 互换）。
/// image crate 语义（源码已核实）：rotate90 = 顺时针，rotate270 = 逆时针。
pub fn rotate_frame(frame: &Frame, dir: RotDir) -> Frame {
    let img = frame.to_image();
    let rotated = match dir {
        RotDir::Left => image::imageops::rotate270(&img), // 逆时针
        RotDir::Right => image::imageops::rotate90(&img), // 顺时针
    };
    frame_from_image(rotated, frame.duration_ms)
}

#[derive(Debug, Clone, Copy)]
pub enum FlipAxis {
    Horizontal,
    Vertical,
}

pub fn flip_frame(frame: &Frame, axis: FlipAxis) -> Frame {
    let img = frame.to_image();
    let flipped = match axis {
        FlipAxis::Horizontal => image::imageops::flip_horizontal(&img),
        FlipAxis::Vertical => image::imageops::flip_vertical(&img),
    };
    frame_from_image(flipped, frame.duration_ms)
}

// ── 全局变换（共识 Q2：一次作用于所有帧，画布统一）─────────────────
// rayon 并行：帧间无依赖。变换产生全新 Frame（新 id），
// 由调用方建立 old→new 映射以迁移 selection/anchor。

pub fn crop_all(frames: &[Arc<Frame>], x: u32, y: u32, w: u32, h: u32) -> Vec<Frame> {
    frames
        .par_iter()
        .map(|f| crop_frame(f, x, y, w, h))
        .collect()
}

pub fn resize_all(
    frames: &[Arc<Frame>],
    width: u32,
    height: u32,
    filter: ResizeFilter,
) -> Vec<Frame> {
    frames
        .par_iter()
        .map(|f| resize_frame(f, width, height, filter))
        .collect()
}

pub fn rotate_all(frames: &[Arc<Frame>], dir: RotDir) -> Vec<Frame> {
    frames.par_iter().map(|f| rotate_frame(f, dir)).collect()
}

pub fn flip_all(frames: &[Arc<Frame>], axis: FlipAxis) -> Vec<Frame> {
    frames.par_iter().map(|f| flip_frame(f, axis)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, tag: u8) -> Frame {
        let mut data = vec![0u8; (w * h * 4) as usize];
        for (i, px) in data.chunks_exact_mut(4).enumerate() {
            px[0] = (i as u8).wrapping_add(tag);
            px[1] = (i / w as usize) as u8;
            px[2] = tag;
            px[3] = 255;
        }
        Frame::from_rgba(data, w, h, 100).unwrap()
    }

    #[test]
    fn crop_reduces_dimensions_and_preserves_pixels() {
        let f = gradient(10, 10, 7);
        let c = crop_frame(&f, 2, 3, 4, 5);
        assert_eq!((c.width, c.height), (4, 5));
        // 左上角像素对应原图 (2,3)
        let base = f.as_rgba();
        let got = c.as_rgba();
        let src = ((3 * 10 + 2) * 4) as usize;
        assert_eq!(&got[0..3], &base[src..src + 3]);
        assert_eq!(c.duration_ms, 100);
    }

    #[test]
    fn rotate_roundtrip_restores_original() {
        let f = gradient(20, 10, 3);
        let r = rotate_frame(&f, RotDir::Right);
        assert_eq!((r.width, r.height), (10, 20));
        let back = rotate_frame(&r, RotDir::Left);
        assert_eq!((back.width, back.height), (20, 10));
        assert_eq!(back.as_rgba(), f.as_rgba());
    }

    #[test]
    fn rotate_right_maps_top_left_correctly() {
        // 3×1 横条 [A B C]（r 通道 0,1,2）。顺时针 90°：
        // 原左上角 A → 新右上角（新宽为 1 即顶部），故新列自上而下仍为 [A B C]。
        let mut data = vec![0u8; 3 * 1 * 4];
        for (i, px) in data.chunks_exact_mut(4).enumerate() {
            px[0] = i as u8; // 0,1,2
            px[3] = 255;
        }
        let f = Frame::from_rgba(data, 3, 1, 10).unwrap();
        let r = rotate_frame(&f, RotDir::Right);
        assert_eq!((r.width, r.height), (1, 3));
        assert_eq!(r.as_rgba()[0], 0);
        assert_eq!(r.as_rgba()[4], 1);
        assert_eq!(r.as_rgba()[8], 2);
    }

    #[test]
    fn resize_keeps_target_dimensions() {
        let f = gradient(100, 50, 9);
        let r = resize_frame(&f, 50, 25, ResizeFilter::Nearest);
        assert_eq!((r.width, r.height), (50, 25));
    }

    #[test]
    fn clamp_crop_bounds() {
        assert_eq!(clamp_crop((100, 80), 90, 70, 50, 50), (90, 70, 10, 10));
        assert_eq!(clamp_crop((100, 80), 0, 0, 0, 0), (0, 0, 1, 1));
    }
}
