//! WebP 解码：动画（AnimDecoder）优先，静态（Decoder）回退。
//!
//! `DecodeAnimImage` 没有 `dimensions()` 方法，画布尺寸取首帧；AnimDecoder 恒返回
//! 画布尺寸的帧，不存在子矩形帧。

use super::{decode_err, DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use crate::model::check_memory_budget;
use std::path::Path;

/// 单帧 / 零间隔 / 静态 WebP 的兜底帧时长（ms）
const FALLBACK_DURATION_MS: u32 = 100;

/// 读入整个文件后按「动画优先、静态回退」解码。
/// AnimDecoder 对静态 WebP 也常能成功（单帧），统一走动画路径；
/// 时间戳不可用时按规格回退到 100ms，结果一致。
pub fn load_webp(path: &Path) -> Result<DecodedAnimation, AppError> {
    let bytes = std::fs::read(path)?;

    // AnimDecoder 会一次性物化全部画布尺寸帧、中途无法拦截：先扫描 RIFF 结构做预算预检
    precheck_budget(&bytes)?;

    match webp::AnimDecoder::new(&bytes).decode() {
        Ok(anim) if anim.len() > 0 => decode_anim(path, &anim),
        anim => {
            // 动画解码不可用（失败或零帧）→ 静态 WebP 回退
            let anim_err = match anim {
                Ok(_) => "no frames".to_string(),
                Err(e) => e,
            };
            decode_static(path, &bytes, &anim_err)
        }
    }
}

/// RIFF/WEBP 头部预算预检：解析 VP8X 画布尺寸 + 数 ANMF 块数。
/// 解析不出（结构异常/无 VP8X）则放行——解码路径仍有事后护栏兜底。
fn precheck_budget(bytes: &[u8]) -> Result<(), AppError> {
    let u24 = |b: &[u8]| u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Ok(()); // 不是标准容器 → 交给解码器报错
    }
    let mut off = 12usize;
    let mut canvas: Option<(u32, u32)> = None;
    let mut frame_count: u64 = 0;
    while off + 8 <= bytes.len() {
        let tag = &bytes[off..off + 4];
        let size = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let data_start = off + 8;
        if tag == b"VP8X" && size >= 10 && data_start + 10 <= bytes.len() {
            let d = &bytes[data_start..];
            // VP8X：4 字节 flags/reserved + 3 字节宽-1 + 3 字节高-1（LE24）
            canvas = Some((1 + u24(&d[4..7]), 1 + u24(&d[7..10])));
        } else if tag == b"ANMF" {
            frame_count += 1;
        }
        // RIFF 块按 2 字节对齐
        off = data_start + size + (size & 1);
    }
    if let Some((w, h)) = canvas {
        let frames = frame_count.max(1);
        let projected = u64::from(w) * u64::from(h) * 4 * frames;
        check_memory_budget(projected)?;
    }
    Ok(())
}

// ─── 动画路径 ───────────────────────────────────────────────────

/// webp crate 的 `WebPAnimDecoderGetNext` 返回的是「帧结束时刻」：
/// `timestamp = prev_timestamp + duration`（anim_decode.c，初值 0），
/// 故第 i 帧时长 = `t[i] - t[i-1]`（i = 0 时 = `t[0]`）；
/// 间隔 <= 0（零时长/乱序）→ 100ms 兜底。
fn decode_anim(path: &Path, anim: &webp::DecodeAnimImage) -> Result<DecodedAnimation, AppError> {
    let frames = anim
        .get_frames(0..anim.len())
        .ok_or_else(|| decode_err(path, "webp: frame range out of bounds"))?;
    if frames.is_empty() {
        return Err(decode_err(path, "webp: no frames decoded"));
    }

    // 画布尺寸：libwebp 每帧恒为画布尺寸，取首帧即可
    let width = frames[0].width();
    let height = frames[0].height();
    if width == 0 || height == 0 {
        return Err(decode_err(
            path,
            format!("webp: invalid canvas {width}x{height}"),
        ));
    }

    let mut out: Vec<DecodedFrame> = Vec::with_capacity(frames.len());
    let mut prev_ts: i32 = 0;
    for f in &frames {
        let ts = f.get_time_ms();
        let duration_ms = frame_interval(prev_ts, ts);
        prev_ts = ts;
        let rgba = frame_to_rgba(f, width, height).map_err(|e| decode_err(path, e))?;
        out.push(DecodedFrame { rgba, duration_ms });
    }

    // 保留源文件循环次数（u16 截断）
    let loop_count = u16::try_from(anim.loop_count).unwrap_or(u16::MAX);
    finish(out, width, height, loop_count)
}

