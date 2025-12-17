#[allow(dead_code)]
pub fn ensure_accessibility(prompt: bool) -> bool {
    let _ = prompt;
    true
}

#[allow(dead_code)]
pub fn open_accessibility_settings() -> Result<(), String> {
    Err("当前平台不需要辅助功能权限".to_string())
}

#[allow(dead_code)]
pub fn open_microphone_settings() -> Result<(), String> {
    #[cfg(windows)]
    {
        return open_sound_settings();
    }
    Err("请在系统设置中检查麦克风权限/设备".to_string())
}

#[allow(dead_code)]
pub fn open_sound_settings() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let status = Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:sound"])
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("打开声音设置失败: status={status}"));
    }

    #[cfg(not(windows))]
    {
        Err("当前平台不支持自动打开声音设置".to_string())
    }
}
