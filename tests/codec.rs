// codec 层集成测试（共识 Q8）：编码 → 落盘 → 解码往返断言。
// 覆盖 GIF（量化容差 ±48/通道、时长 centisecond 精确保真）、
// 动画 WebP（无损像素精确、时间戳时长）、PNG 序列 zip、单帧 PNG。

use gifsennin_rust::codec::{
    encode_gif, encode_png, encode_png_zip, encode_webp, load_gif, load_webp,
};
use gifsennin_rust::model::{Frame, GifQuality};
use image::GenericImageView;
use std::path::{Path, PathBuf};

const SIZE: u32 = 32;
const CENTER: u32 = SIZE / 2;

/// 唯一临时文件（进程号 + 用途后缀），Drop 时清理
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(kind: &str, ext: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gifsennin_codec_test_{}_{}.{}",
            std::process::id(),
            kind,
            ext
        ));
        let _ = std::fs::remove_file(&path); // 清掉可能的历史残留
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 纯色 32×32 帧
fn solid_frame(rgb: [u8; 3], duration_ms: u32) -> Frame {
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = rgb[0];
        px[1] = rgb[1];
        px[2] = rgb[2];
        px[3] = 255;
    }
    Frame::from_rgba(rgba, SIZE, SIZE, duration_ms).expect("32x32x4 invariant")
}

/// 红 / 绿 / 蓝三帧，时长 100/200/300ms
fn three_frames() -> Vec<std::sync::Arc<Frame>> {
    [
        ([255u8, 0, 0], 100u32),
        ([0, 255, 0], 200),
        ([0, 0, 255], 300),
    ]
    .into_iter()
    .map(|(rgb, ms)| std::sync::Arc::new(solid_frame(rgb, ms)))
    .collect()
}

/// 帧中心像素 RGB
fn center_rgb(rgba: &[u8]) -> [u8; 3] {
    let off = ((CENTER * SIZE + CENTER) * 4) as usize;
    [rgba[off], rgba[off + 1], rgba[off + 2]]
}

/// 通道误差 ≤ tol
fn assert_color_close(got: [u8; 3], want: [u8; 3], tol: i32) {
    for i in 0..3 {
        let d = i32::from(got[i]) - i32::from(want[i]);
        assert!(
            d.abs() <= tol,
            "channel {i}: got {got:?}, want {want:?}, diff {d}"
        );
    }
}

// ── (a) GIF 往返 ────────────────────────────────────────────────

#[test]
fn gif_roundtrip_frames_durations_colors() {
    let frames = three_frames();

    // 编码到内存
    let mut buf = std::io::Cursor::new(Vec::new());
    let bytes = encode_gif(&frames, 0, GifQuality::Balanced, &mut buf).expect("gif encode");
    assert!(bytes > 0);
    assert_eq!(bytes, buf.get_ref().len() as u64);

    // 落盘后走加载路径
    let tmp = TempFile::new("gif", "gif");
    std::fs::write(tmp.path(), buf.get_ref()).expect("write temp gif");
    let anim = load_gif(tmp.path()).expect("gif decode");

    assert_eq!(anim.width, SIZE);
    assert_eq!(anim.height, SIZE);
    assert_eq!(anim.frames.len(), 3);
    assert_eq!(anim.loop_count, 0, "encoded infinite → decoded 0");

    // GIF delay 为 centisecond：100/200/300ms 精确保真（±10ms 容差）
    let durations: Vec<u32> = anim.frames.iter().map(|f| f.duration_ms).collect();
    for (got, want) in durations.iter().zip([100, 200, 300]) {
        assert!(
            (i64::from(*got) - i64::from(want)).abs() <= 10,
            "duration {got} vs {want}"
        );
    }

    // 纯色帧量化后中心色接近原色（NeuQuant 调色板误差 ≤ 48/通道）
    let want_colors = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]];
    for (frame, want) in anim.frames.iter().zip(want_colors) {
        assert_eq!(frame.rgba.len(), (SIZE * SIZE * 4) as usize);
        assert_color_close(center_rgb(&frame.rgba), want, 48);
    }
}

