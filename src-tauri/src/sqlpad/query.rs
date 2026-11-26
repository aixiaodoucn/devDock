use super::*;
use serde_json::json;

const BAD: &str = "DB_INVALID";
const STALE: &str = "DB_STALE";

pub(crate) struct Prepared {
    pub(crate) connection: String,
    pub(crate) preview: String,
    pub(crate) auto_approve: bool,
    generation: uuid::Uuid,
    pub(super) replay: history::Replay,
    fields: Vec<String>,
    limit: Option<u64>,
}

fn ident(s: &str) -> bool {
    (1..=63).contains(&s.len())
        && s.as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
// 普通列、分组列与聚合别名都走同一标识符校验，不接收调用方传入的 SQL 表达式。
fn identifier_list(value: &Value) -> Result<Vec<String>> {
    let list = value
        .as_array()
        .filter(|a| (1..=10).contains(&a.len()))
        .ok_or(BAD)?;
    let mut seen = HashSet::new();
    list.iter()
        .map(|v| {
            let name = v.as_str().filter(|s| ident(s)).ok_or(BAD)?;
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err(BAD.into());
            }
            Ok(name.to_owned())
        })
        .collect()
}

struct Projection {
    expressions: Vec<String>,
    fields: Vec<String>,
    grouping: String,
}

