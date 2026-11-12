//! SCM 的只读 HTML 协议适配；Cookie 和系统凭据始终留在 Rust 内。

use reqwest::{
    cookie::{CookieStore, Jar},
    redirect::Policy,
    Client, Url,
};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tauri::{async_runtime::Mutex, AppHandle, Manager};

type Result<T> = std::result::Result<T, String>;
const SESSION_EXPIRED: &str = "SessionExpired: SCM 会话已过期，请重新登录";
const CREDENTIAL_WARNING: &str = "系统凭据存储不可用；当前仅内存登录，无法持久记住会话";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// 仅在项目列表通过远端验证后为 true。
    pub connected: bool,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub remembered: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScmProject {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScmModule {
    pub git_id: Option<String>,
    pub module_name: String,
    pub git_remote_url: Option<String>,
    pub branch: String,
}

// 此类型不能导出给前端，也不实现 Debug，避免意外泄露会话。
#[derive(Serialize, Deserialize)]
struct RememberedSession {
    base_url: String,
    username: String,
    cookie: String,
}

trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<String>>;
    fn save(&self, value: &str) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

struct SystemCredentialStore;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SystemCredentialStore {
    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new("com.devdock.desktop.scm", "default")
            .map_err(|_| CREDENTIAL_WARNING.into())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl CredentialStore for SystemCredentialStore {
    fn load(&self) -> Result<Option<String>> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CREDENTIAL_WARNING.into()),
        }
    }
    fn save(&self, value: &str) -> Result<()> {
        self.entry()?
            .set_password(value)
            .map_err(|_| CREDENTIAL_WARNING.into())
    }
    fn clear(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => {
                Err("Credential: 系统中已记住的 SCM 会话未能删除，请在系统凭据管理中移除".into())
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl CredentialStore for SystemCredentialStore {
    fn load(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: &str) -> Result<()> {
        Err("当前平台不支持持久记住 SCM 会话；当前仅内存登录".into())
    }
    fn clear(&self) -> Result<()> {
        Ok(())
    }
}

struct Session {
    base: Url,
    username: String,
    jar: Arc<Jar>,
    client: Client,
    remembered: bool,
    // 仅留在 Rust 内存；成功保存的 Cookie 用于避免重复触发系统凭据授权。
    saved_cookie: Option<String>,
    persistence_failed: bool,
    message: Option<String>,
}

#[derive(Default)]
struct RuntimeState {
    loaded: bool,
    // None 表示尚未读取或读取失败；显式内存登录仍需清理可能存在的旧账号。
    credential_present: Option<bool>,
    session: Option<Session>,
    message: Option<String>,
}

pub struct Runtime {
    state: Mutex<RuntimeState>,
    credentials: Arc<dyn CredentialStore>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            state: Mutex::new(RuntimeState::default()),
            credentials: Arc::new(SystemCredentialStore),
        }
    }
}

fn validate_base(value: &str) -> Result<Url> {
    let url = Url::parse(value.trim()).map_err(|_| "InvalidInput: SCM 地址格式无效".to_string())?;
    if value.contains('@')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(
            "InvalidInput: SCM 地址必须为 HTTPS 根地址，不能包含凭据、路径、查询或片段".into(),
        );
    }
    Ok(url)
}

fn allowed_redirect(base: &Url, target: &Url, hops: usize) -> bool {
    hops <= 5
        && target.origin() == base.origin()
        && target.scheme() == "https"
        && target.username().is_empty()
        && target.password().is_none()
}

fn new_session(base: Url, username: String) -> Result<Session> {
    let origin = base.clone();
    let jar = Arc::new(Jar::default());
    let client = Client::builder()
        .cookie_provider(jar.clone())
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .redirect(Policy::custom(move |attempt| {
            if allowed_redirect(&origin, attempt.url(), attempt.previous().len()) {
                attempt.follow()
            } else {
                attempt.error("SCM redirect rejected")
            }
        }))
        .build()
        .map_err(|_| "Network: 无法创建 SCM HTTPS 客户端".to_string())?;
    Ok(Session {
        base,
        username,
        jar,
        client,
        remembered: false,
        saved_cookie: None,
        persistence_failed: false,
        message: None,
    })
}

impl Session {
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let url = self
            .base
            .join(path)
            .map_err(|_| "InvalidInput: SCM 请求路径无效".to_string())?;
        let response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(network_error)?;
        response_html(response, true).await
    }

    fn cookie(&self) -> Option<String> {
        let url = self.base.join("/kpi/project/list").ok()?;
        let header = self.jar.cookies(&url)?;
        header.to_str().ok()?.split(';').find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == "PHPSESSID" && valid_cookie(value)).then(|| value.to_owned())
        })
    }

    fn status(&self) -> Status {
        Status {
            connected: true,
            base_url: Some(self.base.to_string()),
            username: Some(self.username.clone()),
            remembered: self.remembered,
            message: self.message.clone(),
        }
    }
}