#[test]
fn gif_loop_count_roundtrip_finite() {
    let frames = three_frames();
    let mut buf = std::io::Cursor::new(Vec::new());
    encode_gif(&frames, 2, GifQuality::Fast, &mut buf).expect("gif encode");

    let tmp = TempFile::new("gif_loop", "gif");
    std::fs::write(tmp.path(), buf.get_ref()).expect("write temp gif");
    let anim = load_gif(tmp.path()).expect("gif decode");
    assert_eq!(anim.loop_count, 2, "NETSCAPE Finite(2) survives roundtrip");
}

// ── (b) 动画 WebP 往返 ──────────────────────────────────────────

#[test]
fn webp_roundtrip_frames_durations_colors_lossless() {
    let frames = three_frames();

    let data = encode_webp(&frames, 0, 80.0, true).expect("webp encode");
    assert!(data.len() > 0);

    let tmp = TempFile::new("webp", "webp");
    std::fs::write(tmp.path(), &data).expect("write temp webp");
    let anim = load_webp(tmp.path()).expect("webp decode");

    assert_eq!(anim.width, SIZE);
    assert_eq!(anim.height, SIZE);
    assert_eq!(anim.frames.len(), 3);

    // 时长由结束时刻差恢复：前两帧精确（100/200）；末帧时长因 webp-0.3.0
    // 以时间戳 0 收尾、libwebp 取前序均值（anim_encode.c），≈ (100+200)/2 = 150
    // （±10ms 容差）
    let durations: Vec<u32> = anim.frames.iter().map(|f| f.duration_ms).collect();
    for (got, want) in durations.iter().zip([100, 200, 150]) {
        assert!(
            (i64::from(*got) - i64::from(want)).abs() <= 10,
            "duration {got} vs {want}"
        );
    }

    // 无损编码：中心像素精确还原
    let want_colors = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]];
    for (frame, want) in anim.frames.iter().zip(want_colors) {
        assert_eq!(frame.rgba.len(), (SIZE * SIZE * 4) as usize);
        let got = center_rgb(&frame.rgba);
        assert_eq!(got, want, "lossless webp center pixel must be exact");
    }
}

// ── (c) PNG 序列 zip 往返 ───────────────────────────────────────

#[test]
fn png_zip_entries_names_and_count() {
    let frames = three_frames();
    let zip_bytes = encode_png_zip(&frames).expect("zip encode");

    let tmp = TempFile::new("zip", "zip");
    std::fs::write(tmp.path(), &zip_bytes).expect("write temp zip");

    let file = std::fs::File::open(tmp.path()).expect("open temp zip");
    let mut archive = zip::ZipArchive::new(file).expect("open zip archive");
    assert_eq!(archive.len(), 3);

    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).expect("entry").name().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "frame_0001.png".to_string(),
            "frame_0002.png".to_string(),
            "frame_0003.png".to_string(),
        ]
    );

    // 抽查第一帧内容：无损 PNG，中心像素精确
    let mut entry = archive.by_index(0).expect("entry 0");
    let mut png_bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut png_bytes).expect("read entry");
    let img = image::load_from_memory(&png_bytes).expect("decode entry png");
    assert_eq!(img.dimensions(), (SIZE, SIZE));
    let rgba = img.to_rgba8();
    let px = rgba.get_pixel(CENTER, CENTER);
    assert_eq!([px[0], px[1], px[2]], [255, 0, 0]);
}

// ── (d) 单帧 PNG 往返 ───────────────────────────────────────────

#[test]
fn png_single_frame_roundtrip() {
    let frame = solid_frame([12, 200, 77], 100);
    let png_bytes = encode_png(&frame).expect("png encode");

    let img = image::load_from_memory(&png_bytes).expect("decode png");
    assert_eq!(img.dimensions(), (SIZE, SIZE));
    let rgba = img.to_rgba8();
    let px = rgba.get_pixel(CENTER, CENTER);
    assert_eq!([px[0], px[1], px[2], px[3]], [12, 200, 77, 255]);
}

// ── 空输入护栏 ──────────────────────────────────────────────────

#[test]
fn encoders_reject_empty_input() {
    let mut buf = std::io::Cursor::new(Vec::new());
    assert!(encode_gif(&[], 0, GifQuality::Balanced, &mut buf).is_err());
    assert!(encode_webp(&[], 0, 80.0, false).is_err());
    assert!(encode_png_zip(&[]).is_err());
}