/// 相邻结束时刻差 → 帧时长（ms）；差值 <= 0（零时长/乱序）按 100ms 兜底
fn frame_interval(prev: i32, cur: i32) -> u32 {
    let d = cur.saturating_sub(prev);
    if d <= 0 {
        FALLBACK_DURATION_MS
    } else {
        d as u32
    }
}

/// 帧数据 → 画布大小 RGBA。Rgb 布局扩成 RGBA（alpha=255）；
/// 帧小于画布时铺透明底、左上覆盖（防御性路径：AnimDecoder 恒返回
/// 画布尺寸帧，正常不会走到）。
fn frame_to_rgba(f: &webp::AnimFrame, canvas_w: u32, canvas_h: u32) -> Result<Vec<u8>, AppError> {
    let (fw, fh) = (f.width(), f.height());
    let data = f.get_image();
    let (bpp, expected) = match f.get_layout() {
        webp::PixelLayout::Rgba => (4usize, fw as usize * fh as usize * 4),
        webp::PixelLayout::Rgb => (3usize, fw as usize * fh as usize * 3),
    };
    if data.len() < expected {
        return Err(AppError::Decode(format!(
            "webp: frame buffer {} bytes < expected {expected} bytes",
            data.len()
        )));
    }
    if fw > canvas_w || fh > canvas_h {
        return Err(AppError::Decode(format!(
            "webp: frame {fw}x{fh} larger than canvas {canvas_w}x{canvas_h}"
        )));
    }

    let mut out = vec![0u8; canvas_w as usize * canvas_h as usize * 4];
    if bpp == 4 {
        if fw == canvas_w && fh == canvas_h {
            out.copy_from_slice(&data[..expected]);
            return Ok(out);
        }
        // 子矩形：透明底 + 逐行左上覆盖
        let src_stride = fw as usize * 4;
        let dst_stride = canvas_w as usize * 4;
        for row in 0..fh as usize {
            let src = &data[row * src_stride..(row + 1) * src_stride];
            let off = row * dst_stride;
            out[off..off + src_stride].copy_from_slice(src);
        }
    } else {
        // Rgb → Rgba：整个画布 alpha=255，子矩形区域覆盖 RGB
        for px in out.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let src_stride = fw as usize * 3;
        let dst_stride = canvas_w as usize * 4;
        for row in 0..fh as usize {
            let src = &data[row * src_stride..(row + 1) * src_stride];
            let off = row * dst_stride;
            for x in 0..fw as usize {
                let d = &mut out[off + x * 4..off + x * 4 + 3];
                d.copy_from_slice(&src[x * 3..x * 3 + 3]);
            }
        }
    }
    Ok(out)
}

// ─── 静态回退路径 ───────────────────────────────────────────────

/// 静态 WebP 单帧解码（时长 100ms）。`anim_err` 附带动画解码失败原因。
fn decode_static(path: &Path, bytes: &[u8], anim_err: &str) -> Result<DecodedAnimation, AppError> {
    let img = webp::Decoder::new(bytes)
        .decode()
        .ok_or_else(|| decode_err(path, format!("webp decode failed (anim: {anim_err})")))?;
    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 {
        return Err(decode_err(
            path,
            format!("webp: invalid size {width}x{height}"),
        ));
    }

    // 布局决定每像素字节数；缓冲不足以覆盖整幅画布即视为损坏
    let bytes_per_pixel = if img.is_alpha() { 4 } else { 3 };
    let expected = width as usize * height as usize * bytes_per_pixel;
    if img.len() < expected {
        return Err(decode_err(
            path,
            format!(
                "webp: static buffer {} < expected {expected} bytes",
                img.len()
            ),
        ));
    }
    let rgba = if img.is_alpha() {
        img[..expected].to_vec()
    } else {
        // Rgb → Rgba（alpha=255）
        let mut out = vec![255u8; expected / 3 * 4];
        let (dst, _) = out.as_chunks_mut::<4>();
        let (src, _) = img[..expected].as_chunks::<3>();
        for (d, s) in dst.iter_mut().zip(src) {
            d[0] = s[0];
            d[1] = s[1];
            d[2] = s[2];
        }
        out
    };

    finish(
        vec![DecodedFrame {
            rgba,
            duration_ms: FALLBACK_DURATION_MS,
        }],
        width,
        height,
        0, // 静态图无循环语义
    )
}

// ─── 收尾：内存护栏 ─────────────────────────────────────────────

/// AnimDecoder 一次性物化全部帧，解码完成后立刻求和校验预算。
fn finish(
    frames: Vec<DecodedFrame>,
    width: u32,
    height: u32,
    loop_count: u16,
) -> Result<DecodedAnimation, AppError> {
    let total: u64 = frames.iter().map(|f| f.rgba.len() as u64).sum();
    check_memory_budget(total)?;
    Ok(DecodedAnimation {
        width,
        height,
        loop_count,
        frames,
    })
}