fn projection(args: &Value, quote: impl Fn(&str) -> String) -> Result<Projection> {
    let group = args.get("groupBy").map(identifier_list).transpose()?;
    let columns = args.get("columns").map(identifier_list).transpose()?;
    let aggregates = args
        .get("aggregates")
        .map(|v| {
            v.as_array()
                .filter(|a| (1..=10).contains(&a.len()))
                .ok_or(BAD)
        })
        .transpose()?;
    let grouped = group.is_some() || aggregates.is_some();
    let group = group.unwrap_or_default();
    if let Some(cols) = &columns {
        if grouped && (cols.len() != group.len() || cols.iter().any(|col| !group.contains(col))) {
            return Err(BAD.into());
        }
    }
    let grouping = if group.is_empty() {
        String::new()
    } else {
        format!(
            " GROUP BY {}",
            group
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut fields = columns.unwrap_or(group);
    if !(1..=10).contains(&(fields.len() + aggregates.map_or(0, |a| a.len()))) {
        return Err(BAD.into());
    }
    let mut expressions: Vec<_> = fields.iter().map(|s| quote(s)).collect();
    for agg in aggregates.into_iter().flatten() {
        let obj = agg.as_object().ok_or(BAD)?;
        if obj
            .keys()
            .any(|k| !matches!(k.as_str(), "fn" | "alias" | "column"))
        {
            return Err(BAD.into());
        }
        let function = match agg["fn"].as_str() {
            Some("count") => "COUNT",
            Some("sum") => "SUM",
            Some("avg") => "AVG",
            Some("min") => "MIN",
            Some("max") => "MAX",
            _ => return Err(BAD.into()),
        };
        let column = match obj.get("column") {
            Some(v) => quote(v.as_str().filter(|s| ident(s)).ok_or(BAD)?),
            None if function == "COUNT" => "*".to_owned(),
            None => return Err(BAD.into()),
        };
        let alias = agg["alias"].as_str().filter(|s| ident(s)).ok_or(BAD)?;
        if fields.iter().any(|s| s.eq_ignore_ascii_case(alias)) {
            return Err(BAD.into());
        }
        expressions.push(format!("{function}({column}) AS {}", quote(alias)));
        fields.push(alias.to_owned());
    }
    Ok(Projection {
        expressions,
        fields,
        grouping,
    })
}

fn compile(driver: &str, tool: &str, args: &Value) -> Result<(String, Vec<String>, Option<u64>)> {
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
        return Ok((format!("SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = {namespace} AND table_name = '{table}' ORDER BY ordinal_position"),vec!["column_name".into(),"data_type".into()],None));
    }
    if tool != "prod_db_query_readonly"
        || map.keys().any(|k| {
            !matches!(
                k.as_str(),
                "table"
                    | "schema"
                    | "columns"
                    | "limit"
                    | "filter"
                    | "filters"
                    | "aggregates"
                    | "groupBy"
            )
        })
    {
        return Err(BAD.into());
    }
    let Projection {
        expressions,
        fields,
        grouping,
    } = projection(args, q)?;
    let limit = map
        .get("limit")
        .map(|v| v.as_u64().filter(|i| *i > 0).ok_or(BAD))
        .transpose()?;
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
    let cols = expressions.join(", ");
    let suffix = limit.map(|n| format!(" LIMIT {n}")).unwrap_or_default();
    Ok((
        format!("SELECT {cols} FROM {from}{predicate}{grouping}{suffix}"),
        fields,
        limit,
    ))
}

pub(crate) fn valid_query_args(tool: &str, args: &Value) -> bool {
    compile("postgres", tool, args).is_ok()
}

fn bounded_result(body: &Value, fields: &[String], limit: Option<u64>) -> Result<Value> {
    let result = body.get("queryResult").ok_or("DB_FORMAT")?;
    let rows = result["rows"].as_array().ok_or("DB_FORMAT")?;
    if result["incomplete"] == true {
        return Err("DB_INCOMPLETE".into());
    }
    if limit.is_some_and(|n| rows.len() as u64 > n) {
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
    let projected = json!({"columns":fields,"rows":output,"rowCount":rows.len(),"limit":limit,"limitReached":Some(rows.len() as u64)==limit,"incomplete":result["incomplete"].as_bool()});
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
        self.session
            .lock()
            .await
            .as_ref()
            .is_some_and(|s| s.status.selected_id.is_some())
    }
    pub(crate) async fn prepare(&self, tool: &str, args: &Value) -> Result<Prepared> {
        let slot = self.session.lock().await;
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
            auto_approve: s.status.auto_approve,
            generation: s.generation,
            replay: history::Replay {
                origin: s.root.origin().ascii_serialization(),
                connection_id: connection.id.clone(),
                driver: connection.driver.clone(),
                args: args.clone(),
            },
            fields,
            limit,
        })
    }
    pub(super) async fn prepare_rerun(
        &self,
        tool: &str,
        replay: &history::Replay,
        sql: &str,
    ) -> Result<Prepared> {
        let mut prepared = self
            .prepare(tool, &replay.args)
            .await
            .map_err(|e| crate::log_mcp::sqlpad_error(&e))?;
        // 历史重跑必须保持当时的 SQL，不把旧记录的默认 LIMIT 扩大为全量查询。
        if prepared.limit.is_none() && prepared.preview != sql {
            let old_limit = if tool == "prod_db_describe_table" {
                50
            } else {
                20
            };
            let legacy_sql = format!("{} LIMIT {old_limit}", prepared.preview);
            if legacy_sql == sql {
                prepared.preview = legacy_sql;
                prepared.limit = Some(old_limit);
            }
        }
        if &prepared.replay != replay || prepared.preview != sql {
            return Err("原连接或查询已变更，请确认已选择原来的 SQLPad 连接".into());
        }
        Ok(prepared)
    }

    pub(crate) async fn execute(&self, p: Prepared) -> Result<Value> {
        // 授权绑定当前会话与所选连接。请求期间不允许换账号；不重新生成 SQL，不重试。
        let mut slot = self.session.lock().await;
        let s = slot.as_mut().ok_or(AUTH_REQUIRED)?;
        if s.generation != p.generation
            || s.status.selected_id.as_ref() != Some(&p.replay.connection_id)
            || !s
                .status
                .connections
                .iter()
                .any(|c| c.id == p.replay.connection_id && c.driver == p.replay.driver)
        {
            return Err(STALE.into());
        }
        let body = json!({"connectionId":p.replay.connection_id,"queryText":p.preview,"cacheKey":uuid::Uuid::new_v4().to_string(),"queryName":"Dev Dock read-only"});
        let response = s
            .client
            .post(s.root.join("api/query-result").map_err(|_| "DB_INVALID")?)
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&body).map_err(|_| "DB_INVALID")?)
            .send()
            .await
            .map_err(|_| "DB_UNKNOWN")?;
        match json_response(response).await {
            Ok(data) => {
                if s.status.remembered {
                    self.persist(s).await;
                }
                bounded_result(&data, &p.fields, p.limit)
            }
            Err(e) if e == AUTH_REQUIRED => {
                *slot = None;
                let _ = self.forget().await;
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
            assert_eq!(sent["queryText"], "SELECT `label` FROM `demo`");
            let response = r#"{"queryResult":{"rows":[{"label":"sample","extra":"private"}],"cacheKey":"unused"}}"#;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        });
        tauri::async_runtime::block_on(async {
            let db = Runtime {
                session: Mutex::new(Some(Session {
                    jar: Default::default(),
                    saved: None,
                    pending_connection: None,
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
                })),
                ..Default::default()
            };
            let args = json!({"table":"demo","columns":["label"]});
            let tool = "prod_db_query_readonly";
            let stale = db.prepare(tool, &args).await.unwrap();
            assert!(!stale.auto_approve);
            let original = stale.replay.clone();
            let sql = stale.preview.clone();
            let legacy = db
                .prepare_rerun(tool, &original, &format!("{sql} LIMIT 20"))
                .await
                .unwrap();
            assert_eq!(legacy.limit, Some(20));
            assert!(db
                .prepare_rerun(tool, &original, &format!("{sql} LIMIT 19"))
                .await
                .is_err());
            let mut other = original.clone();
            other.connection_id = "other".into();
            assert!(db.prepare_rerun(tool, &other, &sql).await.is_err());
            assert!(db
                .prepare_rerun(tool, &original, "SELECT other")
                .await
                .is_err());
            {
                let mut slot = db.session.lock().await;
                let session = slot.as_mut().unwrap();
                assert!(session.set_auto_approve("different", true).is_err());
                session.set_auto_approve("fixture", true).unwrap();
            }
            let granted = db.prepare(tool, &args).await.unwrap();
            assert!(granted.auto_approve);
            db.session
                .lock()
                .await
                .as_mut()
                .unwrap()
                .select(Some("fixture".into()));
            assert!(!db.prepare(tool, &args).await.unwrap().auto_approve);
            assert_eq!(db.execute(granted).await.unwrap_err(), STALE);
            assert_eq!(db.execute(stale).await.unwrap_err(), STALE);
            let result = db
                .execute(db.prepare_rerun(tool, &original, &sql).await.unwrap())
                .await
                .unwrap();
            assert_eq!(result["rows"], json!([{"label":"sample"}]));
            assert_eq!(result["rowCount"], 1);
        });
        service.join().unwrap();
    }
    #[test]
    fn unlimited_queries_keep_all_rows_and_reject_upstream_truncation() {
        let tool = "prod_db_query_readonly";
        let columns: Vec<_> = (0..3).map(|i| format!("col{i}")).collect();
        let mut args = json!({"table":"demo", "columns":columns});
        assert!(!compile("postgres", tool, &args)
            .unwrap()
            .0
            .contains(" LIMIT "));
        args["limit"] = json!(150);
        assert!(compile("mysql", tool, &args)
            .unwrap()
            .0
            .ends_with(" LIMIT 150"));
        assert!(
            !compile("mysql", "prod_db_describe_table", &json!({"table":"demo"}))
                .unwrap()
                .0
                .contains(" LIMIT ")
        );
        let rows: Vec<_> = (0..125).map(|id| json!({"id":id})).collect();
        let mut body = json!({"queryResult":{"rows":rows, "incomplete":false}});
        let fields = [String::from("id")];
        let limit = compile("mysql", tool, &json!({"table":"demo", "columns":fields}))
            .unwrap()
            .2;
        let result = bounded_result(&body, &fields, limit).unwrap();
        assert_eq!(result["rows"], body["queryResult"]["rows"]);
        assert_eq!(result["limit"], Value::Null);
        assert_eq!(result["limitReached"], false);
        body["queryResult"]["incomplete"] = json!(true);
        assert_eq!(
            bounded_result(&body, &fields, limit).unwrap_err(),
            "DB_INCOMPLETE"
        );
    }

    #[test]
    fn grouped_counts_are_computed_before_limit_and_return_named_fields() {
        let tool = "prod_db_query_readonly";
        let args = json!({"table":"orders","groupBy":["status"],"aggregates":[{"fn":"count","alias":"n"},{"fn":"sum","column":"amount","alias":"total"}],"filter":{"column":"region","value":"east"},"limit":5});
        let (sql, fields, limit) = compile("mysql", tool, &args).unwrap();
        assert_eq!(sql,"SELECT `status`, COUNT(*) AS `n`, SUM(`amount`) AS `total` FROM `orders` WHERE `region` = 'east' GROUP BY `status` LIMIT 5");
        let result = bounded_result(&json!({"queryResult":{"rows":[{"status":"paid","n":12,"total":90,"other":"private"}]}}), &fields, limit).unwrap();
        assert_eq!(
            result["rows"][0],
            json!({"status":"paid","n":12,"total":90})
        );
        assert_eq!(
            compile(
                "postgres",
                tool,
                &json!({"table":"orders","aggregates":[{"fn":"count","alias":"n"}]})
            )
            .unwrap()
            .0,
            "SELECT COUNT(*) AS \"n\" FROM \"orders\""
        );
        for invalid in [
            json!({"groupBy":["id"],"columns":["name"]}),
            json!({"groupBy":["id","id"]}),
            json!({"groupBy":["id"],"aggregates":[{"fn":"count","alias":"ID"}]}),
            json!({"aggregates":[{"fn":"sum","alias":"n"}]}),
            json!({"aggregates":[{"fn":"sleep","column":"id","alias":"n"}]}),
            json!({"aggregates":[{"fn":"count","alias":"n;"}]}),
            json!({"groupBy":[]}),
            json!({"aggregates":[]}),
        ] {
            let mut bad = invalid;
            bad["table"] = json!("orders");
            assert!(compile("mysql", tool, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn combines_filters_and_validates_each_comparison() {
        let tool = "prod_db_query_readonly";
        let mut args = json!({"table":"orders","columns":["id"],"filters":[
            {"column":"status","value":"pending"},
            {"column":"created_at","op":"gte","value":"2026-11-01"}
        ]});
        assert_eq!(
            compile("mysql", tool, &args).unwrap().0,
            "SELECT `id` FROM `orders` WHERE `status` = 'pending' AND `created_at` >= '2026-11-01'"
        );
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
        assert_eq!(limit, Some(2));
        for (key, val) in [
            ("table", json!("demo; DELETE")),
            ("sql", json!("SELECT 1")),
            ("limit", json!(0)),
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
