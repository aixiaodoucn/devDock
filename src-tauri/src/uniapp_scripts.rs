use crate::config::Result;
use serde::Serialize;
use serde_json::Value;
use std::{fs, path::Path};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UniappScript {
    pub id: String,
    pub title: String,
    pub uni_platform: String,
    pub platform: String,
    pub run_menu_path: Vec<String>,
    pub publish_menu_path: Vec<String>,
}

pub fn read(root: &Path) -> Result<Vec<UniappScript>> {
    if !root.is_absolute() {
        return Err("项目目录必须为绝对路径".into());
    }
    let metadata = fs::metadata(root)
        .map_err(|error| format!("项目目录不存在或无法访问（{}）: {error}", root.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "项目目录无效，所选路径不是目录（{}）",
            root.display()
        ));
    }
    // 仅读取所选项目根目录，不搜索父目录或其他候选路径。
    let path = root.join("package.json");
    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "package.json 不存在或无法读取（{}）: {error}",
            path.display()
        )
    })?;
    let package: Value = serde_json::from_str(&text)
        .map_err(|error| format!("package.json JSON 格式无效（{}）: {error}", path.display()))?;
    let package = package
        .as_object()
        .ok_or_else(|| format!("package.json 必须为对象（{}）", path.display()))?;
    let Some(uniapp) = package.get("uni-app") else {
        return Ok(vec![]);
    };
    let uniapp = uniapp
        .as_object()
        .ok_or_else(|| format!("package.json uni-app 必须为对象（{}）", path.display()))?;
    let Some(scripts) = uniapp.get("scripts") else {
        return Ok(vec![]);
    };
    let scripts = scripts.as_object().ok_or_else(|| {
        format!(
            "package.json uni-app.scripts 必须为对象（{}）",
            path.display()
        )
    })?;
    let mut result = vec![];
    for (id, script) in scripts {
        let context = format!("package.json uni-app.scripts[{id:?}]（{}）", path.display());
        let script = script
            .as_object()
            .ok_or_else(|| format!("{context} 必须为对象"))?;
        let title = script
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty() && !title.contains(['\n', '\r', '\0']))
            .ok_or_else(|| {
                format!("{context} title 标题必须为非空白字符串，且不能包含换行或 NUL")
            })?;
        let uni_platform = script
            .get("env")
            .and_then(|env| env.get("UNI_PLATFORM"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{context} env.UNI_PLATFORM 必须存在且为字符串"))?;
        // mp-alipay 本身不代表钉钉，必须同时具备项目明确标记。
        let dingtalk = script
            .get("define")
            .and_then(|define| define.get("MP-DINGTALK"))
            .and_then(Value::as_bool)
            == Some(true)
            || id.to_ascii_lowercase().contains("dingtalk")
            || title.contains("钉钉")
            || title.to_ascii_lowercase().contains("dingtalk");
        let platform = match uni_platform {
            "mp-weixin" => "weixin",
            "mp-alipay" if dingtalk => "dingtalk",
            _ => "",
        };
        // title 整体是一个菜单段；环境名和脚本 key 不决定运行/发行能力。
        result.push(UniappScript {
            id: id.clone(),
            title: title.into(),
            uni_platform: uni_platform.into(),
            platform: platform.into(),
            run_menu_path: vec!["运行".into(), title.into()],
            publish_menu_path: vec!["发行".into(), "自定义发行".into(), title.into()],
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Project;
    use serde_json::{json, Value};
    use std::{fs, path::Path};

    fn project(root: &Path) -> Project {
        serde_json::from_value(json!({
            "id":"uniapp", "name":"测试项目", "type":"uniapp", "path":root,
            "platforms":["weixin"], "manifestPath":"", "androidAppId":"", "iosAppId":"",
            "androidProjectPath":"", "androidModule":"", "jdkPath":"", "gradleTask":"",
            "outputDir":"", "syncMainVersion":false, "terminal":"Terminal", "commands":[]
        }))
        .unwrap()
    }

    fn package(root: &Path, value: Value) {
        fs::write(root.join("package.json"), value.to_string()).unwrap();
    }

    fn read_value(root: &Path, value: Value) -> Result<Vec<UniappScript>> {
        package(root, value);
        crate::read_uniapp_scripts(project(root))
    }

    #[test]
    fn weixin_and_dingtalk_samples_have_both_menus_for_every_key() {
        let temp = tempfile::tempdir().unwrap();
        let scripts = read_value(temp.path(), json!({"uni-app":{"scripts":{
            "mp-weixin-dev":{"title":"微信小程序开发环境","env":{"UNI_PLATFORM":"mp-weixin","ENV_TYPE":"development"}},
            "mp-weixin-test":{"title":"微信小程序测试环境","env":{"UNI_PLATFORM":"mp-weixin","ENV_TYPE":"test"}},
            "mp-weixin-prod":{"title":"微信小程序生产环境","env":{"UNI_PLATFORM":"mp-weixin","ENV_TYPE":"production"}},
            "mp-dingtalk-dev":{"title":"钉钉小程序-开发","env":{"UNI_PLATFORM":"mp-alipay","ENV_TYPE":"dev"},"define":{"MP-DINGTALK":true}},
            "mp-dingtalk-prod":{"title":"钉钉小程序-生产","env":{"UNI_PLATFORM":"mp-alipay","ENV_TYPE":"prod"},"define":{"MP-DINGTALK":true}},
            "arbitrary":{"title":"自定义环境","env":{"UNI_PLATFORM":"mp-weixin"}}
        }}})).unwrap();
        assert_eq!(scripts.len(), 6);
        for script in scripts {
            assert_eq!(script.run_menu_path, ["运行", &script.title]);
            assert_eq!(
                script.publish_menu_path,
                ["发行", "自定义发行", &script.title]
            );
            assert_eq!(
                script.platform,
                if script.uni_platform == "mp-weixin" {
                    "weixin"
                } else {
                    "dingtalk"
                }
            );
            let value = serde_json::to_value(&script).unwrap();
            assert_eq!(
                value,
                json!({"id":script.id,"title":script.title,
                "uniPlatform":script.uni_platform,"platform":script.platform,
                "runMenuPath":script.run_menu_path,"publishMenuPath":script.publish_menu_path})
            );
        }
    }

    #[test]
    fn unsupported_platforms_are_retained_and_dingtalk_requires_evidence() {
        let temp = tempfile::tempdir().unwrap();
        for (id, title, uni_platform, define, expected) in [
            ("h5-dev", "h5-开发环境", "h5", json!({}), ""),
            ("h5-prod", "h5-生产环境", "h5", json!({}), ""),
            ("alipay", "支付宝小程序", "mp-alipay", json!({}), ""),
            (
                "alipay",
                "支付宝小程序",
                "mp-alipay",
                json!({"MP-DINGTALK":false}),
                "",
            ),
            (
                "alipay",
                "支付宝小程序",
                "mp-alipay",
                json!({"MP-DINGTALK":"true"}),
                "",
            ),
            (
                "custom",
                "企业小程序",
                "mp-alipay",
                json!({"MP-DINGTALK":true}),
                "dingtalk",
            ),
            (
                "custom-DingTalk-dev",
                "企业小程序",
                "mp-alipay",
                json!({}),
                "dingtalk",
            ),
            ("custom", "钉钉开发", "mp-alipay", json!({}), "dingtalk"),
            (
                "custom",
                "企业小程序",
                "h5",
                json!({"MP-DINGTALK":true}),
                "",
            ),
        ] {
            let scripts = read_value(
                temp.path(),
                json!({"uni-app":{"scripts":{
                    id:{"title":title,"env":{"UNI_PLATFORM":uni_platform},"define":define}
                }}}),
            )
            .unwrap();
            assert_eq!(scripts.len(), 1);
            assert_eq!(scripts[0].id, id);
            assert_eq!(scripts[0].uni_platform, uni_platform);
            assert_eq!(scripts[0].platform, expected, "{id}: {title}");
            assert_eq!(scripts[0].run_menu_path, ["运行", title]);
            assert_eq!(scripts[0].publish_menu_path, ["发行", "自定义发行", title]);
        }
    }

    #[test]
    fn title_is_preserved_as_one_menu_segment() {
        let temp = tempfile::tempdir().unwrap();
        let title = "  微信/测试 / 环境  ";
        let scripts = read_value(
            temp.path(),
            json!({"uni-app":{"scripts":{
                "dev":{"title":title,"env":{"UNI_PLATFORM":"mp-weixin"}}
            }}}),
        )
        .unwrap();
        assert_eq!(scripts[0].title, title);
        assert_eq!(scripts[0].run_menu_path, ["运行", title]);
        assert_eq!(scripts[0].publish_menu_path, ["发行", "自定义发行", title]);
    }

    #[test]
    fn missing_uniapp_scripts_does_not_use_npm_scripts() {
        let temp = tempfile::tempdir().unwrap();
        for value in [
            json!({}),
            json!({"scripts":{"dev":"must never execute"}}),
            json!({"uni-app":{}}),
            json!({"uni-app":{"scripts":{}}}),
        ] {
            assert!(read_value(temp.path(), value).unwrap().is_empty());
        }
    }

    #[test]
    fn malformed_structures_return_explicit_chinese_errors() {
        let temp = tempfile::tempdir().unwrap();
        for invalid in [Value::Null, json!([]), json!("bad"), json!(42), json!(true)] {
            let error =
                read_value(temp.path(), json!({"uni-app":{"scripts":invalid}})).unwrap_err();
            assert!(
                error.contains("uni-app.scripts") && error.contains("必须为对象"),
                "{error}"
            );
            let error = read_value(temp.path(), json!({"uni-app":{"scripts":{"bad":invalid}}}))
                .unwrap_err();
            assert!(
                error.contains("bad") && error.contains("必须为对象"),
                "{error}"
            );
        }
        for entry in [
            json!({}),
            json!({"title":null}),
            json!({"title":12}),
            json!({"title":""}),
            json!({"title":" \t "}),
            json!({"title":"a\nb"}),
            json!({"title":"a\rb"}),
            json!({"title":"a\0b"}),
        ] {
            let error =
                read_value(temp.path(), json!({"uni-app":{"scripts":{"bad":entry}}})).unwrap_err();
            assert!(
                error.contains("bad") && error.contains("title") && error.contains("标题"),
                "{error}"
            );
        }
        for env in [
            Value::Null,
            json!([]),
            json!("bad"),
            json!({}),
            json!({"UNI_PLATFORM":null}),
            json!({"UNI_PLATFORM":true}),
            json!({"UNI_PLATFORM":12}),
        ] {
            let error = read_value(
                temp.path(),
                json!({"uni-app":{"scripts":{
                    "bad":{"title":"有效标题","env":env}
                }}}),
            )
            .unwrap_err();
            assert!(
                error.contains("bad")
                    && error.contains("env.UNI_PLATFORM")
                    && error.contains("字符串"),
                "{error}"
            );
        }
        let error = read_value(
            temp.path(),
            json!({"uni-app":{"scripts":{
                "bad":{"title":"有效标题"}
            }}}),
        )
        .unwrap_err();
        assert!(error.contains("env.UNI_PLATFORM"));
    }

    #[test]
    fn selected_directories_with_spaces_and_chinese_are_independent_and_read_only() {
        let temp = tempfile::tempdir().unwrap();
        for title in ["项目甲/开发", "项目乙/生产"] {
            let root = temp
                .path()
                .join(format!("中文 项目 {title}").replace('/', " "));
            fs::create_dir(&root).unwrap();
            package(
                &root,
                json!({"uni-app":{"scripts":{
                    "same-id":{"title":title,"env":{"UNI_PLATFORM":"mp-weixin"}}
                }}}),
            );
            let path = root.join("package.json");
            let before = fs::read(&path).unwrap();
            let scripts = crate::read_uniapp_scripts(project(&root)).unwrap();
            assert_eq!(scripts[0].id, "same-id");
            assert_eq!(scripts[0].title, title);
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        }
    }

    #[test]
    fn missing_and_invalid_paths_and_json_are_explained_without_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let missing = root.join("不存在的目录");
        let error = crate::read_uniapp_scripts(project(&missing)).unwrap_err();
        assert!(
            error.contains("项目目录")
                && error.contains("不存在")
                && error.contains(missing.to_str().unwrap()),
            "{error}"
        );
        let error = crate::read_uniapp_scripts(project(root)).unwrap_err();
        assert!(
            error.contains("package.json") && error.contains("不存在"),
            "{error}"
        );
        let relative = crate::read_uniapp_scripts(project(Path::new("relative"))).unwrap_err();
        assert!(relative.contains("绝对路径"));
        let path = root.join("package.json");
        fs::write(&path, "invalid json").unwrap();
        let error = crate::read_uniapp_scripts(project(root)).unwrap_err();
        assert!(
            error.contains("package.json") && error.contains("JSON"),
            "{error}"
        );
        let error = crate::read_uniapp_scripts(project(&path)).unwrap_err();
        assert!(
            error.contains("项目目录") && error.contains("不是目录"),
            "{error}"
        );
    }
}
