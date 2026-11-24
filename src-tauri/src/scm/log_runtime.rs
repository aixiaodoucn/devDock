use super::*;

/// 日志站点独占会话和钥匙串条目，后端工作台可同时登录到另一站点。
pub struct LogRuntime(pub(super) Runtime);

impl Default for LogRuntime {
    fn default() -> Self {
        Self(Runtime::for_page(LoginPage::Prodlog))
    }
}

impl LogRuntime {
    pub async fn login(
        &self,
        base_url: String,
        username: String,
        password: String,
        remember: bool,
    ) -> Result<Status> {
        self.0.login(base_url, username, password, remember).await
    }

    pub async fn status(&self) -> Result<Status> {
        self.0.status().await
    }

    pub async fn logout(&self) -> Result<Status> {
        self.0.logout().await
    }

    pub async fn log_modules(&self) -> Result<Vec<(String, String)>> {
        let mut state = self.0.state.lock().await;
        self.0.restore(&mut state).await?;
        let session = state.session.as_mut().ok_or(SESSION_EXPIRED)?;
        let result = session
            .get(LoginPage::Prodlog.path(), &[])
            .await
            .and_then(|html| parse_log_modules(&html));
        if let Err(ref error) = result {
            self.0.expire(&mut state, error).await;
        } else if session.remembered {
            persist(session, self.0.credentials.clone()).await;
        }
        result
    }
}

pub(super) fn parse_log_modules(html: &str) -> Result<Vec<(String, String)>> {
    check_html(html)?;
    let document = Html::parse_document(html);
    let select = selector("select#git_pid");
    let mut selects = document.select(&select);
    let modules = selects.next().ok_or("Protocol: SCM 日志页缺少模块选择框")?;
    if selects.next().is_some() {
        return Err("Protocol: SCM 日志页存在多个模块选择框".into());
    }
    let mut result = Vec::new();
    let mut ids = HashSet::new();
    for option in modules.select(&selector("option")) {
        if option.attr("disabled").is_some()
            || option
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|ancestor| {
                    matches!(ancestor.value().name(), "select" | "optgroup")
                        && ancestor.attr("disabled").is_some()
                })
        {
            continue;
        }
        let id = option.attr("value").unwrap_or_default().trim();
        if !numeric_id(id) || id.trim_start_matches('0').is_empty() {
            continue;
        }
        let name = option
            .attr("label")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| text(option));
        if name.is_empty() || !ids.insert(id.to_owned()) {
            return Err("Protocol: SCM 日志模块名称为空或 ID 重复".into());
        }
        result.push((id.to_owned(), name));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_module_options_from_authenticated_page() {
        let html = r#"<select><option value='31'>无关</option></select>
            <select id='git_pid'><option value=''>请选择</option><option value='0'>全部</option>
            <option disabled value='33'>禁用</option><option value='7'> 日志 &amp; API </option>
            <option value='19' label='作业'>fallback</option></select>"#;
        assert_eq!(
            parse_log_modules(html).unwrap(),
            vec![
                ("7".into(), "日志 & API".into()),
                ("19".into(), "作业".into())
            ]
        );
        assert!(parse_log_modules("<select id=git_pid></select>")
            .unwrap()
            .is_empty());
        assert!(parse_log_modules("<select></select>").is_err());
        assert!(parse_log_modules(
            "<select id=git_pid><option value=7>a<option value=7>b</select>"
        )
        .is_err());
        assert_eq!(
            parse_log_modules(&format!("<input type=password>{html}")).unwrap_err(),
            SESSION_EXPIRED
        );
    }
}
