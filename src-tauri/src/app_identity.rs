use tauri::Config;

pub const DEVELOPMENT_ID: &str = "cn.aixiaodou.devdock.dev";
pub const DEVELOPMENT_NAME: &str = "DevDock Dev";

pub fn configure(config: &mut Config, development: bool) {
    if !development {
        return;
    }
    // 即使绕过 CLI 直接运行 debug 二进制，也不能读写正式版的数据目录。
    config.identifier = DEVELOPMENT_ID.into();
    config.product_name = Some(DEVELOPMENT_NAME.into());
    for window in &mut config.app.windows {
        if window.label == "main" {
            window.title = DEVELOPMENT_NAME.into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_config() -> Config {
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap()
    }

    #[test]
    fn debug_identity_is_separate_without_changing_window_labels_or_resources() {
        let mut config = production_config();
        let original = config.clone();
        configure(&mut config, true);
        assert_eq!(config.identifier, DEVELOPMENT_ID);
        assert_ne!(config.identifier, original.identifier);
        assert_eq!(config.product_name.as_deref(), Some(DEVELOPMENT_NAME));
        assert_eq!(config.app.windows[0].title, DEVELOPMENT_NAME);
        assert_eq!(config.app.windows[0].label, original.app.windows[0].label);
        assert_eq!(config.bundle.resources, original.bundle.resources);
        assert_eq!(config.version, original.version);
    }

    #[test]
    fn release_identity_and_configuration_remain_unchanged() {
        let mut config = production_config();
        let original = serde_json::to_value(&config).unwrap();
        configure(&mut config, false);
        assert_eq!(serde_json::to_value(&config).unwrap(), original);
        assert_eq!(config.identifier, "cn.aixiaodou.devdock");
        assert_eq!(config.product_name.as_deref(), Some("DevDock"));
    }
}
