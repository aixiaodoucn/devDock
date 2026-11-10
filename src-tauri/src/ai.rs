use crate::config::{validate_ai_tools, Action, Project, Result, Settings};

pub fn prepare_action(
    project: &Project,
    settings: &Settings,
    tool_id: Option<&str>,
) -> Result<(Project, Action)> {
    let mut action = Action {
        id: "open-project-terminal".into(),
        name: "打开项目终端".into(),
        platform: String::new(),
        kind: "shell".into(),
        menu_path: vec![],
        command: String::new(),
    };
    if let Some(tool_id) = tool_id {
        validate_ai_tools(&settings.ai_tools)?;
        let tool = settings
            .ai_tools
            .iter()
            .find(|tool| tool.id == tool_id)
            .ok_or("AI 工具不存在")?;
        if !tool.enabled {
            return Err("AI 工具已禁用".into());
        }
        action.id = tool.id.clone();
        action.name = tool.name.clone();
        action.command = tool.command.clone();
    }
    // 打开终端和 AI CLI 均使用终端默认环境，仅清空临时副本中的 Node 选择。
    let mut temporary = project.clone();
    temporary.node_version.clear();
    temporary.nvm_dir.clear();
    Ok((temporary, action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AiTool;

    fn project(project_type: &str) -> Project {
        serde_json::from_value(serde_json::json!({
            "id":"project", "name":"Project", "type":project_type,
            "path":"/tmp/ai-project", "platforms":[], "manifestPath":"",
            "androidAppId":"", "iosAppId":"", "androidProjectPath":"",
            "androidModule":"", "jdkPath":"", "gradleTask":"", "outputDir":"",
            "syncMainVersion":false, "terminal":"/Applications/iTerm.app",
            "nodeVersion":"v16.20.2", "nvmDir":"/not-installed/.nvm",
            "commands":[{"id":"existing", "name":"Existing", "platform":"pc",
                "kind":"shell", "menuPath":[], "command":"npm run dev"}]
        }))
        .unwrap()
    }

    fn settings() -> Settings {
        Settings {
            ai_tools: vec![AiTool {
                id: "custom".into(),
                name: "Custom CLI".into(),
                command: "  custom --arg 'a b' \"c d\";\necho done  ".into(),
                enabled: true,
            }],
            ..Settings::default()
        }
    }

    #[test]
    fn prepares_shell_for_all_project_types_without_changing_saved_configuration() {
        for project_type in ["uniapp", "nodejs", "backend"] {
            let original = project(project_type);
            let settings = settings();
            let before = serde_json::to_value(&original).unwrap();
            let settings_before = serde_json::to_value(&settings).unwrap();
            let (temporary, action) = prepare_action(&original, &settings, Some("custom")).unwrap();
            assert_eq!(action.kind, "shell");
            assert_eq!(action.id, "custom");
            assert_eq!(action.name, "Custom CLI");
            assert!(action.menu_path.is_empty());
            assert_eq!(action.command, settings.ai_tools[0].command);
            assert!(temporary.node_version.is_empty());
            assert!(temporary.nvm_dir.is_empty());
            // 直接验证现有终端分支拿到原始命令，不生成项目 Node 的激活前缀。
            assert_eq!(
                crate::nvm::command_for(&temporary, &action).unwrap(),
                action.command
            );
            let mut expected = before.clone();
            expected["nodeVersion"] = "".into();
            expected["nvmDir"] = "".into();
            assert_eq!(serde_json::to_value(temporary).unwrap(), expected);
            assert_eq!(serde_json::to_value(&original).unwrap(), before);
            assert_eq!(serde_json::to_value(&settings).unwrap(), settings_before);
        }
    }

    #[test]
    fn no_tool_opens_only_directory_even_with_invalid_project_node_settings() {
        for project_type in ["uniapp", "nodejs", "backend"] {
            let mut original = project(project_type);
            original.node_version = "invalid-version".into();
            original.nvm_dir = "relative/nvm".into();
            let before = serde_json::to_value(&original).unwrap();
            let settings = Settings::default();
            assert!(settings.ai_tools.is_empty());
            let (temporary, action) = prepare_action(&original, &settings, None).unwrap();
            assert_eq!(action.kind, "shell");
            assert_eq!(action.name, "打开项目终端");
            assert!(action.command.is_empty());
            assert!(action.menu_path.is_empty());
            assert!(temporary.node_version.is_empty());
            assert!(temporary.nvm_dir.is_empty());
            assert_eq!(crate::nvm::command_for(&temporary, &action).unwrap(), "");
            let mut expected = before.clone();
            expected["nodeVersion"] = "".into();
            expected["nvmDir"] = "".into();
            assert_eq!(serde_json::to_value(temporary).unwrap(), expected);
            assert_eq!(serde_json::to_value(&original).unwrap(), before);
        }
    }

    #[test]
    fn rejects_missing_disabled_and_invalid_tools() {
        let original = project("backend");
        let mut settings = settings();
        assert!(prepare_action(&original, &settings, Some("")).is_err());
        assert!(prepare_action(&original, &settings, Some("missing"))
            .unwrap_err()
            .contains("不存在"));
        settings.ai_tools[0].enabled = false;
        assert!(prepare_action(&original, &settings, Some("custom"))
            .unwrap_err()
            .contains("禁用"));
        settings.ai_tools[0].enabled = true;
        settings.ai_tools[0].command = "bad\0command".into();
        assert!(prepare_action(&original, &settings, Some("custom")).is_err());
    }
}
