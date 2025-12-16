fn main() {
    #[cfg(target_os = "windows")]
    ensure_windows_icon();
    tauri_build::build()
}

#[cfg(target_os = "windows")]
fn ensure_windows_icon() {
    use std::fs;
    use std::path::Path;

    let icon_path = Path::new("icons").join("icon.ico");
    if icon_path.exists() {
        return;
    }

    if let Some(parent) = icon_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Minimal 1x1 transparent ICO (BMP/DIB), used only as a fallback to keep builds reproducible.
    const FALLBACK_ICO: &[u8] = &[
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, // ICONDIR
        0x01, 0x01, 0x00, 0x00, // width, height, color count, reserved
        0x01, 0x00, 0x20, 0x00, // planes=1, bpp=32
        0x30, 0x00, 0x00, 0x00, // bytes in res = 48
        0x16, 0x00, 0x00, 0x00, // image offset = 22
        // BITMAPINFOHEADER (40 bytes)
        0x28, 0x00, 0x00, 0x00, // biSize = 40
        0x01, 0x00, 0x00, 0x00, // biWidth = 1
        0x02, 0x00, 0x00, 0x00, // biHeight = 2 (1*2, includes mask)
        0x01, 0x00, // biPlanes = 1
        0x20, 0x00, // biBitCount = 32
        0x00, 0x00, 0x00, 0x00, // biCompression = BI_RGB
        0x04, 0x00, 0x00, 0x00, // biSizeImage = 4
        0x00, 0x00, 0x00, 0x00, // biXPelsPerMeter
        0x00, 0x00, 0x00, 0x00, // biYPelsPerMeter
        0x00, 0x00, 0x00, 0x00, // biClrUsed
        0x00, 0x00, 0x00, 0x00, // biClrImportant
        // Pixel (BGRA) + AND mask (32-bit padded)
        0x00, 0x00, 0x00, 0x00, // pixel
        0x00, 0x00, 0x00, 0x00, // mask
    ];

    let _ = fs::write(icon_path, FALLBACK_ICO);
}
