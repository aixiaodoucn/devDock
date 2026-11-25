use super::*;
use serde_json::json;

const BAD: &str = "DB_INVALID";
const STALE: &str = "DB_STALE";

pub(crate) struct Prepared {
    pub(crate) connection: String,
    pub(crate) preview: String,
    generation: uuid::Uuid,
    connection_id: String,
    driver: String,
    fields: Vec<String>,
    limit: usize,
}

fn ident(s: &str) -> bool {
    (1..=63).contains(&s.len())
        && s.as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn compile(driver: &str, tool: &str, args: &Value) -> Result<(String, Vec<String>, usize)> {
    let quote = match driver {
        "mysql" | "mysql2" => '`',
        "postgres" | "postgresql" => '"',
        _ => return Err("DB_DRIVER".into()),
    };
    let q = |s: &str| format!("{quote}{s}{quote}");
    let map = args.as_object().ok_or(BAD)?;
    let table = args["table"].as_str().filter(|s| ident(s)).ok_or(BAD)?;
    let schema = map
        .get("schema")
        .map(|v| v.as_str().filter(|s| ident(s)).ok_or(BAD))
        .transpose()?;
    if tool == "prod_db_describe_table" {
        if map
            .keys()
            .any(|k| !matches!(k.as_str(), "table" | "schema"))
        {
            return Err(BAD.into());
        }
        // 只返回指定表的列名和类型；不枚举其他表、默认值或注释。
        let namespace = schema.map(|s| format!("'{s}'")).unwrap_or_else(|| {
            if quote == '`' {
                "DATABASE()".into()
            } else {
                "current_schema()".into()
            }
        });
        return Ok((format!("SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = {namespace} AND table_name = '{table}' ORDER BY ordinal_position LIMIT 50"),vec!["column_name".into(),"data_type".into()],50));
    }
    if tool != "prod_db_query_readonly"
        || map.keys().any(|k| {
            !matches!(
                k.as_str(),
                "table" | "schema" | "columns" | "limit" | "filter" | "filters"
            )
        })
    {
        return Err(BAD.into());
    }
    let columns = args["columns"]
        .as_array()
        .filter(|a| (1..=10).contains(&a.len()))
        .ok_or(BAD)?;
    let fields: Vec<String> = columns
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| ident(s))
                .map(str::to_owned)
                .ok_or(BAD)
        })
        .collect::<std::result::Result<_, _>>()?;
    if fields
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<HashSet<_>>()
        .len()
        != fields.len()
    {
        return Err(BAD.into());
    }
    let limit = map
        .get("limit")
        .map(|v| v.as_u64().filter(|i| (1..=50).contains(i)).ok_or(BAD))
        .transpose()?
        .unwrap_or(20) as usize;
    let from = schema
        .map(|s| format!("{}.{}", q(s), q(table)))
        .unwrap_or_else(|| q(table));
    // 旧入口仅接受等值条件，新入口按输入顺序组合，审批预览与执行共用同一段 SQL。
    let filters = match (map.get("filter"), map.get("filters")) {
        (Some(single), None) => std::slice::from_ref(single),
        (None, Some(list)) => list
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or(BAD)?
            .as_slice(),
        (None, None) => &[],
        (Some(_), Some(_)) => return Err(BAD.into()),
    };
    let mut predicates = Vec::with_capacity(filters.len());
    for filter in filters {
        let f = filter.as_object().ok_or(BAD)?;
        if f.keys()
            .any(|key| !matches!(key.as_str(), "column" | "value" | "op"))
        {
            return Err(BAD.into());
        }
        let operator = match f.get("op") {
            None => "=",
            Some(op) if map.contains_key("filters") => match op.as_str() {
                Some("eq") => "=",
                Some("ne") => "<>",
                Some("gt") => ">",
                Some("gte") => ">=",
                Some("lt") => "<",
                Some("lte") => "<=",
                _ => return Err(BAD.into()),
            },
            _ => return Err(BAD.into()),
        };
        let col = f
            .get("column")
            .and_then(Value::as_str)
            .filter(|s| ident(s))
            .ok_or(BAD)?;
        let value = f
            .get("value")
            .and_then(Value::as_str)
            .filter(|s| {
                !s.trim().is_empty()
                    && s.len() <= 128
                    && s.chars()
                        .all(|c| c.is_alphanumeric() || " _-. :".contains(c))
            })
            .ok_or(BAD)?;
        predicates.push(format!("{} {operator} '{value}'", q(col)));
    }
    let predicate = if predicates.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", predicates.join(" AND "))
    };
    let cols = fields.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ");
    Ok((
        format!("SELECT {cols} FROM {from}{predicate} LIMIT {limit}"),
        fields,
        limit,
    ))
}

