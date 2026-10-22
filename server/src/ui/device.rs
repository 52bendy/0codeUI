//! 设备信息：请求字段与平台归一化。

use serde::Deserialize;

/// 设备信息（UI 生成请求的一部分）。
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInfo {
    /// 屏幕宽度（CSS px），1..=20000。
    pub screen_width: u32,
    /// 屏幕高度（CSS px），1..=20000。
    pub screen_height: u32,
    /// 平台标识：android / ios / web / harmonyos（大小写不敏感）。
    pub platform: String,
}

impl DeviceInfo {
    /// 把平台标识归一化为稳定的小写值；未知平台返回可读错误。
    pub fn normalized_platform(&self) -> Result<&'static str, String> {
        match self.platform.trim().to_ascii_lowercase().as_str() {
            "android" => Ok("android"),
            "ios" => Ok("ios"),
            "web" => Ok("web"),
            "harmonyos" | "harmony" => Ok("harmonyos"),
            other => Err(format!(
                "device_info.platform must be one of android, ios, web, harmonyos; got `{other}`"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(platform: &str) -> DeviceInfo {
        DeviceInfo {
            screen_width: 390,
            screen_height: 844,
            platform: platform.to_string(),
        }
    }

    #[test]
    fn platform_is_normalized_case_insensitively() {
        assert_eq!(device("Android").normalized_platform().unwrap(), "android");
        assert_eq!(device("IOS").normalized_platform().unwrap(), "ios");
        assert_eq!(device("Web").normalized_platform().unwrap(), "web");
        assert_eq!(
            device("HarmonyOS").normalized_platform().unwrap(),
            "harmonyos"
        );
        assert_eq!(
            device("harmony").normalized_platform().unwrap(),
            "harmonyos"
        );
    }

    #[test]
    fn unknown_platform_is_rejected() {
        let error = device("blackberry").normalized_platform().unwrap_err();
        assert!(error.contains("android, ios, web, harmonyos"));
    }
}
