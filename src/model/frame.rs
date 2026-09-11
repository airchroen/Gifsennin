use std::sync::Arc;

/// 帧的稳定身份（共识 Q3）。newtype：零成本类型安全；
/// 在重排/删除/undo 后仍保持不变，选中状态与纹理缓存都以它为 key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u64);

/// 不可变帧（共识 Q3/Q4）：扁平 RGBA8 字节存储，零转换直通 GPU/编码器。
/// `Arc` 结构共享使快照式 undo 几乎免费。
#[derive(Debug)]
pub struct Frame {
    data: Arc<Vec<u8>>, // RGBA8, len == width * height * 4
    pub width: u32,
    pub height: u32,
    /// 统一时基：每帧时长 ms（GIF delay / WebP timestamp 在 codec 层归一到此）
    pub duration_ms: u32,
}

impl Frame {
    /// 校验缓冲区尺寸后构造帧。
    pub fn from_rgba(data: Vec<u8>, width: u32, height: u32, duration_ms: u32) -> Option<Self> {
        let expected = width as usize * height as usize * 4;
        if data.len() != expected {
            return None;
        }
        Some(Self {
            data: Arc::new(data),
            width,
            height,
            duration_ms,
        })
    }

    pub fn as_rgba(&self) -> &[u8] {
        &self.data
    }

    /// codec 层零拷贝访问（Arc 克隆仅计指针）
    pub fn arc_data(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.data)
    }

    pub fn memory_size(&self) -> usize {
        self.data.len()
    }

    /// 改时长：仅克隆 Arc，不复制像素（SetDuration 命令的廉价路径）
    pub fn with_duration(&self, duration_ms: u32) -> Self {
        Self {
            data: Arc::clone(&self.data),
            width: self.width,
            height: self.height,
            duration_ms,
        }
    }

    /// 转成 image crate 的缓冲（会拷贝一次，仅用于像素变换/缩略图生成）
    pub fn to_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.data.as_ref().clone())
            .expect("Frame invariant: buffer length matches dimensions")
    }
}
