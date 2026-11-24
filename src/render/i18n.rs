//! i18n：rust-i18n 编译期宏，双语文案，默认 zh-CN。
//! 文案表：locales/zh-CN.toml / locales/en.toml
//! 文案表由 lib.rs 的 `rust_i18n::i18n!("locales")` 注册（须在 crate 根），
//! 这里只提供便捷包装。

/// 动态 key 翻译（key 多由枚举映射而来，用不了编译期的 `t!` 宏）
pub fn tr(key: &str) -> String {
    rust_i18n::t!(key).to_string()
}

/// 带参数翻译：文案中 {name} 占位符逐个替换
pub fn tra(key: &str, args: &[(&str, String)]) -> String {
    let mut s = rust_i18n::t!(key).to_string();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
