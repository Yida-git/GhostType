#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod fallback;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use fallback as imp;

/// 检查/请求 macOS 辅助功能权限。
///
/// - `prompt=true`：触发系统弹窗引导（如果尚未授权）
/// - 非 macOS：直接返回 `true`
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn ensure_accessibility(prompt: bool) -> bool {
    imp::ensure_accessibility(prompt)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_accessibility_settings() -> Result<(), String> {
    imp::open_accessibility_settings()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_microphone_settings() -> Result<(), String> {
    imp::open_microphone_settings()
}

#[allow(dead_code)]
pub fn open_sound_settings() -> Result<(), String> {
    imp::open_sound_settings()
}
