// CJK 字体注册（共识 Q9）：打包 Noto Sans SC 作为回退字体，
// 追加到 Proportional/Monospace 家族末尾——拉丁字形仍用 egui 默认字体，
// 中文命中回退，避免全文字形变宽。
// 另注册 Material Symbols 图标字体（独立家族，见 icons.rs）。

use super::icons;
use eframe::egui;

const NOTO_SC: &[u8] = include_bytes!("../../resources/fonts/NotoSansSC-Regular.otf");

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("noto_sc".into(), egui::FontData::from_static(NOTO_SC));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto_sc".into()); // 追加到末尾 = 回退字体
    }
    // 图标字体独立家族，但追加回退链：epaint 对每个 (字号,家族) 预查替换字形
    // '?'，纯图标字体会查不到而刷 WARN；有回退字体也能让误用的普通字符有形可显
    fonts.font_data.insert(
        "material_symbols".into(),
        egui::FontData::from_static(icons::FONT_BYTES),
    );
    let icon_family = fonts
        .families
        .entry(icons::family())
        .or_default();
    icon_family.push("material_symbols".into());
    icon_family.push("Ubuntu-Light".into());
    icon_family.push("noto_sc".into());
    ctx.set_fonts(fonts);
}
