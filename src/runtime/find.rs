use radroots_sql_core::{SqlExecutor, SqliteExecutor, utils};
use serde::Deserialize;
use serde_json::Value;

use crate::cli::FindArgs;
use crate::domain::runtime::{
    FindPriceView, FindQuantityView, FindResultProvenanceView, FindResultView, FindView,
    SyncFreshnessView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sync::freshness_from_executor;

const FIND_SOURCE: &str = "local replica · local first";

#[derive(Debug, Clone, Deserialize)]
struct FindRow {
    id: String,
    key: String,
    category: String,
    title: String,
    summary: String,
    qty_amt: i64,
    qty_unit: String,
    qty_label: Option<String>,
    qty_avail: Option<i64>,
    price_amt: f64,
    price_currency: String,
    price_qty_amt: u32,
    price_qty_unit: String,
    location_primary: Option<String>,
}

pub fn search(config: &RuntimeConfig, args: &FindArgs) -> Result<FindView, RuntimeError> {
    let query = args.query.join(" ");
    if !config.local.replica_db_path.exists() {
        return Ok(FindView {
            state: "unconfigured".to_owned(),
            source: FIND_SOURCE.to_owned(),
            query,
            count: 0,
            relay_count: config.relay.urls.len(),
            replica_db: config.local.replica_db_path.display().to_string(),
            freshness: SyncFreshnessView {
                state: "never".to_owned(),
                display: "never synced".to_owned(),
                age_seconds: None,
                last_event_at: None,
            },
            results: Vec::new(),
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots local init".to_owned()],
        });
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let freshness = freshness_from_executor(&executor)?;
    let rows = query_rows(&executor, &args.query)?;
    let relay_count = config.relay.urls.len();
    let result_provenance = FindResultProvenanceView {
        origin: "local_replica.trade_product".to_owned(),
        freshness: freshness.display.clone(),
        relay_count,
    };
    let results = rows
        .into_iter()
        .map(|row| FindResultView {
            id: row.id,
            product_key: row.key,
            title: row.title,
            category: row.category,
            summary: non_empty(row.summary),
            location_primary: row.location_primary.and_then(non_empty),
            available: FindQuantityView {
                total_amount: row.qty_amt,
                total_unit: row.qty_unit,
                label: row.qty_label.and_then(non_empty),
                available_amount: row.qty_avail,
            },
            price: FindPriceView {
                amount: row.price_amt,
                currency: row.price_currency,
                per_amount: row.price_qty_amt,
                per_unit: row.price_qty_unit,
            },
            provenance: result_provenance.clone(),
        })
        .collect::<Vec<_>>();

    let (state, reason, actions) = if results.is_empty() {
        let actions = if freshness.state == "never" {
            vec!["radroots sync status".to_owned()]
        } else {
            Vec::new()
        };
        (
            "empty".to_owned(),
            Some(format!("no local market results matched `{query}`")),
            actions,
        )
    } else {
        ("ready".to_owned(), None, Vec::new())
    };

    Ok(FindView {
        state,
        source: FIND_SOURCE.to_owned(),
        query,
        count: results.len(),
        relay_count,
        replica_db: config.local.replica_db_path.display().to_string(),
        freshness,
        results,
        reason,
        actions,
    })
}

fn query_rows(
    executor: &SqliteExecutor,
    query_terms: &[String],
) -> Result<Vec<FindRow>, RuntimeError> {
    let mut where_clauses = Vec::with_capacity(query_terms.len());
    let mut bind_values = Vec::<Value>::with_capacity(query_terms.len() * 5);

    for term in query_terms {
        let pattern = format!("%{}%", term.to_lowercase());
        where_clauses.push(
            "(lower(tp.title) LIKE ? OR lower(tp.summary) LIKE ? OR lower(tp.category) LIKE ? OR lower(tp.key) LIKE ? OR lower(COALESCE(tp.notes, '')) LIKE ?)"
                .to_owned(),
        );
        for _ in 0..5 {
            bind_values.push(Value::from(pattern.clone()));
        }
    }

    let sql = format!(
        "SELECT tp.id, tp.key, tp.category, tp.title, tp.summary, tp.qty_amt, tp.qty_unit, tp.qty_label, tp.qty_avail, tp.price_amt, tp.price_currency, tp.price_qty_amt, tp.price_qty_unit, loc.location_primary \
         FROM trade_product tp \
         LEFT JOIN (\
             SELECT tpl.tb_tp AS trade_product_id, MIN(COALESCE(gl.label, gl.gc_name, gl.gc_admin1_name, gl.gc_country_name, gl.d_tag)) AS location_primary \
             FROM trade_product_location tpl \
             JOIN gcs_location gl ON gl.id = tpl.tb_gl \
             GROUP BY tpl.tb_tp\
         ) loc ON loc.trade_product_id = tp.id \
         WHERE {} \
         ORDER BY lower(tp.title) ASC, tp.id ASC;",
        where_clauses.join(" AND ")
    );
    let params_json = utils::to_params_json(bind_values)?;
    let raw = executor.query_raw(&sql, &params_json)?;
    serde_json::from_str(&raw).map_err(RuntimeError::from)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
