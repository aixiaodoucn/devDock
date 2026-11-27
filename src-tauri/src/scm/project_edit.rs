use super::*;
#[path = "project_remove.rs"]
mod project_remove;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

const FORM_ERROR: &str = "Parse: SCM 修改页字段不完整或不匹配，未提交修改";
const DATE_FIELDS: [&str; 8] = [
    "dev_start_time",
    "dev_end_time",
    "union_start_time",
    "union_end_time",
    "qa_start_time",
    "qa_end_time",
    "plan_release_time",
    "release_time",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditOptions {
    options: CreateOptions,
    input: CreateInput,
    existing_modules: Vec<String>,
    authors: Vec<String>,
    revision: String,
}

struct EditForm {
    data: EditOptions,
    // 保留原页字段，不接收前端传来的日期、等级或项目 ID。
    original: Value,
}

fn field(doc: &Html, id: &str) -> Result<String> {
    let sel = selector(&format!("#{id}"));
    let mut matches = doc.select(&sel);
    let node = matches.next().ok_or(FORM_ERROR)?;
    if matches.next().is_some() {
        return Err(FORM_ERROR.into());
    }
    match node.value().name() {
        "input" => Ok(node.value().attr("value").unwrap_or_default().into()),
        "textarea" => Ok(node.text().collect()),
        _ => Err(FORM_ERROR.into()),
    }
}

// 只识别原站固定格式的字面量初始化，不运行 JavaScript，也不把脚本放进响应。
static SELECT_INIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\$\(\s*["']#(level|test-level|project_type)["']\s*\)\s*\.val\(\s*["']([A-Za-z0-9_-]+)["']\s*\)\s*;"#).unwrap()
});
static AUTHOR_INIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bvar\s+author\s*=\s*["']([^"'\\\r\n]*)["']\s*\.split\(\s*["'],["']\s*\)\s*;"#)
        .unwrap()
});

fn parse_edit(html: &str, id: &str, username: &str) -> Result<EditForm> {
    check_html(html)?;
    let doc = Html::parse_document(html);
    if doc
        .select(&selector(
            "button#smt:not([disabled]),input#smt:not([disabled])",
        ))
        .next()
        .is_none()
    {
        return Err("Permission: SCM 未提供可编辑表单，无权修改或页面已变更".into());
    }
    if !numeric_id(id) || field(&doc, "pid")? != id {
        return Err(FORM_ERROR.into());
    }
    let mut values = std::collections::HashMap::new();
    let mut authors = None;
    for script in doc.select(&selector("script:not([src])")) {
        let js = script.text().collect::<String>();
        for m in SELECT_INIT.captures_iter(&js) {
            if values.insert(m[1].to_owned(), m[2].to_owned()).is_some() {
                return Err(FORM_ERROR.into());
            }
        }
        for m in AUTHOR_INIT.captures_iter(&js) {
            if authors.is_some() {
                return Err(FORM_ERROR.into());
            }
            authors = Some(
                m[1].split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            );
        }
    }
    let mut original = json!({"id":id});
    for date in DATE_FIELDS {
        original[date] = field(&doc, date)?.into();
    }
    let mut selected = |id: &str| -> Result<String> {
        let value = values.remove(id).ok_or(FORM_ERROR)?;
        if !select_choices(&doc, &format!("select#{id}"))?
            .0
            .iter()
            .any(|option| option.value == value)
        {
            return Err(FORM_ERROR.into());
        }
        Ok(value)
    };
    original["level"] = selected("level")?.into();
    original["test_level"] = selected("test-level")?.into();
    let authors = authors.ok_or(FORM_ERROR)?;
    let project_type = selected("project_type")?;
    let users = select_choices(&doc, "select#user")?.0;
    let default_user = users
        .iter()
        .find(|c| c.value.eq_ignore_ascii_case(username))
        .map(|c| c.value.clone())
        .unwrap_or_default();
    let options = CreateOptions {
        users,
        default_user,
        modules: select_choices(&doc, "#mode_template_add")?.0,
        types: select_choices(&doc, "select#project_type")?.0,
    };
    let existing_modules: Vec<String> = doc
        .select(&selector(".del_mode[git_pid]"))
        .map(|node| node.value().attr("git_pid").unwrap().to_owned())
        .collect();
    if existing_modules.iter().any(|id| id.is_empty()) {
        return Err(FORM_ERROR.into());
    }
    let input = CreateInput {
        name: field(&doc, "name")?,
        branch_suffix: field(&doc, "name_en")?,
        project_type,
        user: authors.first().cloned().unwrap_or_default(),
        details: field(&doc, "project_desc")?,
        modules: Vec::new(),
    };
    // 只对项目状态取摘要；无关页面内容变化不会使正在编辑的表单失效。
    let snapshot = serde_json::to_vec(&(&original, &input, &authors, &existing_modules))
        .map_err(|_| FORM_ERROR)?;
    let revision = format!("{:x}", Sha256::digest(snapshot));
    Ok(EditForm {
        data: EditOptions {
            options,
            input,
            existing_modules,
            authors,
            revision,
        },
        original,
    })
}

impl EditForm {
    fn payload(self, input: &CreateInput, authors: &[String], revision: &str) -> Result<Value> {
        if self.data.revision != revision {
            return Err("Conflict: 项目已被修改，请重新打开表单核对".into());
        }
        self.data.options.validate(input)?;
        let unique = |items: &[String]| items.iter().collect::<HashSet<_>>().len() == items.len();
        // 历史负责人即使已从候选中移除，也允许原样保留，但不能添加陌生账号。
        let allowed = |u: &String| {
            self.data.authors.contains(u) || self.data.options.users.iter().any(|c| &c.value == u)
        };
        if authors.is_empty()
            || !unique(authors)
            || !authors.contains(&input.user)
            || !authors.iter().all(allowed)
        {
            return Err("InvalidInput: 负责人不在可选范围内或重复".into());
        }
        if !unique(&input.modules)
            || input
                .modules
                .iter()
                .any(|id| self.data.existing_modules.contains(id))
        {
            return Err("InvalidInput: 只能添加尚未关联且不重复的模块".into());
        }
        let mut param = self.original;
        param["name"] = json!(input.name);
        param["name_en"] = json!(input.branch_suffix.trim());
        param["description"] = json!(input.details);
        param["author"] = json!(authors);
        param["git_pids"] = if input.modules.is_empty() {
            Value::Null
        } else {
            json!(input.modules)
        };
        param["project_type"] = json!(input.project_type);
        Ok(param)
    }
}

impl Session {
    async fn edit_form(&self, id: &str) -> Result<EditForm> {
        if !numeric_id(id) {
            return Err("InvalidInput: 项目 ID 无效".into());
        }
        parse_edit(
            &self.get("/kpi/project/modify", &[("id", id)]).await?,
            id,
            &self.username,
        )
    }
    async fn update_project(
        &self,
        id: &str,
        input: &CreateInput,
        authors: &[String],
        revision: &str,
    ) -> Result<()> {
        // 在当前会话中重新 GET，权限、原值、候选项与 revision 均通过后才允许 POST。
        let param = self
            .edit_form(id)
            .await?
            .payload(input, authors, revision)?;
        let body = self
            .submit_form("/kpi/project/modifySmt", param.to_string(), "修改")
            .await?;
        write_response(&body, "修改").map(|_| ())
    }
}

impl Runtime {
    pub async fn edit_options(&self, id: String) -> Result<EditOptions> {
        self.project_form_session(async |s| Ok(s.edit_form(&id).await?.data))
            .await
    }
    pub(crate) async fn approved_update(
        &self,
        binding: &SessionBinding,
        id: String,
        input: CreateInput,
        authors: Vec<String>,
        revision: String,
    ) -> Result<()> {
        self.project_form_session(async |session| {
            binding.check(session)?;
            session
                .update_project(&id, &input, &authors, &revision)
                .await
        })
        .await
    }
    pub async fn update(
        &self,
        id: String,
        input: CreateInput,
        authors: Vec<String>,
        revision: String,
    ) -> Result<()> {
        self.project_form_session(async |s| {
            s.update_project(&id, &input, &authors, &revision).await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn page() -> String {
        let mut html = String::from(
            r##"<input type="hidden" id="pid" value="11"><input id="name" value="项目&amp;测试"><input id="name_en" value="v1"><textarea id="project_desc"> 说明 </textarea>
            <button id="smt">保存</button><select id="user" multiple><option value="a">甲</option><option value="b">乙</option></select>
            <select id="mode_template_add" multiple><option value="1">旧模块</option><option value="2">新模块</option></select>
            <i class="del_mode"></i><i class="del_mode" git_pid="1"></i>
            <select id="level"><option value="D">D</option><option value="C">C</option></select><select id="test-level"><option value="1">一</option><option value="2">二</option></select>
            <select id="project_type"><option value="1">一</option><option value="3">三</option></select>
            <script>$("#level").val('C'); $("#test-level").val('2'); $("#project_type").val('3'); var author = 'a,b'.split(',');</script>"##,
        );
        for date in DATE_FIELDS {
            html.push_str(&format!(r#"<input id="{date}" value="{date}原值">"#));
        }
        html
    }
    #[test]
    fn edit_round_trips_hidden_values_and_only_adds_modules() {
        let page = page();
        let read = || parse_edit(&page, "11", "b").unwrap();
        let data = read().data;
        assert_eq!(data.input.user, "a");
        assert_eq!(data.options.default_user, "b");
        assert_eq!(data.existing_modules, ["1"]);
        assert_eq!(data.authors, ["a", "b"]);
        let mut input = data.input;
        let payload = read()
            .payload(&input, &data.authors, &data.revision)
            .unwrap();
        assert!(payload.get("git_pids").unwrap().is_null());
        assert_eq!(payload["name"], "项目&测试");
        assert_eq!(payload["name_en"], "v1");
        assert_eq!(payload["description"], " 说明 ");
        assert_eq!(payload["id"], "11");
        assert_eq!(payload["author"], json!(["a", "b"]));
        assert_eq!(payload["level"], "C");
        assert_eq!(payload["test_level"], "2");
        assert_eq!(payload["project_type"], "3");
        for date in DATE_FIELDS {
            assert_eq!(payload[date], format!("{date}原值"));
        }
        input.modules = vec!["2".into()];
        assert_eq!(
            read()
                .payload(&input, &data.authors, &data.revision)
                .unwrap()["git_pids"],
            json!(["2"])
        );
    }
    #[test]
    fn refuses_stale_revision_invalid_choices_and_duplicate_modules() {
        let page = page();
        let read = || parse_edit(&page, "11", "b").unwrap();
        let data = read().data;
        let mut input = data.input;
        for (from, to) in [
            ("release_time原值", "明天"),
            ("a,b", "b"),
            ("git_pid=\"1\"", "git_pid=\"2\""),
        ] {
            let changed = parse_edit(&page.replace(from, to), "11", "b").unwrap();
            assert!(changed
                .payload(&input, &data.authors, &data.revision)
                .unwrap_err()
                .starts_with("Conflict:"));
        }
        for modules in [vec!["1"], vec!["3"], vec!["2", "2"]] {
            input.modules = modules.into_iter().map(str::to_owned).collect();
            assert!(read()
                .payload(&input, &data.authors, &data.revision)
                .is_err());
        }
        input.modules.clear();
        for authors in [vec![], vec!["a", "a"], vec!["a", "other"]] {
            assert!(read()
                .payload(
                    &input,
                    &authors.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    &data.revision
                )
                .is_err());
        }
        input.branch_suffix = "/".into();
        assert!(read()
            .payload(&input, &data.authors, &data.revision)
            .is_err());
    }
    #[test]
    fn preserves_existing_author_missing_from_current_choices() {
        let html = page().replace("'a,b'.split", "'legacy,b'.split");
        let read = || parse_edit(&html, "11", "b").unwrap();
        let data = read().data;
        assert_eq!(data.input.user, "legacy");
        assert_eq!(
            read()
                .payload(&data.input, &data.authors, &data.revision)
                .unwrap()["author"],
            json!(["legacy", "b"])
        );
    }

    #[test]
    fn refuses_denied_incomplete_or_mismatched_page() {
        assert!(parse_edit("不是项目开发人员，不能编辑项目信息！", "11", "a").is_err());
        assert!(parse_edit("<input type=password>", "11", "a")
            .err()
            .unwrap()
            .starts_with("SessionExpired:"));
        assert!(parse_edit(&page(), "12", "a").is_err());
        for (from, to) in [
            ("<button ", "<button disabled "),
            ("plan_release_time", "missing"),
            ("var author", "var other"),
            ("val('C')", "val(compute())"),
        ] {
            assert!(parse_edit(&page().replace(from, to), "11", "a").is_err());
        }
    }
}
