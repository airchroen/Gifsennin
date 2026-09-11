// codec 层（共识 Q5）：编解码边界，纯函数。
// 所有格式在入口处归一时基为「每帧时长 ms」（共识 Q1/Q6 附注）。

pub mod gif_in;
pub mod gif_out;
pub mod png_seq;
pub mod static_in;
pub mod webp_in;
pub mod webp_out;

use crate::errors::AppError;
use crate::model::GifQuality;
use std::path::Path;

pub use gif_in::load_gif;
pub use gif_out::encode_gif;
pub use png_seq::{encode_png, encode_png_zip};
pub use static_in::load_static;
pub use webp_in::load_webp;
pub use webp_out::encode_webp;

/// 解码结果：帧已合成到画布尺寸（GIF 的 delta/disposal 在解码时处理完毕）
#[derive(Debug)]
pub struct DecodedAnimation {
    pub width: u32,
    pub height: u32,
    /// GIF 语义：0 = 无限循环；解析不到时默认 0
    pub loop_count: u16,
    pub frames: Vec<DecodedFrame>,
}

#[derive(Debug)]
pub struct DecodedFrame {
    /// 扁平 RGBA8，len == width * height * 4
    pub rgba: Vec<u8>,
    pub duration_ms: u32,
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &["gif", "png", "jpg", "jpeg", "webp", "bmp"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_lowercase();
            SUPPORTED_EXTENSIONS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// 按扩展名分派到具体解码器
pub fn load_any(path: &Path) -> Result<DecodedAnimation, AppError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "gif" => gif_in::load_gif(path),
        "webp" => webp_in::load_webp(path),
        "png" | "jpg" | "jpeg" | "bmp" => static_in::load_static(path),
        other => Err(AppError::UnsupportedFormat(other.to_string())),
    }
}

/// GIF 编码质量档位 → gif crate NeuQuant speed（1=最慢最好 … 30=最快最差）
pub fn gif_speed_for(quality: GifQuality) -> i32 {
    match quality {
        GifQuality::High => 3,
        GifQuality::Balanced => 10,
        GifQuality::Fast => 22,
    }
}
