//! Persist aggregated usage to a local SQLite file.

use crate::state::{ModelUsage, ProviderUsage, UsageData};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

pub fn init_schema(path: &Path) -> Result<(), String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS provider_usage (
            provider TEXT PRIMARY KEY,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            requests INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS model_usage (
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            requests INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (provider, model)
        );
        ",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_usage(path: &Path) -> Result<UsageData, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let mut by_provider: HashMap<String, ProviderUsage> = HashMap::new();

    let mut stmt = conn
        .prepare("SELECT provider, input_tokens, output_tokens, requests FROM provider_usage")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
            ))
        })
        .map_err(|e| e.to_string())?;

    for r in rows {
        let (provider, it, ot, rq) = r.map_err(|e| e.to_string())?;
        by_provider.insert(
            provider,
            ProviderUsage {
                input_tokens: it,
                output_tokens: ot,
                requests: rq,
                per_model: HashMap::new(),
            },
        );
    }

    let mut stmt = conn
        .prepare(
            "SELECT provider, model, input_tokens, output_tokens, requests FROM model_usage",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
                row.get::<_, i64>(4)? as u64,
            ))
        })
        .map_err(|e| e.to_string())?;

    for r in rows {
        let (prov, model, it, ot, rq) = r.map_err(|e| e.to_string())?;
        let pu = by_provider.entry(prov).or_default();
        pu.per_model.insert(
            model,
            ModelUsage {
                input_tokens: it,
                output_tokens: ot,
                requests: rq,
            },
        );
    }

    Ok(UsageData { by_provider })
}

pub fn save_usage(path: &Path, data: &UsageData) -> Result<(), String> {
    let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM model_usage", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM provider_usage", [])
        .map_err(|e| e.to_string())?;

    for (provider, pu) in &data.by_provider {
        tx.execute(
            "INSERT INTO provider_usage (provider, input_tokens, output_tokens, requests) VALUES (?1, ?2, ?3, ?4)",
            params![
                provider,
                pu.input_tokens as i64,
                pu.output_tokens as i64,
                pu.requests as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
        for (model, m) in &pu.per_model {
            tx.execute(
                "INSERT INTO model_usage (provider, model, input_tokens, output_tokens, requests) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    provider,
                    model,
                    m.input_tokens as i64,
                    m.output_tokens as i64,
                    m.requests as i64,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
