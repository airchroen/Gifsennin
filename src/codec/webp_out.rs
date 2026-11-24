//! 动画 WebP 编码。
//!
//! - `AnimFrame::from_rgba` 借用像素缓冲，`add_frame` 只是把帧压入 encoder，
//!   像素须存活到 `try_encode()` 结束；调用方的 `frames` 切片覆盖整个编码过程，
//!   直接借用即可（零像素拷贝）。
//! - `encode()` 失败会 panic，故用 `try_encode` 并映射 `AppError::Encode`。
//!
//! ## 已知 crate 局限
//! webp-0.3.0 的 `anim_encode` 以时间戳 0 收尾，libwebp 对末帧时长取「前序帧时长均值」
//! （anim_encode.c:1542-1546）——末帧时长无法经此 API 精确保留，解码侧会得到均值近似值。
use crate::errors::AppError;
use crate::model::Frame;
use std::sync::Arc;
use webp::{AnimEncoder, AnimFrame, WebPConfig};

/// 逐帧 RGBA8 → 动画 WebP。画布尺寸取首帧，时间戳为前序帧时长累加（首帧 0）。
pub fn encode_webp(
    frames: &[Arc<Frame>],
    loop_count: u16,
    quality: f32,
    lossless: bool,
) -> Result<Vec<u8>, AppError> {
    if frames.is_empty() {
        return Err(AppError::Encode("no frames".into()));
    }
    let (canvas_w, canvas_h) = (frames[0].width, frames[0].height);

    // 帧必须与画布同尺寸：AnimEncoder 内部按画布尺寸原生读取缓冲，
    // 尺寸不符会造成越界读，这里显式拦截（model 层不变量之外的防御）。
    if frames
        .iter()
        .any(|f| f.width != canvas_w || f.height != canvas_h)
    {
        return Err(AppError::Encode("frame size mismatch with canvas".into()));
    }

    let mut config =
        WebPConfig::new().map_err(|()| AppError::Encode("WebPConfig init failed".into()))?;
    config.quality = quality.clamp(0.0, 100.0);
    config.lossless = i32::from(lossless);

    let mut encoder = AnimEncoder::new(canvas_w, canvas_h, &config);
    // 0 = 无限循环（libwebp 与 GIF 语义一致），>0 = 播放次数
    encoder.set_loop_count(i32::from(loop_count));

    let mut timestamp: i64 = 0;
    for frame in frames {
        // WebP 时间戳为 i32 ms，极端长动画累计溢出时夹到 i32::MAX
        let ts = timestamp.min(i64::from(i32::MAX)) as i32;
        encoder.add_frame(AnimFrame::from_rgba(
            frame.as_rgba(),
            canvas_w,
            canvas_h,
            ts,
        ));
        timestamp += i64::from(frame.duration_ms);
    }

    let memory = encoder
        .try_encode()
        .map_err(|e| AppError::Encode(format!("webp anim encode: {e:?}")))?;
    Ok(memory.to_vec())
}