fn valid_cookie(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_,".contains(&b))
}

fn network_error(error: reqwest::Error) -> String {
    // reqwest 错误可能携带 URL、重定向信息等，不直接输出底层错误。
    if error.is_timeout() {
        "Network: SCM 请求超时，请稍后重试".into()
    } else if error.is_redirect() {
        "Network: SCM 重定向超出五跳或离开同一 HTTPS 站点，已拒绝".into()
    } else {
        "Network: 无法安全连接 SCM，请检查地址、网络及系统证书".into()
    }
}

async fn response_html(response: reqwest::Response, authenticated: bool) -> Result<String> {
    if authenticated
        && (is_login_path(response.url()) || response.status() == reqwest::StatusCode::UNAUTHORIZED)
    {
        return Err(SESSION_EXPIRED.into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Network: SCM 返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let text = response.text().await.map_err(network_error)?;
    if authenticated && is_login_page(&text) {
        return Err(SESSION_EXPIRED.into());
    }
    Ok(text)
}

fn is_login_path(url: &Url) -> bool {
    matches!(
        url.path().trim_end_matches('/'),
        "/kpi/login" | "/kpi/loginSmt"
    )
}

async fn credential_load(store: Arc<dyn CredentialStore>) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || store.load())
        .await
        .map_err(|_| "Credential: 无法读取系统凭据".to_string())?
}
async fn credential_clear(store: Arc<dyn CredentialStore>) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || store.clear())
        .await
        .map_err(|_| "Credential: 无法清除系统凭据".to_string())?
}
async fn persist(session: &mut Session, store: Arc<dyn CredentialStore>) {
    // 保存失败后保留当前内存会话，查询不再自动重试，避免反复弹出授权。
    if session.persistence_failed {
        return;
    }
    let Some(cookie) = session.cookie() else {
        session.remembered = false;
        session.persistence_failed = true;
        session.message = Some("SCM 未提供可记住的会话 Cookie；当前仅内存登录".into());
        return;
    };
    if session.saved_cookie.as_deref() == Some(cookie.as_str()) {
        return;
    }
    let data = RememberedSession {
        base_url: session.base.to_string(),
        username: session.username.clone(),
        cookie: cookie.clone(),
    };
    let result = match serde_json::to_string(&data) {
        Ok(value) => tauri::async_runtime::spawn_blocking(move || store.save(&value))
            .await
            .unwrap_or_else(|_| Err(CREDENTIAL_WARNING.into())),
        Err(_) => Err(CREDENTIAL_WARNING.into()),
    };
    session.remembered = result.is_ok();
    session.persistence_failed = result.is_err();
    if result.is_ok() {
        session.saved_cookie = Some(cookie);
    }
    session.message = result.err();
}

impl RuntimeState {
    fn begin_login(&mut self) {
        // 不恢复或删除旧凭据；失败登录也不能暴露或重新启用旧内存会话。
        self.loaded = true;
        self.session = None;
        self.message = None;
    }
}

impl Runtime {
    async fn clear_remembered(&self, state: &mut RuntimeState) {
        state.message = match credential_clear(self.credentials.clone()).await {
            Ok(()) => {
                state.credential_present = Some(false);
                None
            }
            Err(message) => Some(message),
        };
    }

    async fn finish_login(
        &self,
        state: &mut RuntimeState,
        mut session: Session,
        remember: bool,
    ) -> Status {
        if remember {
            persist(&mut session, self.credentials.clone()).await;
            if session.remembered {
                state.credential_present = Some(true);
            }
        } else if state.credential_present != Some(false) {
            // 未读取时直接删除一次，避免先读再删触发两次授权，也防止下次恢复旧账号。
            self.clear_remembered(state).await;
            session.message = state.message.clone();
        }
        let status = session.status();
        state.session = Some(session);
        status
    }

    async fn restore(&self, state: &mut RuntimeState) -> Result<()> {
        if state.loaded {
            return Ok(());
        }
        state.loaded = true;
        let value = match credential_load(self.credentials.clone()).await {
            Ok(value) => value,
            Err(message) => {
                state.message = Some(message);
                return Ok(());
            }
        };
        state.credential_present = Some(value.is_some());
        let Some(value) = value else {
            return Ok(());
        };
        let remembered = serde_json::from_str::<RememberedSession>(&value)
            .map_err(|_| "Credential: 已记住的 SCM 会话格式无效，请重新登录".to_string())?;
        if !valid_cookie(&remembered.cookie) {
            return Err("Credential: 已记住的 SCM 会话格式无效，请重新登录".into());
        }
        let mut session = new_session(validate_base(&remembered.base_url)?, remembered.username)?;
        session.jar.add_cookie_str(
            &format!("PHPSESSID={}; Path=/; Secure; HttpOnly", remembered.cookie),
            &session.base,
        );
        session.remembered = true;
        session.saved_cookie = Some(remembered.cookie);
        state.session = Some(session);
        Ok(())
    }

