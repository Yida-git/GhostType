use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFString;
use std::process::Command;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
}

pub fn ensure_accessibility(prompt: bool) -> bool {
    unsafe {
        if prompt {
            let key = CFString::new("AXTrustedCheckOptionPrompt");
            let value = CFBoolean::true_value();
            let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
            AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
        } else {
            AXIsProcessTrusted()
        }
    }
}

pub fn open_accessibility_settings() -> Result<(), String> {
    // macOS Sonoma/Sequoia/Monterey: 该 URL 通常可直达“隐私与安全 -> 辅助功能”
    let status = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(format!("open failed: status={status}"))
}

pub fn open_microphone_settings() -> Result<(), String> {
    let status = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(format!("open failed: status={status}"))
}

pub fn open_sound_settings() -> Result<(), String> {
    let status = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.sound")
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(format!("open failed: status={status}"))
}
