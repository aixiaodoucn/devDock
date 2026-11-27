use super::*;
use project_remove::unlink_payload;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Changes {
    name: Option<String>,
    branch_suffix: Option<String>,
    details: Option<String>,
    project_type: Option<String>,
    user: Option<String>,
    authors: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectPatch {
    project_id: String,
    revision: String,
    #[serde(default)]
    changes: Changes,
    #[serde(default)]
    add_module_ids: Vec<String>,
    #[serde(default)]
    remove_module_ids: Vec<String>,
}

const UNKNOWN: &str = "UnknownOutcome: 写入结果未确认，请重新读取项目，不要重复提交";

impl EditForm {
    // 模块为集合：允许上游回读时重新排序，但不能多一个或少一个关联。
    fn state_digest(&self) -> Result<String> {
        let mut ids = self.data.existing_modules.clone();
        ids.sort_unstable();
        project_revision(&self.original, &self.data.input, &self.data.authors, &ids)
    }

    fn prepare_patch(&mut self, patch: &ProjectPatch) -> Result<Option<Value>> {
        let ProjectPatch {
            changes,
            add_module_ids: add,
            remove_module_ids: remove,
            ..
        } = patch;
        if self.data.revision != patch.revision {
            return Err("Conflict: 项目已变化，请重新读取编辑选项".into());
        }
        if remove.len() > 128
            || add.len() > 128
            || !unique(add)
            || !unique(remove)
            || add.iter().chain(remove).any(|id| !numeric_id(id))
            || add.iter().any(|id| {
                remove.contains(id)
                    || self.data.existing_modules.contains(id)
                    || !self.data.options.modules.iter().any(|c| c.value == *id)
            })
            || remove.iter().any(|id| {
                self.data
                    .existing_modules
                    .iter()
                    .filter(|x| *x == id)
                    .count()
                    != 1
            })
        {
            return Err("InvalidInput: 模块不在可选范围内或重复".into());
        }
        let mut target = self.data.input.clone();
        if let Some(name) = &changes.name {
            if name.trim().is_empty() {
                return Err("InvalidInput: 项目名称不能为空".into());
            }
            target.name.clone_from(name);
        }
        if let Some(branch) = &changes.branch_suffix {
            let branch = branch.trim();
            if !branch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
            {
                return Err("InvalidInput: 分支后缀无效".into());
            }
            target.branch_suffix = branch.into();
        }
        if let Some(details) = &changes.details {
            target.details.clone_from(details);
        }
        if let Some(kind) = &changes.project_type {
            if !self.data.options.types.iter().any(|c| &c.value == kind) {
                return Err("InvalidInput: 项目类型无效".into());
            }
            target.project_type.clone_from(kind);
        }
        let mut authors = changes
            .authors
            .clone()
            .unwrap_or_else(|| self.data.authors.clone());
        if let Some(user) = &changes.user {
            if changes.authors.is_none() {
                authors.retain(|a| a != user);
                authors.insert(0, user.clone());
            } else if authors.first() != Some(user) {
                return Err("InvalidInput: user 必须与首位负责人一致".into());
            }
        }
        if changes.user.is_some() || changes.authors.is_some() {
            target.user = authors
                .first()
                .ok_or("InvalidInput: 负责人不能为空")?
                .clone();
            self.data
                .options
                .validate_authors(&target, &authors, &self.data.authors)?;
        }
        let fields = changes.name.is_some()
            || changes.branch_suffix.is_some()
            || changes.details.is_some()
            || changes.project_type.is_some()
            || changes.user.is_some()
            || changes.authors.is_some();
        if !fields && add.is_empty() && remove.is_empty() {
            return Err("InvalidInput: 没有变更内容".into());
        }
        let before = self.state_digest()?;
        target.modules.clone_from(add);
        let param = self.modify_param(&target, &authors, &target.branch_suffix);
        // modifySmt 的 git_pids 只传新增项，回读模型则保留所有未动的关联。
        target.modules.clear();
        self.data.input = target;
        self.data.authors = authors;
        self.data.existing_modules.extend_from_slice(add);
        Ok((self.state_digest()? != before).then_some(param))
    }
}

impl Session {
    async fn read_patch_page(&self, id: &str) -> Result<(String, EditForm)> {
        let html = self.get("/kpi/project/modify", &[("id", id)]).await?;
        let form = parse_edit(&html, id, &self.username)?;
        Ok((html, form))
    }

    async fn patch_project(&self, patch: &ProjectPatch) -> Result<Value> {
        let id = &patch.project_id;
        let (mut html, mut form) = self.edit_page(id).await?;
        let mut modify = form.prepare_patch(patch)?;
        // 在首次写入之前确认每个删除控件可用，避免先添加、再发现删除目标无效。
        for module in &patch.remove_module_ids {
            unlink_payload(&html, id, module, &patch.revision, &self.username)?;
        }
        let mut removals = patch.remove_module_ids.iter();
        let mut verified = false;
        let mut removed = 0;
        loop {
            // 仅报告已回读确认的连续解除数，不把当前不确定步骤算作成功。
            let failure = || {
                if verified {
                    format!("PartialUpdate:{removed}")
                } else {
                    UNKNOWN.to_owned()
                }
            };
            let is_removal = modify.is_none();
            let (path, param) = if let Some(param) = modify.take() {
                ("/kpi/project/modifySmt", param)
            } else if let Some(module) = removals.next() {
                let (param, _) =
                    unlink_payload(&html, id, module, &form.data.revision, &self.username)
                        .map_err(|e| if verified { failure() } else { e })?;
                form.data.existing_modules.retain(|m| m != module);
                ("/kpi/project/deleteMode", param)
            } else {
                break;
            };
            let expected = form.state_digest()?;
            // 任何 POST 均只发一次；即便上游报告成功，也必须回读校验后才允许下一步。
            let response = self.submit_form(path, param.to_string(), "修改").await;
            let accepted = response
                .and_then(|body| write_response(&body, "修改"))
                .is_ok();
            (html, form) = self.read_patch_page(id).await.map_err(|_| failure())?;
            if !accepted || form.state_digest().map_err(|_| failure())? != expected {
                return Err(failure());
            }
            if is_removal {
                removed += 1;
            }
            verified = true;
        }
        Ok(json!({"projectId":id,"status":"updated","revision":form.data.revision}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    fn page() -> String {
        super::super::tests::page().replace(
            "<i class=\"del_mode\" git_pid=\"1\"></i>",
            "<div><span><button class='del_mode' git_pid='1'></button></span></div>",
        )
    }
    fn patch(html: &str, changes: Value) -> ProjectPatch {
        serde_json::from_value(
            json!({"projectId":"11", "revision":parse_edit(html,"11","a").unwrap().data.revision,
            "changes":changes,"addModuleIds":["2"],"removeModuleIds":["1"]}),
        )
        .unwrap()
    }

    #[test]
    fn merges_changes_without_normalizing_omitted_values() {
        let html = page().replace("value=\"v1\"", "value=\" v1 \"");
        let make = || parse_edit(&html, "11", "a").unwrap();
        for (changes, expected) in [
            (json!({"user":"b"}), json!(["b", "a"])),
            (json!({"authors":["b"]}), json!(["b"])),
            (json!({"details":""}), json!(["a", "b"])),
        ] {
            let mut form = make();
            let param = form
                .prepare_patch(&patch(&html, changes.clone()))
                .unwrap()
                .unwrap();
            assert_eq!(param["author"], expected);
            assert_eq!(
                param["description"],
                changes.get("details").cloned().unwrap_or(json!(" 说明 "))
            );
            assert_eq!(param["name_en"], " v1 ");
            assert_eq!(param["level"], "C");
            assert_eq!(param["git_pids"], json!(["2"]));
        }
        for changes in [
            json!({"user":"a","authors":["b","a"]}),
            json!({"authors":[]}),
            json!({"authors":["bad"]}),
        ] {
            assert!(make().prepare_patch(&patch(&html, changes)).is_err());
        }
    }

    // 以本地脚本化响应检查真实 Session 的请求顺序、载荷与读回边界，不连接 SCM。
    fn run(steps: Vec<(&'static str, String)>, patch: ProjectPatch) -> (Result<Value>, Vec<Value>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let base = Url::parse(&format!("http://{address}/")).unwrap();
        let server = std::thread::spawn(move || {
            let mut params = Vec::new();
            for (expected, reply) in steps {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&stream);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                assert_eq!(first.split(" HTTP/").next(), Some(expected));
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                }
                if length > 0 {
                    let mut buf = vec![0; length];
                    reader.read_exact(&mut buf).unwrap();
                    let mut encoded = Url::parse("http://localhost/").unwrap();
                    encoded.set_query(Some(std::str::from_utf8(&buf).unwrap()));
                    let (key, value) = encoded.query_pairs().next().unwrap();
                    assert_eq!(key, "param");
                    params.push(serde_json::from_str(&value).unwrap());
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.len(),
                    reply
                )
                .unwrap();
            }
            // 结束标记必须紧跟最后一次预期请求；盲重试或多删一次都会失败。
            let (stream, _) = listener.accept().unwrap();
            let mut end = String::new();
            BufReader::new(stream).read_line(&mut end).unwrap();
            assert_eq!(end, "END\n");
            params
        });
        let mut session = new_session(LoginPage::Projects, base, "a".into()).unwrap();
        session.client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let result = tauri::async_runtime::block_on(session.patch_project(&patch));
        std::net::TcpStream::connect(address)
            .unwrap()
            .write_all(b"END\n")
            .unwrap();
        (result, server.join().unwrap())
    }

    #[test]
    fn reports_only_the_verified_removal_prefix() {
        let initial =
            page() + "<div><span><button class='del_mode' git_pid='7'></button></span></div>";
        let after = initial.replace(" git_pid='1'", "");
        let mut request = patch(&initial, json!({}));
        request.add_module_ids.clear();
        request.remove_module_ids.push("7".into());
        let read = "GET /kpi/project/modify?id=11";
        let delete = "POST /kpi/project/deleteMode";
        let (result, params) = run(
            vec![
                (read, initial),
                (
                    "GET /kpi/project/add",
                    super::super::super::tests::FORM.into(),
                ),
                (delete, r#"{"status":{"status_code":0}}"#.into()),
                (read, after.clone()),
                (delete, "rejected".into()),
                (read, after),
            ],
            request,
        );
        assert_eq!(result.unwrap_err(), "PartialUpdate:1");
        assert_eq!(
            params
                .iter()
                .map(|v| v["git_pid"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["1", "7"]
        );
    }

    #[test]
    fn preflights_all_targets_and_stops_at_unverified_write() {
        const READ: &str = "GET /kpi/project/modify?id=11";
        const ADD: &str = "POST /kpi/project/modifySmt";
        const REMOVE: &str = "POST /kpi/project/deleteMode";
        let ok = r#"{"status":{"status_code":0}}"#;
        let initial = page();
        let after_add = initial.replace(" 说明 ", "updated")
            + "<div><span><button class='del_mode' git_pid='2'></button></span></div>";
        let after_remove = after_add.replace(" git_pid='1'", "");
        for case in [
            "success",
            "stale",
            "disabled",
            "unknown",
            "concurrent",
            "partial",
            "delete_rejected",
            "lost_ack",
            "remove_only",
        ] {
            let html = if case == "disabled" {
                initial.replace("git_pid='1'", "disabled git_pid='1'")
            } else {
                initial.clone()
            };
            let mut request = patch(&html, json!({"details":"updated"}));
            let mut steps = vec![
                (READ, html),
                (
                    "GET /kpi/project/add",
                    super::super::super::tests::FORM.to_string(),
                ),
            ];
            match case {
                "stale" => request.revision = "old".into(),
                "disabled" => (),
                "remove_only" => {
                    request.changes = Changes::default();
                    request.add_module_ids.clear();
                    steps.extend([
                        (REMOVE, ok.into()),
                        (READ, initial.replace(" git_pid='1'", "")),
                    ]);
                }
                _ => {
                    steps.push((
                        ADD,
                        if case == "lost_ack" {
                            "bad reply".into()
                        } else {
                            ok.into()
                        },
                    ));
                    steps.push((
                        READ,
                        if case == "unknown" {
                            initial.clone()
                        } else if case == "concurrent" {
                            after_add.replace("plan_release_time原值", "其他人修改的日期")
                        } else {
                            after_add.clone()
                        },
                    ));
                    if matches!(case, "success" | "partial" | "delete_rejected") {
                        steps.extend([
                            (REMOVE, if case == "delete_rejected" { r#"{"status":{"status_code":1,"status_reason":"upstream-private"}}"#.into() } else { ok.into() }),
                            (
                                READ,
                                if matches!(case, "partial" | "delete_rejected") {
                                    after_add.clone()
                                } else {
                                    after_remove.clone()
                                },
                            ),
                        ]);
                    }
                }
            }
            let (result, params) = run(steps, request);
            match case {
                "success" => {
                    assert_eq!(
                        result.unwrap(),
                        json!({"projectId":"11","status":"updated","revision":parse_edit(&after_remove,"11","a").unwrap().data.revision})
                    );
                    assert_eq!(params[0]["git_pids"], json!(["2"]));
                    assert_eq!(params[1], json!({"id":11,"git_pid":"1"}));
                }
                "remove_only" => {
                    assert!(result.is_ok());
                    assert_eq!(params.len(), 1);
                }
                "unknown" | "lost_ack" | "concurrent" => {
                    assert_eq!(result.unwrap_err(), UNKNOWN);
                    assert_eq!(params.len(), 1);
                }
                "partial" | "delete_rejected" => {
                    assert_eq!(result.unwrap_err(), "PartialUpdate:0");
                    assert_eq!(params.len(), 2);
                }
                _ => {
                    assert!(result.is_err());
                    assert!(params.is_empty());
                }
            }
        }
    }
}

impl Runtime {
    pub(crate) async fn patch_project(&self, patch: ProjectPatch) -> Result<Value> {
        self.project_form_session(async |s| s.patch_project(&patch).await)
            .await
    }
}