    async fn expire(&self, state: &mut RuntimeState, error: &str) {
        if error.starts_with("SessionExpired:") {
            state.session = None;
            self.clear_remembered(state).await;
        }
    }
}

pub async fn login(
    app: &AppHandle,
    base_url: String,
    username: String,
    password: String,
    remember: bool,
) -> Result<Status> {
    let runtime = app.state::<Runtime>();
    runtime.login(base_url, username, password, remember).await
}
pub async fn status(app: &AppHandle) -> Result<Status> {
    app.state::<Runtime>().status().await
}
pub async fn logout(app: &AppHandle) -> Result<Status> {
    app.state::<Runtime>().logout().await
}
pub async fn projects(app: &AppHandle) -> Result<Vec<ScmProject>> {
    app.state::<Runtime>().projects().await
}
pub async fn project_modules(app: &AppHandle, project_id: String) -> Result<Vec<ScmModule>> {
    app.state::<Runtime>().project_modules(project_id).await
}

impl Runtime {
    pub async fn login(
        &self,
        base_url: String,
        username: String,
        password: String,
        remember: bool,
    ) -> Result<Status> {
        let mut state = self.state.lock().await;
        state.begin_login();
        let base = validate_base(&base_url)?;
        let username = username.trim().to_string();
        if username.is_empty() || password.is_empty() {
            return Err("InvalidInput: 请输入 SCM 账号和密码".into());
        }
        let session = new_session(base, username)?;
        let login_url = session
            .base
            .join("/kpi/login")
            .map_err(|_| "InvalidInput: 登录地址无效".to_string())?;
        let response = session
            .client
            .get(login_url)
            .query(&[("ref", "/kpi/project/list")])
            .send()
            .await
            .map_err(network_error)?;
        response_html(response, false).await?;
        let login_url = session
            .base
            .join("/kpi/loginSmt")
            .map_err(|_| "InvalidInput: 登录地址无效".to_string())?;
        let response = session
            .client
            .post(login_url)
            .form(&[
                ("username", session.username.as_str()),
                ("password", password.as_str()),
                ("ref", "/kpi/project/list"),
                ("act", "loginSubmit"),
                ("submit", "Enter"),
            ])
            .send()
            .await
            .map_err(network_error)?;
        let html = response_html(response, false).await?;
        if is_login_page(&html) {
            return Err("Authentication: SCM 登录未成功，请检查账号和密码".into());
        }
        let html = session.get("/kpi/project/list", &[]).await?;
        parse_projects(&html)?;
        if session.cookie().is_none() {
            return Err("Authentication: SCM 未建立有效会话，请重新登录".into());
        }
        Ok(self.finish_login(&mut state, session, remember).await)
    }

    pub async fn status(&self) -> Result<Status> {
        let mut state = self.state.lock().await;
        self.restore(&mut state).await?;
        let Some(session) = state.session.as_mut() else {
            return Ok(Status {
                connected: false,
                base_url: None,
                username: None,
                remembered: false,
                message: state.message.clone(),
            });
        };
        let result = session
            .get("/kpi/project/list", &[])
            .await
            .and_then(|html| parse_projects(&html));
        if let Err(error) = result {
            self.expire(&mut state, &error).await;
            return Err(error);
        }
        if session.remembered {
            persist(session, self.credentials.clone()).await;
        }
        Ok(session.status())
    }

    pub async fn logout(&self) -> Result<Status> {
        let mut state = self.state.lock().await;
        state.loaded = true;
        state.session = None;
        self.clear_remembered(&mut state).await;
        // 仅清本地会话，不请求站点的 loginOut。
        Ok(Status {
            connected: false,
            base_url: None,
            username: None,
            remembered: false,
            message: state.message.clone(),
        })
    }

    pub async fn projects(&self) -> Result<Vec<ScmProject>> {
        let mut state = self.state.lock().await;
        self.restore(&mut state).await?;
        let session = state
            .session
            .as_mut()
            .ok_or_else(|| SESSION_EXPIRED.to_string())?;
        let result = session
            .get("/kpi/project/list", &[])
            .await
            .and_then(|html| parse_projects(&html));
        if let Err(ref error) = result {
            self.expire(&mut state, error).await;
        } else if session.remembered {
            persist(session, self.credentials.clone()).await;
        }
        result
    }

