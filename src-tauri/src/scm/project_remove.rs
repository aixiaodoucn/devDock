use super::*;
use scraper::Element;

// 按原站 deleteMode 的行级参数构造请求，不接受客户端传入的 mode 或 URL。
fn unlink_payload(
    html: &str,
    project: &str,
    module: &str,
    revision: &str,
    account: &str,
) -> Result<Value> {
    let form = parse_edit(html, project, account)?;
    if form.data.revision != revision {
        return Err("Conflict: 项目已变化，请重新读取编辑选项".into());
    }
    if !numeric_id(module)
        || form
            .data
            .existing_modules
            .iter()
            .filter(|m| *m == module)
            .count()
            != 1
    {
        return Err("InvalidInput: 目标模块不是该项目的唯一关联项".into());
    }
    let doc = Html::parse_document(html);
    let button = doc
        .select(&selector(".del_mode[git_pid]:not([disabled])"))
        .find(|e| e.value().attr("git_pid") == Some(module))
        .ok_or(FORM_ERROR)?;
    let row = button
        .parent_element()
        .and_then(|p| p.parent_element())
        .ok_or(FORM_ERROR)?;
    if row.select(&selector(".del_mode[git_pid]")).count() != 1 {
        return Err(FORM_ERROR.into());
    }
    let id: u64 = project.parse().map_err(|_| FORM_ERROR)?;
    let mut param = json!({"id":id,"git_pid":module});
    // jQuery 对空选择器返回 undefined，JSON.stringify 会省略 mode。
    if let Some(select) = row.select(&selector("select")).next() {
        if select.value().attr("multiple").is_some() {
            return Err(FORM_ERROR.into());
        }
        let available = selector("option:not([disabled])");
        let mut options = select.select(&available);
        if let Some(item) = options
            .clone()
            .find(|o| o.value().attr("selected").is_some())
            .or_else(|| options.next())
        {
            param["mode"] = json!(item
                .value()
                .attr("value")
                .map(str::to_owned)
                .unwrap_or_else(|| text(item)));
        } else {
            param["mode"] = Value::Null;
        }
    }
    Ok(param)
}

impl Runtime {
    pub(crate) async fn approved_remove_module(
        &self,
        binding: &SessionBinding,
        project_id: String,
        module_id: String,
        revision: String,
    ) -> Result<()> {
        if !numeric_id(&project_id) || !numeric_id(&module_id) {
            return Err("InvalidInput: 项目和模块 ID 必须是数字".into());
        }
        self.project_form_session(async |session| {
            binding.check(session)?;
            let html = session
                .get("/kpi/project/modify", &[("id", &project_id)])
                .await?;
            let param =
                unlink_payload(&html, &project_id, &module_id, &revision, &session.username)?;
            let body = session
                .submit_form("/kpi/project/deleteMode", param.to_string(), "解除模块关联")
                .await?;
            write_response(&body, "解除模块关联").map(|_| ())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        super::super::tests::page().replace("<i class=\"del_mode\" git_pid=\"1\"></i>", "<div><span><button class='del_mode' git_pid='1'></button></span><select><option value='99'>other</option><option selected value='1'>existing</option></select></div>")
    }

    #[test]
    fn unlinks_only_the_selected_association_in_the_current_revision() {
        let html = fixture();
        let revision = parse_edit(&html, "11", "a").unwrap().data.revision;
        assert_eq!(
            unlink_payload(&html, "11", "1", &revision, "a").unwrap(),
            json!({"id":11,"git_pid":"1","mode":"1"})
        );
        for (project, module, version) in [
            ("12", "1", revision.as_str()),
            ("11", "2", revision.as_str()),
            ("11", "1", "old"),
        ] {
            assert!(unlink_payload(&html, project, module, version, "a").is_err());
        }
        assert!(unlink_payload(
            &html.replace("git_pid='1'", "disabled git_pid='1'"),
            "11",
            "1",
            &revision,
            "a"
        )
        .is_err());
    }
}
