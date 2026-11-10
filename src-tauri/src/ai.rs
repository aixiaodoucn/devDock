use crate::config::{validate_ai_tools, Action, AiTool, AppConfig, Project, Result, Settings};

pub fn resolve_tool<'a>(project: &Project, settings: &'a Settings) -> Option<&'a AiTool> {
    let tool_id = if project.ai_tool_id.is_empty() {
        &settings.default_ai_tool_id
    } else {
        &project.ai_tool_id
    };
    // 显式选择失效时只打开目录，不回退到其他全局工具。
    settings
        .ai_tools
        .iter()
        .find(|tool| !tool_id.is_empty() && tool.id == *tool_id && tool.enabled)
}

pub fn prepare_saved_action(config: &AppConfig, project_id: &str) -> Result<(Project, Action)> {
    let project = config
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or("项目不存在")?;
    let tool_id = resolve_tool(project, &config.settings).map(|tool| tool.id.as_str());
    prepare_action(project, &config.settings, tool_id)
}

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
            .ok_or("终端命令不存在")?;
        if !tool.enabled {
            return Err("终端命令已禁用".into());
        }
        action.id = tool.id.clone();
        action.name = tool.name.clone();
        action.command = tool.command.clone();
    }
    // aiTools 兼容字段承载通用终端命令，使用终端默认环境，仅清空临时副本的 Node 选择。
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
    fn global_default_accepts_generic_terminal_command_verbatim() {
        let mut project = project("backend");
        project.commands.clear();
        project.node_version.clear();
        project.nvm_dir.clear();
        let mut settings = settings();
        settings.default_ai_tool_id = "custom".into();
        settings.ai_tools[0].name = "更新代码".into();
        for command in ["git pull", "  git pull --ff-only  "] {
            settings.ai_tools[0].command = command.into();
            let config = crate::config::AppConfig {
                projects: vec![project.clone()],
                settings: settings.clone(),
                ..Default::default()
            };
            let saved = crate::config::parse(&serde_json::to_string(&config).unwrap()).unwrap();
            let (temporary, action) = prepare_saved_action(&saved, &project.id).unwrap();
            assert_eq!(action.command, command);
            assert_eq!(
                crate::nvm::command_for(&temporary, &action).unwrap(),
                command
            );
        }
    }

    #[test]
    fn resolves_project_override_and_follows_current_global_default() {
        let mut project = project("backend");
        let mut settings = settings();
        settings.ai_tools.push(AiTool {
            id: "global".into(),
            name: "Global".into(),
            command: "global-cli".into(),
            enabled: true,
        });
        assert!(resolve_tool(&project, &settings).is_none());
        settings.default_ai_tool_id = "global".into();
        assert_eq!(resolve_tool(&project, &settings).unwrap().id, "global");
        settings.default_ai_tool_id = "custom".into();
        assert_eq!(resolve_tool(&project, &settings).unwrap().id, "custom");
        project.ai_tool_id = "global".into();
        assert_eq!(resolve_tool(&project, &settings).unwrap().id, "global");
    }

    #[test]
    fn unavailable_preferences_only_change_directory_without_falling_back() {
        for explicit in [false, true] {
            for missing in [false, true] {
                let mut project = project("backend");
                let mut settings = settings();
                settings.default_ai_tool_id = "custom".into();
                if explicit {
                    project.ai_tool_id = "unavailable".into();
                    settings.ai_tools.push(AiTool {
                        id: "unavailable".into(),
                        name: "Disabled".into(),
                        command: "never-run".into(),
                        enabled: false,
                    });
                    if missing {
                        settings.ai_tools.pop();
                    }
                } else if missing {
                    settings.ai_tools.clear();
                } else {
                    settings.ai_tools[0].enabled = false;
                }
                assert!(resolve_tool(&project, &settings).is_none());
                let (temporary, action) = prepare_action(
                    &project,
                    &settings,
                    resolve_tool(&project, &settings).map(|tool| tool.id.as_str()),
                )
                .unwrap();
                assert!(action.command.is_empty());
                assert_eq!(crate::nvm::command_for(&temporary, &action).unwrap(), "");
                assert!(temporary.node_version.is_empty());
                assert!(temporary.nvm_dir.is_empty());
                assert_eq!(project.node_version, "v16.20.2");
            }
        }
    }

    #[test]
    fn saved_project_is_used_instead_of_stale_snapshot() {
        let snapshot = project("backend");
        let mut saved = snapshot.clone();
        saved.ai_tool_id = "custom".into();
        saved.path = "/tmp/current-project".into();
        let mut config = crate::config::AppConfig {
            projects: vec![saved],
            settings: settings(),
            ..Default::default()
        };
        let (temporary, action) = prepare_saved_action(&config, &snapshot.id).unwrap();
        assert_eq!(action.id, "custom");
        assert_eq!(temporary.path, "/tmp/current-project");
        assert!(snapshot.ai_tool_id.is_empty());
        config.projects[0].ai_tool_id.clear();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        for default in ["custom", "removed", ""] {
            config.settings.default_ai_tool_id = default.into();
            config.projects[0].node_version.clear();
            config.projects[0].nvm_dir.clear();
            config.projects[0].commands.clear();
            crate::config::atomic_write(&path, &serde_json::to_string(&config).unwrap()).unwrap();
            let current = crate::config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let (_, action) = prepare_saved_action(&current, &snapshot.id).unwrap();
            assert_eq!(action.command.is_empty(), default != "custom");
        }
        assert!(prepare_saved_action(&config, "deleted-project")
            .unwrap_err()
            .contains("不存在"));
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
