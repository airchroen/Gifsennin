// i18n（共识 Q9）：rust-i18n 编译期宏，双语文案，默认 zh-CN。
// 文案表：locales/zh-CN.toml / locales/en.toml
// 注意：`rust_i18n::i18n!("locales")` 在 lib.rs（crate 根）调用，
// 这里只提供便捷包装。

/// 动态 key 翻译便捷函数（render 层大量使用枚举映射出的 key）
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
