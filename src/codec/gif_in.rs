//! GIF 解码。
//!
//! - image crate 的 `GifDecoder::into_frames()` 已做画布合成（delta/disposal），
//!   每帧尺寸恒等于画布；逐帧解码、逐帧累计内存，超预算立即中止。
//! - `frame.delay().numer_denom_ms()` 已是毫秒分数，`duration_ms = (num / den).round()`，
//!   不得再乘 1000；0 保留 0（播放层套 MIN_PLAYBACK_MS 下限）。
//! - image crate 不暴露循环次数；gif crate 的 `Decoder::repeat()` 在读到文件头后
//!   即可调用，故另开一个轻量解码器读取。
use super::{decode_err, DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use crate::model::{check_memory_budget, MEMORY_BUDGET_BYTES};
use image::AnimationDecoder;
use image::ImageDecoder;
use std::io::BufReader;
use std::path::Path;

pub fn load_gif(path: &Path) -> Result<DecodedAnimation, AppError> {
    // loop_count 与帧解码解耦：独立 header 解析失败只影响循环语义，不阻塞加载
    let loop_count = read_loop_count(path);

    let file = std::fs::File::open(path).map_err(|e| decode_err(path, &e))?;
    let buffered = BufReader::with_capacity(64 * 1024, file);
    let mut decoder =
        image::codecs::gif::GifDecoder::new(buffered).map_err(|e| decode_err(path, &e))?;
    let (width, height) = decoder.dimensions();

    // GifDecoder 默认 Limits::no_limits()：帧迭代器会在累计护栏生效前先分配整幅画布
    // （40 字节声明 65535² 的恶意 GIF 即可触发 ~16GiB 分配直接 abort）。把分配上限
    // 交给 image crate，超限时迭代器返回 LimitError。
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MEMORY_BUDGET_BYTES);
    decoder
        .set_limits(limits)
        .map_err(|e| decode_err(path, &e))?;

    let mut frames = Vec::new();
    let mut cum: u64 = 0;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|e| decode_err(path, &e))?;
        let (num, den) = frame.delay().numer_denom_ms();
        let duration_ms = if den == 0 {
            0
        } else {
            (f64::from(num) / f64::from(den)).round() as u32
        };
        let rgba = frame.into_buffer().into_raw();
        cum += rgba.len() as u64;
        check_memory_budget(cum)?;
        frames.push(DecodedFrame { rgba, duration_ms });
    }

    if frames.is_empty() {
        return Err(decode_err(path, "gif: no frames"));
    }

    Ok(DecodedAnimation {
        width,
        height,
        loop_count,
        frames,
    })
}

/// NETSCAPE2.0 循环次数（GIF 语义：0 = 无限）；任何失败都兜底为 0。
fn read_loop_count(path: &Path) -> u16 {
    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    let Ok(reader) = gif::Decoder::new(BufReader::new(file)) else {
        return 0;
    };
    match reader.repeat() {
        gif::Repeat::Infinite => 0,
        // Finite(0) = 无 NETSCAPE 块（只播一次）→ 规格定为 0（无限兜底）；
        // Finite(n>0) = 追加播放次数，与编码侧 Repeat::Finite(n) 对称往返
        gif::Repeat::Finite(n) => n,
    }
}
