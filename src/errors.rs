use thiserror::Error;

/// 应用级错误。跨线程以结构化形式传递（共识 Q5），
/// UI 层根据变体选择 i18n 文案展示，不再降级为裸字符串。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("decode failed: {0}")]
    Decode(String),

    #[error("encode failed: {0}")]
    Encode(String),

    /// 内存预算护栏（共识 Q4：解码后 RGBA 总量超预算时拒绝加载）
    #[error("needs {needed} bytes, budget is {budget} bytes")]
    TooLarge { needed: u64, budget: u64 },

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// i18n 文案键（render 层用 t! 翻译并插入参数）
    pub fn i18n_key(&self) -> &str {
        match self {
            Self::Io(_) => "error.io",
            Self::UnsupportedFormat(_) => "error.unsupported",
            Self::Decode(_) => "error.decode",
            Self::Encode(_) => "error.encode",
            Self::TooLarge { .. } => "error.too_large",
            Self::Other(_) => "error.other",
        }
    }
}
