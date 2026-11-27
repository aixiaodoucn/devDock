use super::*;

#[path = "project_edit.rs"]
pub mod edit;
use serde_json::{json, Value};

#[derive(Serialize)]
pub struct Choice {
    value: String,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOptions {
    users: Vec<Choice>,
    modules: Vec<Choice>,
    types: Vec<Choice>,
    default_user: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateInput {
    name: String,
    branch_suffix: String,
    project_type: String,
    user: String,
    details: String,
    modules: Vec<String>,
}

struct Form {
    options: CreateOptions,
    level: String,
    test_level: String,
}

fn select_choices(doc: &Html, id: &str) -> Result<(Vec<Choice>, String)> {
    let element = doc
        .select(&selector(id))
        .next()
        .ok_or_else(|| "Parse: SCM 表单缺少预期字段".to_string())?;
    let mut choices = Vec::new();
    let mut selected = None;
    for option in element.select(&selector("option[value]:not([disabled])")) {
        let value = option.value().attr("value").unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        if option.value().attr("selected").is_some() {
            selected = Some(value.to_owned());
        }
        choices.push(Choice {
            value: value.into(),
            label: text(option),
        });
    }
    let default = selected
        .or_else(|| choices.first().map(|c| c.value.clone()))
        .unwrap_or_default();
    Ok((choices, default))
}

fn parse_form(html: &str, username: &str) -> Result<Form> {
    check_html(html)?;
    let doc = Html::parse_document(html);
    let select = |id| select_choices(&doc, id);
    let (users, _) = select("select#user")?;
    let default_user = users
        .iter()
        .find(|u| u.value.eq_ignore_ascii_case(username))
        .map(|u| u.value.clone())
        .unwrap_or_default();
    Ok(Form {
        options: CreateOptions {
            users,
            default_user,
            modules: select("select#mode")?.0,
            types: select("select#project_type")?.0,
        },
        level: select("select#level")?.1,
        test_level: select("select#test-level")?.1,
    })
}

impl CreateOptions {
    fn validate(&self, input: &CreateInput) -> Result<()> {
        let has = |choices: &[Choice], v: &str| choices.iter().any(|c| c.value == v);
        if input.name.trim().is_empty()
            || !input
                .branch_suffix
                .trim()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
        {
            return Err(
                "InvalidInput: 项目名称不能为空，分支后缀仅支持字母、数字、下划线和点".into(),
            );
        }
        if !has(&self.types, &input.project_type)
            || input.modules.iter().any(|id| !has(&self.modules, id))
        {
            return Err("InvalidInput: SCM 选项已变更，请重新打开表单".into());
        }
        Ok(())
    }
}

impl Form {
    fn payload(&self, input: &CreateInput) -> Result<Value> {
        self.options.validate(input)?;
        if !self.options.users.iter().any(|u| u.value == input.user) {
            return Err("InvalidInput: 负责人不在可选范围内".into());
        }
        // 原站将多选模块作为一组加入 mode，而负责人直接传数组。
        let mode = if input.modules.is_empty() {
            json!([])
        } else {
            json!([input.modules])
        };
        Ok(
            json!({"name":input.name.trim(),"name_en":input.branch_suffix.trim(),"author":[input.user],"description":input.details.trim(),"mode":mode,"project_type":input.project_type,"level":self.level,"test_level":self.test_level,"dev_start_time":"","dev_end_time":"","union_start_time":"","union_end_time":"","qa_start_time":"","qa_end_time":"","plan_release_time":""}),
        )
    }
}

fn write_response(body: &str, action: &str) -> Result<Value> {
    let unknown = || format!("SCM {action}结果未确认，请先刷新项目列表核对，不要重复提交");
    let res: Value = serde_json::from_str(body).map_err(|_| unknown())?;
    let code = &res["status"]["status_code"];
    if !code.is_string() && !code.is_number() {
        return Err(unknown());
    }
    if code.as_i64() != Some(0) && code.as_str() != Some("0") {
        return Err(format!(
            "SCM {action}失败：{}",
            res["status"]["status_reason"]
                .as_str()
                .unwrap_or("请核对项目信息")
        ));
    }
    Ok(res)
}

fn created_id(body: &str) -> Result<String> {
    let res = write_response(body, "创建")?;
    let id = res["pid"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| res["pid"].as_u64().map(|n| n.to_string()))
        .unwrap_or_default();
    Ok(if numeric_id(&id) { id } else { String::new() })
}

impl Session {
    async fn submit_form(&self, path: &str, param: String, action: &str) -> Result<String> {
        let url = self.base.join(path).map_err(|_| "SCM 地址无效")?;
        let response = self
            .client
            .post(url)
            .form(&[("param", param)])
            .send()
            .await
            .map_err(|_| format!("SCM {action}结果未确认，请先刷新项目列表核对，不要重复提交"))?;
        response_html(response, true).await.map_err(|e| {
            if e == SESSION_EXPIRED {
                e
            } else {
                format!("{e}；{action}结果未确认，请先刷新项目列表核对")
            }
        })
    }
    async fn create_form(&self) -> Result<Form> {
        parse_form(&self.get("/kpi/project/add", &[]).await?, &self.username)
    }
    async fn matching_projects(&self, input: &CreateInput) -> Result<HashSet<String>> {
        let html = self
            .get(
                "/kpi/project/list",
                &[
                    ("author", input.user.as_str()),
                    ("show", "1"),
                    ("project_name", input.name.trim()),
                ],
            )
            .await?;
        Ok(parse_projects(&html)?
            .into_iter()
            .filter(|p| p.name == input.name.trim())
            .map(|p| p.id)
            .collect())
    }
    async fn create_project(&self, input: &CreateInput) -> Result<String> {
        let param = self.create_form().await?.payload(input)?.to_string();
        // 旧版响应可能没有 pid。只匹配本次提交后唯一新增的同负责人、同名项目。
        let before = self.matching_projects(input).await?;
        let body = self
            .submit_form("/kpi/project/addSmt", param, "创建")
            .await?;
        let id = created_id(&body)?;
        if !id.is_empty() {
            return Ok(id);
        }
        if let Ok(after) = self.matching_projects(input).await {
            let mut added = after.difference(&before);
            if let (Some(id), None) = (added.next(), added.next()) {
                return Ok(id.clone());
            }
        }
        Ok(String::new())
    }
}

// Jar 区分每次登录；指纹检测同一 Jar 中的会话轮换，两者均不会进入审批事件。
#[derive(Clone)]
pub(crate) struct SessionBinding {
    pub site: String,
    pub account: String,
    jar: std::sync::Weak<Jar>,
    fingerprint: Option<[u8; 32]>,
}
impl SessionBinding {
    fn fingerprint(session: &Session) -> Option<[u8; 32]> {
        use sha2::{Digest, Sha256};
        session
            .cookie()
            .map(|cookie| Sha256::digest(cookie.as_bytes()).into())
    }
    pub(super) fn check(&self, session: &Session) -> Result<()> {
        if self.site == session.base.as_str()
            && self.account == session.username
            && self.jar.ptr_eq(&Arc::downgrade(&session.jar))
            && self.fingerprint == Self::fingerprint(session)
        {
            Ok(())
        } else {
            Err("项目 SCM 会话已变更，请重新发起审批".into())
        }
    }
}

impl Runtime {
    pub(crate) async fn session_binding(&self) -> Result<SessionBinding> {
        let mut state = self.state.lock().await;
        self.restore(&mut state).await?;
        let session = state.session.as_ref().ok_or(SESSION_EXPIRED)?;
        Ok(SessionBinding {
            site: session.base.to_string(),
            account: session.username.clone(),
            jar: Arc::downgrade(&session.jar),
            fingerprint: SessionBinding::fingerprint(session),
        })
    }

    pub(crate) async fn ensure_binding(&self, binding: &SessionBinding) -> Result<()> {
        let state = self.state.lock().await;
        binding.check(state.session.as_ref().ok_or(SESSION_EXPIRED)?)
    }

    pub(crate) async fn approved_create(
        &self,
        binding: &SessionBinding,
        input: CreateInput,
    ) -> Result<String> {
        self.project_form_session(async |session| {
            binding.check(session)?;
            session.create_project(&input).await
        })
        .await
    }

    pub(super) async fn project_form_session<T>(
        &self,
        f: impl AsyncFnOnce(&Session) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.state.lock().await;
        self.restore(&mut state).await?;
        let session = state
            .session
            .as_mut()
            .ok_or_else(|| SESSION_EXPIRED.to_owned())?;
        let result = f(session).await;
        if let Err(ref error) = result {
            self.expire(&mut state, error).await;
        } else if session.remembered {
            persist(session, self.credentials.clone()).await;
        }
        result
    }
    pub async fn create_options(&self) -> Result<CreateOptions> {
        self.project_form_session(async |session| Ok(session.create_form().await?.options))
            .await
    }
    pub async fn create(&self, input: CreateInput) -> Result<String> {
        self.project_form_session(async |session| session.create_project(&input).await)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FORM: &str = r#"<select id='user' multiple><option value='a'>甲</option><option value='b'>乙</option></select><select id='mode' multiple><option value='9'>模块</option></select><select id='project_type'><option value='1'>修复</option></select><select id='level'><option value='D'>D</option><option selected value='C'>C</option></select><select id='test-level'><option value='2'>正常</option></select>"#;
    #[test]
    fn approval_is_bound_to_login_instance_not_just_account_and_site() {
        let session = || {
            new_session(
                LoginPage::Projects,
                validate_base("https://example.invalid").unwrap(),
                "user".into(),
            )
            .unwrap()
        };
        let first = session();
        let binding = SessionBinding {
            site: first.base.to_string(),
            account: first.username.clone(),
            jar: Arc::downgrade(&first.jar),
            fingerprint: SessionBinding::fingerprint(&first),
        };
        assert!(binding.check(&first).is_ok());
        assert!(binding.check(&session()).is_err());
        first
            .jar
            .add_cookie_str("PHPSESSID=rotated; Path=/; Secure", &first.base);
        assert!(binding.check(&first).is_err());
    }

    #[test]
    fn builds_same_payload_as_the_scm_page() {
        let form = parse_form(FORM, "b").unwrap();
        assert_eq!(form.options.default_user, "b");
        let mut input = CreateInput {
            name: " 项目 ".into(),
            branch_suffix: String::new(),
            project_type: "1".into(),
            user: "b".into(),
            details: String::new(),
            modules: vec!["9".into()],
        };
        let payload = form.payload(&input).unwrap();
        assert_eq!(payload["author"], json!(["b"]));
        assert_eq!(payload["mode"], json!([["9"]]));
        assert_eq!(payload["level"], "C");
        assert_eq!(payload["test_level"], "2");
        assert_eq!(payload["name"], "项目");
        input.modules.clear();
        assert_eq!(form.payload(&input).unwrap()["mode"], json!([]));
        input.branch_suffix = "a/b".into();
        assert!(form.payload(&input).is_err());
    }
    #[test]
    fn requires_confirmed_success_and_valid_project_id() {
        assert_eq!(
            created_id(r#"{"status":{"status_code":"0"},"pid":123}"#).unwrap(),
            "123"
        );
        assert_eq!(created_id(r#"{"status":{"status_code":0}}"#).unwrap(), "");
        assert!(
            created_id(r#"{"status":{"status_code":1,"status_reason":"名称重复"}}"#)
                .unwrap_err()
                .contains("名称重复")
        );
        assert!(created_id("{}").is_err());
    }
}