pub(crate) fn valid_query_args(tool: &str, args: &Value) -> bool {
    compile("postgres", tool, args).is_ok()
}

fn bounded_result(body: &Value, fields: &[String], limit: usize) -> Result<Value> {
    let result = body.get("queryResult").ok_or("DB_FORMAT")?;
    let rows = result["rows"].as_array().ok_or("DB_FORMAT")?;
    if rows.len() > limit {
        return Err("DB_RESPONSE_LIMIT".into());
    }
    let mut output = Vec::new();
    for row in rows {
        let mut item = serde_json::Map::new();
        for key in fields {
            let value = row.get(key).ok_or("DB_FORMAT")?;
            if value.is_object()
                || value.is_array()
                || value.as_str().is_some_and(|v| v.len() > 4096)
            {
                return Err("DB_RESPONSE_LIMIT".into());
            }
            item.insert(key.clone(), value.clone());
        }
        output.push(item);
    }
    let projected = json!({"columns":fields,"rows":output,"rowCount":rows.len(),"limit":limit,"limitReached":rows.len()==limit,"incomplete":result["incomplete"].as_bool()});
    if serde_json::to_vec(&projected)
        .map_err(|_| "DB_FORMAT")?
        .len()
        > 48 * 1024
    {
        return Err("DB_RESPONSE_LIMIT".into());
    }
    Ok(projected)
}