    pub async fn project_modules(&self, project_id: String) -> Result<Vec<ScmModule>> {
        if !numeric_id(&project_id) {
            return Err("InvalidInput: SCM 项目 ID 必须为数字".into());
        }
        let mut state = self.state.lock().await;
        self.restore(&mut state).await?;
        let session = state
            .session
            .as_mut()
            .ok_or_else(|| SESSION_EXPIRED.to_string())?;
        let result = async {
            let html = session
                .get(
                    "/kpi/project/project",
                    &[("show_type", "2"), ("id", &project_id)],
                )
                .await?;
            let repos = session.get("/kpi/gitmode/list", &[]).await?;
            parse_modules(&html, &parse_repositories(&repos)?)
        }
        .await;
        if let Err(ref error) = result {
            self.expire(&mut state, error).await;
        } else if session.remembered {
            persist(session, self.credentials.clone()).await;
        }
        result
    }
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static SCM selector")
}
fn text(element: ElementRef<'_>) -> String {
    element.text().collect::<String>().trim().to_string()
}
fn numeric_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit())
}
fn is_login_page(html: &str) -> bool {
    let document = Html::parse_document(html);
    document
        .select(&selector(
            "input[type='password'], input[name='password'], form[action*='loginSmt']",
        ))
        .next()
        .is_some()
}
fn check_html(html: &str) -> Result<()> {
    if is_login_page(html) {
        Err(SESSION_EXPIRED.into())
    } else {
        Ok(())
    }
}

fn table_rows(html: &str, required: &[&str]) -> Result<(Vec<String>, Vec<Vec<ElementData>>)> {
    check_html(html)?;
    let document = Html::parse_document(html);
    for table in document.select(&selector("table")) {
        let mut headers = None;
        let mut rows = Vec::new();
        for row in table.select(&selector("tr")) {
            let cells: Vec<_> = row.select(&selector("th, td")).collect();
            let labels: Vec<_> = cells
                .iter()
                .map(|cell| text(*cell).replace(char::is_whitespace, ""))
                .collect();
            if headers.is_none()
                && required
                    .iter()
                    .all(|name| labels.iter().any(|label| label.eq_ignore_ascii_case(name)))
            {
                headers = Some(labels);
            } else if headers.is_some() {
                rows.push(
                    cells
                        .iter()
                        .map(|cell| ElementData {
                            text: text(*cell),
                            links: cell
                                .select(&selector("a[href]"))
                                .filter_map(|link| link.value().attr("href").map(str::to_owned))
                                .collect(),
                        })
                        .collect(),
                );
            }
        }
        if let Some(headers) = headers {
            return Ok((headers, rows));
        }
    }
    Err("Parse: SCM 页面缺少预期的表格结构，请确认账号权限或站点版本".into())
}
struct ElementData {
    text: String,
    links: Vec<String>,
}
fn column(headers: &[String], name: &str) -> usize {
    headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(name))
        .expect("validated table header")
}
fn query_id(link: &str, name: &str) -> Option<String> {
    let base = Url::parse("https://example.invalid/").ok()?;
    let url = base.join(link).ok()?;
    url.query_pairs()
        .find_map(|(key, value)| (key == name && numeric_id(&value)).then(|| value.into_owned()))
}

fn parse_projects(html: &str) -> Result<Vec<ScmProject>> {
    let (headers, rows) = table_rows(html, &["项目ID", "项目名称", "项目状态"])?;
    let id_col = column(&headers, "项目ID");
    let name_col = column(&headers, "项目名称");
    let status_col = column(&headers, "项目状态");
    let mut projects = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let (Some(id), Some(name), Some(status)) =
            (row.get(id_col), row.get(name_col), row.get(status_col))
        else {
            continue;
        };
        if !numeric_id(&id.text) || name.text.is_empty() {
            continue;
        }
        let linked = row.iter().flat_map(|cell| &cell.links).any(|link| {
            link.contains("/kpi/project/project")
                && query_id(link, "id").as_deref() == Some(&id.text)
        });
        if !linked {
            continue;
        }
        if !seen.insert(id.text.clone()) {
            return Err("Parse: SCM 项目 ID 重复".into());
        }
        projects.push(ScmProject {
            id: id.text.clone(),
            name: name.text.clone(),
            status: (!status.text.is_empty()).then(|| status.text.clone()),
        });
    }
    Ok(projects)
}

struct Repository {
    id: String,
    name: String,
    remote: Option<String>,
}
fn parse_repositories(html: &str) -> Result<Vec<Repository>> {
    let (headers, rows) = table_rows(html, &["gitID", "模块名", "git地址"])?;
    let mut repos = Vec::new();
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for row in rows {
        let (Some(id), Some(name), Some(remote)) = (
            row.get(column(&headers, "gitID")),
            row.get(column(&headers, "模块名")),
            row.get(column(&headers, "git地址")),
        ) else {
            continue;
        };
        if !numeric_id(&id.text) || name.text.is_empty() {
            continue;
        }
        if !ids.insert(id.text.clone()) || !names.insert(name.text.clone()) {
            return Err("Parse: SCM 仓库映射重复，无法确定模块".into());
        }
        repos.push(Repository {
            id: id.text.clone(),
            name: name.text.clone(),
            // 无地址的其他模块不应阻断整个目录，保留 ID 供用户明确匹配。
            remote: sanitize_remote(&remote.text).ok(),
        });
    }
    Ok(repos)
}

