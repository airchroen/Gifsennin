// i18n 集成测试：render 层使用的所有文案键必须在 zh-CN / en 两个 locale
// 中都能解析（缺键时 rust_i18n::t! 会原样返回 key，用这一点做断言）。

use gifsennin_rust::render::i18n::{tr, tra};

/// render 层全部 tr()/tra() 键（toolbar 侧 edit.* / status.* 预留键一并入表）
const KEYS: &[&str] = &[
    // 属性栏
    "sidebar.file_info",
    "sidebar.file_name",
    "sidebar.file_size",
    "sidebar.canvas",
    "sidebar.frame_count",
    "sidebar.loop_mode",
    "sidebar.loop_infinite",
    "sidebar.selection",
    "sidebar.selected_count",
    "sidebar.duration",
    "sidebar.duration_mixed",
    "sidebar.apply_all",
    "sidebar.apply",
    "sidebar.memory",
    "sidebar.mem_frames",
    "sidebar.mem_thumbs",
    "sidebar.mem_budget",
    // 裁剪
    "crop.hint",
    "crop.selection_size",
    "crop.confirm",
    "crop.cancel",
    "crop.position",
    "crop.size",
    "crop.x",
    "crop.y",
    "crop.w",
    "crop.h",
    // 导出
    "export.title",
    "export.tab_gif",
    "export.tab_webp",
    "export.tab_png",
    "export.gif_quality_high",
    "export.gif_quality_balanced",
    "export.gif_quality_fast",
    "export.webp_quality",
    "export.webp_lossless",
    "export.png_hint",
    "export.estimate_pending",
    "export.estimate_modified",
    "export.estimate_failed",
    "export.estimate_size",
    "export.start",
    "export.cancel",
    // 缩放
    "resize.title",
    "resize.width",
    "resize.height",
    "resize.lock_aspect",
    "resize.filter",
    "resize.filter_catmull",
    "resize.filter_nearest",
    "resize.filter_triangle",
    "resize.filter_lanczos",
    "resize.apply",
    "resize.cancel",
    // 清空确认
    "confirm.title",
    "confirm.body",
    "confirm.clear",
    "confirm.cancel",
    // 编辑命令标签（工具栏/右键菜单）
    "edit.delete",
    "edit.reverse",
    "edit.reorder",
    "edit.duration",
    "edit.crop",
    "edit.resize",
    "edit.rotl",
    "edit.rotr",
    "edit.fliph",
    "edit.flipv",
    // 状态栏事件
    "status.ready",
    "status.loading",
    "status.exporting",
    "status.dialog_cancelled",
    "status.cleared",
    "status.loaded",
    "status.export_done",
    "status.error",
];

fn assert_all_resolve(locale: &str) {
    rust_i18n::set_locale(locale);
    for key in KEYS {
        let s = tr(key);
        assert_ne!(s, *key, "missing {locale} translation for {key}");
        assert!(!s.is_empty(), "empty {locale} translation for {key}");
    }
}

#[test]
fn all_render_keys_resolve_in_both_locales() {
    assert_all_resolve("zh-CN");
    assert_all_resolve("en");
    rust_i18n::set_locale("zh-CN"); // 还原默认（共识 Q9）
}

#[test]
fn tra_replaces_named_placeholders() {
    rust_i18n::set_locale("en");
    let s = tra("sidebar.selected_count", &[("n", "3".to_string())]);
    assert_eq!(s, "3 frame(s) selected");
    assert!(!s.contains("{n}"), "placeholder must be replaced: {s}");

    let s = tra("export.estimate_size", &[("size", "1.5 MB".to_string())]);
    assert_eq!(s, "~ 1.5 MB");
    rust_i18n::set_locale("zh-CN");
}