impl Runtime {
    pub(crate) async fn ready(&self) -> bool {
        self.0
            .lock()
            .await
            .as_ref()
            .is_some_and(|s| s.status.selected_id.is_some())
    }
    pub(crate) async fn prepare(&self, tool: &str, args: &Value) -> Result<Prepared> {
        let slot = self.0.lock().await;
        let s = slot.as_ref().ok_or(AUTH_REQUIRED)?;
        let connection = s
            .status
            .connections
            .iter()
            .find(|c| Some(&c.id) == s.status.selected_id.as_ref())
            .ok_or("DB_NO_CONNECTION")?;
        let (preview, fields, limit) = compile(&connection.driver, tool, args)?;
        Ok(Prepared {
            connection: format!(
                "{} · {}",
                s.root.origin().ascii_serialization(),
                connection.name
            ),
            preview,
            generation: s.generation,
            connection_id: connection.id.clone(),
            driver: connection.driver.clone(),
            fields,
            limit,
        })
    }
    pub(crate) async fn execute(&self, p: Prepared) -> Result<Value> {
        // 授权绑定当前会话与所选连接。请求期间不允许换账号；不重新生成 SQL，不重试。
        let mut slot = self.0.lock().await;
        let s = slot.as_ref().ok_or(AUTH_REQUIRED)?;
        if s.generation != p.generation
            || s.status.selected_id.as_ref() != Some(&p.connection_id)
            || !s
                .status
                .connections
                .iter()
                .any(|c| c.id == p.connection_id && c.driver == p.driver)
        {
            return Err(STALE.into());
        }
        let body = json!({"connectionId":p.connection_id,"queryText":p.preview,"cacheKey":uuid::Uuid::new_v4().to_string(),"queryName":"Dev Dock read-only"});
        let response = s
            .client
            .post(s.root.join("api/query-result").map_err(|_| "DB_INVALID")?)
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&body).map_err(|_| "DB_INVALID")?)
            .send()
            .await
            .map_err(|_| "DB_UNKNOWN")?;
        match json_response(response).await {
            Ok(data) => bounded_result(&data, &p.fields, p.limit),
            Err(e) if e == AUTH_REQUIRED => {
                *slot = None;
                Err(e)
            }
            Err(e) if e == "SQLPad 响应过大" => Err("DB_RESPONSE_LIMIT".into()),
            Err(_) => Err("DB_UNKNOWN".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_returns_only_approved_columns_and_rejects_changed_selection() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let service = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert_eq!(first.trim_end(), "POST /api/query-result HTTP/1.1");
            let mut size = 0;
            loop {
                let mut header = String::new();
                assert!(reader.read_line(&mut header).unwrap() > 0);
                if header == "\r\n" {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = v.trim().parse().unwrap();
                }
            }
            let mut data = vec![0; size];
            reader.read_exact(&mut data).unwrap();
            let sent: Value = serde_json::from_slice(&data).unwrap();
            assert_eq!(sent["connectionId"], "fixture");
            assert_eq!(sent["queryText"], "SELECT `label` FROM `demo` LIMIT 20");
            let response = r#"{"queryResult":{"rows":[{"label":"sample","extra":"private"}],"cacheKey":"unused"}}"#;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        });
        tauri::async_runtime::block_on(async {
            let db = Runtime(Mutex::new(Some(Session {
                client: Client::builder().no_proxy().build().unwrap(),
                generation: uuid::Uuid::new_v4(),
                root,
                status: Status {
                    connected: true,
                    selected_id: Some("fixture".into()),
                    connections: vec![Connection {
                        id: "fixture".into(),
                        name: "example".into(),
                        driver: "mysql".into(),
                    }],
                    ..Status::default()
                },
            })));
            let args = json!({"table":"demo","columns":["label"]});
            let tool = "prod_db_query_readonly";
            let stale = db.prepare(tool, &args).await.unwrap();
            db.0.lock().await.as_mut().unwrap().generation = uuid::Uuid::new_v4();
            assert_eq!(db.execute(stale).await.unwrap_err(), STALE);
            let result = db
                .execute(db.prepare(tool, &args).await.unwrap())
                .await
                .unwrap();
            assert_eq!(result["rows"], json!([{"label":"sample"}]));
            assert_eq!(result["rowCount"], 1);
        });
        service.join().unwrap();
    }
    #[test]
    fn combines_filters_and_validates_each_comparison() {
        let tool = "prod_db_query_readonly";
        let mut args = json!({"table":"orders","columns":["id"],"filters":[
            {"column":"status","value":"pending"},
            {"column":"created_at","op":"gte","value":"2026-11-01"}
        ]});
        assert_eq!(compile("mysql", tool, &args).unwrap().0,
            "SELECT `id` FROM `orders` WHERE `status` = 'pending' AND `created_at` >= '2026-11-01' LIMIT 20");
        for invalid in [
            json!([]),
            json!([{"column":"id","op":"unknown","value":"1"}]),
            json!([{"column":"id","value":"1"},{"column":"id","value":"'"}]),
        ] {
            args["filters"] = invalid;
            assert!(!valid_query_args(tool, &args));
        }
        args["filters"] = json!([{"column":"id","value":"1"}]);
        args["filter"] = json!({"column":"id","value":"2"});
        assert!(!valid_query_args(tool, &args));
    }

    #[test]
    fn preview_is_bounded_and_never_accepts_sql_fragments() {
        let args = json!({"table":"demo","columns":["title"],"filter":{"column":"id","value":"123"},"limit":2});
        let (sql, _, limit) = compile("postgres", "prod_db_query_readonly", &args).unwrap();
        assert_eq!(
            sql,
            "SELECT \"title\" FROM \"demo\" WHERE \"id\" = '123' LIMIT 2"
        );
        assert_eq!(limit, 2);
        for (key, val) in [
            ("table", json!("demo; DELETE")),
            ("sql", json!("SELECT 1")),
            ("limit", json!(51)),
            ("columns", json!(["*"])),
            ("filter", json!({"column":"id","value":"a'b"})),
        ] {
            let mut bad = args.clone();
            bad[key] = val;
            assert!(compile("mysql", "prod_db_query_readonly", &bad).is_err());
        }
        assert!(compile("oracle", "prod_db_query_readonly", &args).is_err());
        assert!(
            compile("mysql", "prod_db_describe_table", &json!({"table":"demo"}))
                .unwrap()
                .0
                .contains("table_schema = DATABASE()")
        );
    }
}
