//! WebP 解码：动画（AnimDecoder）优先，静态（Decoder）回退。
//!
//! ## 已核实的 webp-0.3.0 API（源码 `~/.cargo/registry/src/*/webp-0.3.0/src/`）
//! - `AnimDecoder::new(&bytes).decode() -> Result<DecodeAnimImage, String>`：
//!   内部强制 `MODE_RGBA` 输出，且每帧均以画布尺寸物化（`animation_decoder.rs`
//!   中所有帧共用 `anim_info.canvas_width/height`），不存在子矩形帧；
//!   `DecodeAnimImage` 没有 `dimensions()` 方法 → 画布尺寸取首帧。
//! - `AnimFrame`：`get_image() -> &[u8]`、`get_layout() -> PixelLayout::{Rgb, Rgba}`、
//!   `get_time_ms() -> i32`、`width()/height() -> u32`。
//! - 静态回退：`webp::Decoder::new(&bytes).decode() -> Option<WebPImage>`，
//!   `is_alpha()` 区分 Rgba/Rgb 布局，`Deref<Target=[u8]>` 取像素字节。
//! - 源文件 loop_count 经 `DecodeAnimImage.loop_count`（pub 字段）保留（u16 截断）。

use super::{DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use std::path::Path;

/// 单帧 / 零间隔 / 静态 WebP 的兜底帧时长（ms）
const FALLBACK_DURATION_MS: u32 = 100;

/// 读入整个文件后按「动画优先、静态回退」解码。
/// AnimDecoder 对静态 WebP 也常能成功（单帧），统一走动画路径；
/// 时间戳不可用时按规格回退到 100ms，结果一致。
pub fn load_webp(path: &Path) -> Result<DecodedAnimation, AppError> {
    let bytes = std::fs::read(path)?;

    // 审查修复：AnimDecoder 内部一次性物化全部画布尺寸帧，解码完成前
    // 无法拦截——先扫描 RIFF 结构（VP8X 画布尺寸 + ANMF 帧数）做预算预检
    precheck_budget(&bytes).map_err(ctx(path))?;

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
        if projected > crate::model::MEMORY_BUDGET_BYTES {
            return Err(AppError::TooLarge {
                needed: projected,
                budget: crate::model::MEMORY_BUDGET_BYTES,
            });
        }
    }
    Ok(())
}

// ─── 动画路径 ───────────────────────────────────────────────────

/// 动画帧 → 解码结果。
/// webp crate 的 `WebPAnimDecoderGetNext` 返回的是「帧结束时刻」：
/// `timestamp = prev_timestamp + duration`（anim_decode.c，初值 0），
/// 故第 i 帧时长 = `t[i] - t[i-1]`（i = 0 时 = `t[0]`）；
/// 间隔 <= 0（零时长/乱序）→ 100ms 兜底。
fn decode_anim(path: &Path, anim: &webp::DecodeAnimImage) -> Result<DecodedAnimation, AppError> {
    let frames = anim
        .get_frames(0..anim.len())
        .ok_or_else(|| AppError::Decode("webp: frame range out of bounds".into()))
        .map_err(ctx(path))?;
    if frames.is_empty() {
        return Err(AppError::Decode("webp: no frames decoded".into())).map_err(ctx(path));
    }

    // 画布尺寸：libwebp 每帧恒为画布尺寸，取首帧即可
    let width = frames[0].width();
    let height = frames[0].height();
    if width == 0 || height == 0 {
        return Err(AppError::Decode(format!(
            "webp: invalid canvas {width}x{height}"
        )))
        .map_err(ctx(path));
    }

    let mut out: Vec<DecodedFrame> = Vec::with_capacity(frames.len());
    let mut prev_ts: i32 = 0;
    for f in frames.iter() {
        let ts = f.get_time_ms();
        let duration_ms = frame_interval(prev_ts, ts);
        prev_ts = ts;
        let rgba = frame_to_rgba(f, width, height).map_err(ctx(path))?;
        out.push(DecodedFrame { rgba, duration_ms });
    }

    // 审查修复：保留源文件循环次数（原恒 0 无限，静默改变播放语义）
    let loop_count = u16::try_from(anim.loop_count).unwrap_or(u16::MAX);
    finish(out, width, height, loop_count)
}

/// 给 Decode 错误补上路径上下文
fn ctx(path: &Path) -> impl Fn(AppError) -> AppError + '_ {
    move |e| match e {
        AppError::Decode(msg) => AppError::Decode(format!("{msg}: {}", path.display())),
        other => other,
    }
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
/// 帧小于画布时铺透明底、左上覆盖（防御性路径：核实 AnimDecoder 恒返回
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
    let img = webp::Decoder::new(bytes).decode().ok_or_else(|| {
        AppError::Decode(format!(
            "webp decode failed (anim: {anim_err}): {}",
            path.display()
        ))
    })?;
    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 {
        return Err(AppError::Decode(format!(
            "webp: invalid size {width}x{height}: {}",
            path.display()
        )));
    }

    let rgba = if img.is_alpha() {
        let expected = width as usize * height as usize * 4;
        if img.len() < expected {
            return Err(AppError::Decode(format!(
                "webp: static buffer {} < expected {expected} bytes: {}",
                img.len(),
                path.display()
            )));
        }
        img[..expected].to_vec()
    } else {
        let expected = width as usize * height as usize * 3;
        if img.len() < expected {
            return Err(AppError::Decode(format!(
                "webp: static buffer {} < expected {expected} bytes: {}",
                img.len(),
                path.display()
            )));
        }
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

// ─── 收尾：内存护栏（共识 Q4） ──────────────────────────────────

/// AnimDecoder 一次性物化全部帧，解码完成后立刻求和校验预算。
fn finish(
    frames: Vec<DecodedFrame>,
    width: u32,
    height: u32,
    loop_count: u16,
) -> Result<DecodedAnimation, AppError> {
    let total: u64 = frames.iter().map(|f| f.rgba.len() as u64).sum();
    let budget = crate::model::MEMORY_BUDGET_BYTES;
    if total > budget {
        return Err(AppError::TooLarge {
            needed: total,
            budget,
        });
    }
    Ok(DecodedAnimation {
        width,
        height,
        loop_count,
        frames,
    })
}