fn sanitize_remote(value: &str) -> Result<String> {
    let invalid = || "Parse: SCM 仓库地址格式无效或包含不安全凭据".to_string();
    if value.chars().any(char::is_whitespace) || value.is_empty() {
        return Err(invalid());
    }
    if value.contains("://") {
        let mut url = Url::parse(value).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "https" | "http" | "ssh" | "git") || url.host_str().is_none() {
            return Err(invalid());
        }
        // URL 形式去掉全部 userInfo（包括 SSH 用户名）；scp 形式单独验证。
        url.set_username("").map_err(|_| invalid())?;
        url.set_password(None).map_err(|_| invalid())?;
        url.set_query(None);
        url.set_fragment(None);
        return Ok(url.to_string());
    }
    if let Some((user_host, path)) = value.split_once(':') {
        if let Some((user, host)) = user_host.split_once('@') {
            if !user.is_empty()
                && !host.is_empty()
                && !path.is_empty()
                && user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                && !path.contains('@')
            {
                return Ok(value.to_string());
            }
        }
    }
    Err(invalid())
}

fn parse_modules(html: &str, repositories: &[Repository]) -> Result<Vec<ScmModule>> {
    check_html(html)?;
    let document = Html::parse_document(html);
    let branch_pattern =
        regex::Regex::new(r"分支\s*[:：]\s*([^\s]+)").expect("static branch regex");
    let mut modules = Vec::new();
    let mut seen = HashSet::new();
    for container in document.select(&selector(".modelist")) {
        // 页面包含供“添加模块”克隆的隐藏模板，不能当作已关联模块解析。
        if container.value().attr("id") == Some("mode_template")
            || container
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|ancestor| ancestor.value().attr("id") == Some("mode_template"))
        {
            continue;
        }
        let Some(module) = container.select(&selector(".subpro[id]")).next() else {
            return Err("Parse: SCM 模块缺少名称".into());
        };
        let name = module.value().attr("id").unwrap_or_default().trim();
        if name.is_empty() || !seen.insert(name.to_string()) {
            return Err("Parse: SCM 模块名称缺失或重复".into());
        }
        let branch = container
            .select(&selector(".message"))
            .find_map(|message| {
                branch_pattern
                    .captures(&text(message))
                    .map(|capture| capture[1].to_string())
            })
            .ok_or_else(|| "Parse: SCM 模块缺少分支信息".to_string())?;
        let git_ids: HashSet<_> = container
            .select(&selector("a[href]"))
            .filter_map(|a| query_id(a.value().attr("href")?, "g_pid"))
            .collect();
        if git_ids.len() > 1 {
            return Err("Parse: SCM 模块存在冲突的仓库 ID".into());
        }
        let git_id = git_ids.into_iter().next();
        let by_name = repositories.iter().find(|repo| repo.name == name);
        let by_id = git_id
            .as_ref()
            .and_then(|id| repositories.iter().find(|repo| &repo.id == id));
        if by_id.is_some_and(|repo| repo.name != name)
            || by_name.is_some_and(|repo| git_id.as_ref().is_some_and(|id| &repo.id != id))
        {
            return Err("Parse: SCM 模块名称与仓库 ID 映射不一致".into());
        }
        let repo = by_id.or(by_name);
        modules.push(ScmModule {
            git_id: git_id.or_else(|| repo.map(|r| r.id.clone())),
            module_name: name.to_string(),
            git_remote_url: repo.and_then(|r| r.remote.clone()),
            branch,
        });
    }
    if modules.is_empty() {
        return Err("Parse: SCM 项目页未找到模块结构".into());
    }
    Ok(modules)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECTS: &str = r#"<table><tr><th>项目名称</th><th>项目状态</th><th>项目ID</th></tr>
      <tr><td><a href='/kpi/project/project?id=12'>Example app</a></td><td>开发中</td><td>12</td></tr>
      <tr><td>无项目链接</td><td>完成</td><td>99</td></tr></table>"#;
    const REPOS: &str = r#"<table><tr><th>模块名</th><th>git地址</th><th>gitID</th></tr>
      <tr><td>example-api</td><td>git@example.invalid:group/api.git</td><td>7</td></tr></table>"#;
    const MODULES: &str = r#"<div class='modelist'><div class='subpro' id='example-api'></div>
      <div class='message'>分支: feature/example-12</div><a href='/kpi/log?g_pid=7'>日志</a></div>"#;

    #[test]
    fn ignores_hidden_module_template_and_tolerates_missing_repository_address() {
        let html = format!("<div id='mode_template'><div class='modelist'><div class='subpro_add'></div></div></div>{MODULES}");
        let repositories = parse_repositories(REPOS).unwrap();
        assert_eq!(parse_modules(&html, &repositories).unwrap().len(), 1);
        let html = REPOS.replace(
            "</table>",
            "<tr><td>unrelated-module</td><td></td><td>99</td></tr></table>",
        );
        let repositories = parse_repositories(&html).unwrap();
        assert_eq!(
            parse_modules(MODULES, &repositories).unwrap()[0]
                .git_id
                .as_deref(),
            Some("7")
        );
        let missing = REPOS.replace("git@example.invalid:group/api.git", "");
        let repositories = parse_repositories(&missing).unwrap();
        let modules = parse_modules(MODULES, &repositories).unwrap();
        assert!(modules[0].git_remote_url.is_none());
        assert_eq!(modules[0].branch, "feature/example-12");
    }

    #[test]
    fn rejects_non_root_and_unsafe_urls() {
        for value in [
            "http://example.invalid",
            "https://u:p@example.invalid",
            "https://@example.invalid",
            "https://example.invalid/\n",
            "https://example.invalid/path",
            "https://example.invalid/?q=x",
            "https://example.invalid/#x",
        ] {
            assert!(validate_base(value).is_err());
        }
        assert!(validate_base("https://example.invalid/").is_ok());
        let base = validate_base("https://example.invalid").unwrap();
        assert!(allowed_redirect(
            &base,
            &base.join("/kpi/project/list").unwrap(),
            5
        ));
        assert!(!allowed_redirect(&base, &base, 6));
        assert!(!allowed_redirect(
            &base,
            &Url::parse("https://other.example.invalid/").unwrap(),
            1
        ));
        assert!(!allowed_redirect(
            &base,
            &Url::parse("http://example.invalid/").unwrap(),
            1
        ));
    }

    #[test]
    fn parses_project_headers_and_filters_non_project_rows() {
        let rows = parse_projects(PROJECTS).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "12");
        assert_eq!(rows[0].name, "Example app");
        assert_eq!(rows[0].status.as_deref(), Some("开发中"));
        assert!(parse_projects("<html>unexpected</html>").is_err());
        assert!(parse_projects(&PROJECTS.replace("id=12", "id=bad"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn detects_expired_session_even_with_200() {
        let html = "<form action='/kpi/loginSmt'><input name='username'><input name='password' type='password'></form>";
        assert!(is_login_page(html));
        assert!(parse_projects(html)
            .unwrap_err()
            .starts_with("SessionExpired:"));
    }

    #[test]
    fn module_branch_mapping_and_missing_mapping() {
        let repos = parse_repositories(REPOS).unwrap();
        let rows = parse_modules(MODULES, &repos).unwrap();
        assert_eq!(rows[0].git_id.as_deref(), Some("7"));
        assert_eq!(rows[0].branch, "feature/example-12");
        assert_eq!(
            rows[0].git_remote_url.as_deref(),
            Some("git@example.invalid:group/api.git")
        );
        let missing = parse_modules(MODULES, &[]).unwrap();
        assert!(missing[0].git_remote_url.is_none());
        assert!(parse_modules(&MODULES.repeat(2), &repos).is_err());
        assert!(parse_modules(&MODULES.replace("feature/example-12", ""), &repos).is_err());
        assert!(parse_repositories(&REPOS.replace(
            "</table>",
            &format!(
                "{} </table>",
                "<tr><td>example-api</td><td>https://example.invalid/a.git</td><td>7</td></tr>"
            )
        ))
        .is_err());
    }

    #[test]
    fn strips_remote_credentials_and_rejects_unsafe_remotes() {
        assert_eq!(
            sanitize_remote("https://user:secret@example.invalid/a.git").unwrap(),
            "https://example.invalid/a.git"
        );
        assert!(sanitize_remote("javascript:alert(1)").is_err());
        assert!(sanitize_remote("git:secret@example.invalid:a.git").is_err());
    }

    #[derive(Default)]
    struct FakeCredentialStore {
        value: std::sync::Mutex<Option<String>>,
        fail: bool,
        loads: std::sync::atomic::AtomicUsize,
        saves: std::sync::atomic::AtomicUsize,
        clears: std::sync::atomic::AtomicUsize,
    }
    impl FakeCredentialStore {
        fn counts(&self) -> (usize, usize, usize) {
            use std::sync::atomic::Ordering::SeqCst;
            (
                self.loads.load(SeqCst),
                self.saves.load(SeqCst),
                self.clears.load(SeqCst),
            )
        }
    }
    impl CredentialStore for FakeCredentialStore {
        fn load(&self) -> Result<Option<String>> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(CREDENTIAL_WARNING.into());
            }
            Ok(self.value.lock().unwrap().clone())
        }
        fn save(&self, value: &str) -> Result<()> {
            self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(CREDENTIAL_WARNING.into());
            }
            *self.value.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
        fn clear(&self) -> Result<()> {
            self.clears
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err("Credential: 无法删除已记住会话".into());
            }
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    fn cookie_session(cookie: &str) -> Session {
        let session = new_session(
            validate_base("https://example.invalid").unwrap(),
            "example-user".into(),
        )
        .unwrap();
        session.jar.add_cookie_str(
            &format!("PHPSESSID={cookie}; Path=/; Secure; HttpOnly"),
            &session.base,
        );
        session
    }

    #[test]
    fn unchanged_cookie_never_rewrites_and_restored_cookie_is_already_saved() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore::default());
            let mut session = cookie_session("synthetic-session");
            persist(&mut session, store.clone()).await;
            assert_eq!(store.counts(), (0, 1, 0));
            persist(&mut session, store.clone()).await;
            assert_eq!(store.counts(), (0, 1, 0));
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store.clone(),
            };
            let mut state = runtime.state.lock().await;
            for _ in 0..3 {
                runtime.restore(&mut state).await.unwrap();
                persist(state.session.as_mut().unwrap(), store.clone()).await;
            }
            assert_eq!(store.counts(), (1, 1, 0));
            let session = state.session.as_mut().unwrap();
            session.jar.add_cookie_str(
                "PHPSESSID=rotated-session; Path=/; Secure; HttpOnly",
                &session.base,
            );
            persist(session, store.clone()).await;
            persist(session, store.clone()).await;
            assert_eq!(store.counts(), (1, 2, 0));
        });
    }

    #[test]
    fn failed_save_is_not_retried_by_queries() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore {
                fail: true,
                ..Default::default()
            });
            let mut session = cookie_session("synthetic-session");
            persist(&mut session, store.clone()).await;
            persist(&mut session, store.clone()).await;
            session.jar.add_cookie_str(
                "PHPSESSID=rotated-session; Path=/; Secure; HttpOnly",
                &session.base,
            );
            persist(&mut session, store.clone()).await;
            assert_eq!(store.counts(), (0, 1, 0));
            assert!(!session.remembered);
            assert!(session.message.as_deref().unwrap().contains("仅内存"));
            assert_eq!(session.cookie().as_deref(), Some("rotated-session"));
        });
    }

    #[test]
    fn login_credential_flow_overwrites_without_delete_and_memory_login_clears_once() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore::default());
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store.clone(),
            };
            let mut state = runtime.state.lock().await;
            state.begin_login();
            let status = runtime
                .finish_login(&mut state, cookie_session("first-session"), true)
                .await;
            assert!(status.remembered);
            assert_eq!(store.counts(), (0, 1, 0));
            state.begin_login();
            runtime
                .finish_login(&mut state, cookie_session("second-session"), true)
                .await;
            assert_eq!(store.counts(), (0, 2, 0));
            state.begin_login();
            let status = runtime
                .finish_login(&mut state, cookie_session("memory-session"), false)
                .await;
            assert!(status.connected);
            assert!(!status.remembered);
            assert_eq!(store.counts(), (0, 2, 1));
            assert!(store.value.lock().unwrap().is_none());
            state.begin_login();
            runtime
                .finish_login(&mut state, cookie_session("another-memory-session"), false)
                .await;
            runtime.restore(&mut state).await.unwrap();
            assert_eq!(store.counts(), (0, 2, 1));
            drop(state);
            runtime.logout().await.unwrap();
            assert_eq!(store.counts(), (0, 2, 2));
        });
    }

    #[test]
    fn memory_login_cleans_unknown_or_restored_credentials_but_skips_known_empty_store() {
        tauri::async_runtime::block_on(async {
            for restore in [false, true] {
                let store = Arc::new(FakeCredentialStore::default());
                persist(&mut cookie_session("old-session"), store.clone()).await;
                let runtime = Runtime {
                    state: Mutex::new(RuntimeState::default()),
                    credentials: store.clone(),
                };
                let mut state = runtime.state.lock().await;
                if restore {
                    runtime.restore(&mut state).await.unwrap();
                }
                state.begin_login();
                runtime
                    .finish_login(&mut state, cookie_session("new-session"), false)
                    .await;
                assert_eq!(store.counts(), (usize::from(restore), 1, 1));
                assert!(store.value.lock().unwrap().is_none());
            }
            let store = Arc::new(FakeCredentialStore::default());
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store.clone(),
            };
            let mut state = runtime.state.lock().await;
            runtime.restore(&mut state).await.unwrap();
            state.begin_login();
            runtime
                .finish_login(&mut state, cookie_session("memory-session"), false)
                .await;
            assert_eq!(store.counts(), (1, 0, 0));
        });
    }

    #[test]
    fn failed_login_does_not_touch_credentials_or_restore_previous_session() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore::default());
            persist(&mut cookie_session("old-session"), store.clone()).await;
            let stored = store.value.lock().unwrap().clone();
            let runtime = Runtime {
                state: Mutex::new(RuntimeState {
                    session: Some(cookie_session("old-memory-session")),
                    ..Default::default()
                }),
                credentials: store.clone(),
            };
            // 输入校验即失败，不发出 HTTP；验证实际登录入口先隔离旧会话。
            assert!(runtime
                .login(
                    "http://example.invalid".into(),
                    "new-user".into(),
                    "synthetic-password".into(),
                    true
                )
                .await
                .is_err());
            let status = runtime.status().await.unwrap();
            assert!(!status.connected);
            assert!(status.username.is_none());
            assert_eq!(store.counts(), (0, 1, 0));
            assert_eq!(*store.value.lock().unwrap(), stored);
        });
    }

    #[test]
    fn memory_login_clear_failure_warns_without_query_retry() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore {
                fail: true,
                ..Default::default()
            });
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store.clone(),
            };
            let mut state = runtime.state.lock().await;
            state.begin_login();
            let status = runtime
                .finish_login(&mut state, cookie_session("memory-session"), false)
                .await;
            assert!(status.connected);
            assert!(!status.remembered);
            assert!(status
                .message
                .as_deref()
                .unwrap()
                .starts_with("Credential:"));
            for _ in 0..3 {
                runtime.restore(&mut state).await.unwrap();
            }
            assert_eq!(store.counts(), (0, 0, 1));
            assert_eq!(
                state.session.as_ref().unwrap().cookie().as_deref(),
                Some("memory-session")
            );
        });
    }

    #[test]
    fn persists_restores_and_clears_with_fake_store_only() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore::default());
            let mut session = new_session(
                validate_base("https://example.invalid").unwrap(),
                "example-user".into(),
            )
            .unwrap();
            session.jar.add_cookie_str(
                "PHPSESSID=synthetic-session; Path=/; Secure; HttpOnly",
                &session.base,
            );
            persist(&mut session, store.clone()).await;
            assert!(session.remembered);
            let stored = store.value.lock().unwrap().clone().unwrap();
            assert!(!stored.contains("password"));
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store.clone(),
            };
            {
                let mut state = runtime.state.lock().await;
                runtime.restore(&mut state).await.unwrap();
                assert_eq!(
                    state.session.as_ref().unwrap().cookie().as_deref(),
                    Some("synthetic-session")
                );
                runtime.expire(&mut state, SESSION_EXPIRED).await;
                assert!(state.session.is_none());
            }
            assert!(store.value.lock().unwrap().is_none());
            let status = runtime.logout().await.unwrap();
            assert!(!status.connected);
            assert!(!status.remembered);
        });
    }

    #[test]
    fn persistence_failure_keeps_memory_session_and_warns() {
        tauri::async_runtime::block_on(async {
            let store = Arc::new(FakeCredentialStore {
                fail: true,
                ..Default::default()
            });
            let mut session = new_session(
                validate_base("https://example.invalid").unwrap(),
                "example-user".into(),
            )
            .unwrap();
            session.jar.add_cookie_str(
                "PHPSESSID=synthetic-session; Path=/; Secure; HttpOnly",
                &session.base,
            );
            persist(&mut session, store.clone()).await;
            assert!(!session.remembered);
            assert!(session.message.as_deref().unwrap().contains("仅内存"));
            assert!(session.cookie().is_some());
            let runtime = Runtime {
                state: Mutex::new(RuntimeState::default()),
                credentials: store,
            };
            let status = runtime.status().await.unwrap();
            assert!(!status.connected);
            assert!(status.message.is_some());
            let status = runtime.logout().await.unwrap();
            assert!(status.message.as_deref().unwrap().contains("无法删除"));
        });
    }

    #[test]
    fn rejects_conflicting_module_mapping_and_preserves_full_branch() {
        let repos = parse_repositories(REPOS).unwrap();
        assert!(parse_modules(&MODULES.replace("g_pid=7", "g_pid=8"), &repos).is_err());
        assert_eq!(
            parse_modules(
                &MODULES.replace("feature/example-12", "release/example/deep-token_12.3"),
                &repos
            )
            .unwrap()[0]
                .branch,
            "release/example/deep-token_12.3"
        );
        assert!(parse_modules("<div>unexpected</div>", &repos).is_err());
    }

    #[test]
    fn status_does_not_export_credentials() {
        let status = Status {
            connected: true,
            base_url: Some("https://example.invalid/".into()),
            username: Some("example-user".into()),
            remembered: false,
            message: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("cookie"));
        assert!(!json.contains("password"));
        assert!(json.contains("baseUrl"));
    }
}
