use radroots_replica_db::ReplicaSql;
use radroots_sql_core::SqliteExecutor;

use crate::cli::FindArgs;
use crate::domain::runtime::{
    FindHyfView, FindPriceView, FindQuantityView, FindResultHyfView, FindResultProvenanceView,
    FindResultView, FindView, SyncFreshnessView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::hyf::{self, HyfQueryRewriteRequest, HyfRequestContext};
use crate::runtime::sync::freshness_from_executor;

const FIND_SOURCE: &str = "local replica · local first";
const FIND_HYF_SOURCE: &str = "hyf query_rewrite · local first";
const FIND_HYF_QUERY_REWRITE_REQUEST_ID: &str = "cli-find-query-rewrite";

#[derive(Debug, Clone)]
struct AppliedQueryRewrite {
    rewritten_query: String,
    query_terms: Vec<String>,
}

impl AppliedQueryRewrite {
    fn to_find_view(&self) -> FindHyfView {
        FindHyfView {
            state: "query_rewrite_applied".to_owned(),
            source: FIND_HYF_SOURCE.to_owned(),
            rewritten_query: self.rewritten_query.clone(),
            query_terms: self.query_terms.clone(),
        }
    }

    fn to_result_view(&self) -> FindResultHyfView {
        FindResultHyfView {
            state: "query_rewrite_applied".to_owned(),
            rewritten_query: self.rewritten_query.clone(),
            query_terms: self.query_terms.clone(),
        }
    }
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
            hyf: None,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots local init".to_owned()],
        });
    }

    let db = ReplicaSql::new(SqliteExecutor::open(&config.local.replica_db_path)?);
    let freshness = freshness_from_executor(db.executor())?;
    let applied_query_rewrite = attempt_query_rewrite(config, query.as_str(), &args.query);
    let effective_query_terms = applied_query_rewrite
        .as_ref()
        .map(|rewrite| rewrite.query_terms.clone())
        .unwrap_or_else(|| normalize_query_terms(args.query.clone()));
    let rows = db.trade_product_search(effective_query_terms.as_slice())?;
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
            listing_addr: row.listing_addr.and_then(non_empty),
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
            hyf: applied_query_rewrite
                .as_ref()
                .map(AppliedQueryRewrite::to_result_view),
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
        hyf: applied_query_rewrite.map(|rewrite| rewrite.to_find_view()),
        reason,
        actions,
    })
}

fn attempt_query_rewrite(
    config: &RuntimeConfig,
    query: &str,
    original_terms: &[String],
) -> Option<AppliedQueryRewrite> {
    if query.trim().is_empty() {
        return None;
    }

    let client = hyf::resolve_runtime_client(config).ok()?;
    let response = client
        .query_rewrite(
            FIND_HYF_QUERY_REWRITE_REQUEST_ID,
            Some(FIND_HYF_QUERY_REWRITE_REQUEST_ID),
            &HyfRequestContext::deterministic_cli(),
            &HyfQueryRewriteRequest::new(query),
        )
        .ok()?;

    let rewritten_terms = normalize_query_terms(response.output.query_terms.clone());
    if rewritten_terms.is_empty() {
        return None;
    }

    if rewritten_terms == normalize_query_terms(original_terms.iter().cloned()) {
        return None;
    }

    let rewritten_query = {
        let rewritten_text = response.output.rewritten_text.trim();
        if rewritten_text.is_empty() {
            rewritten_terms.join(" ")
        } else {
            rewritten_text.to_owned()
        }
    };

    Some(AppliedQueryRewrite {
        rewritten_query,
        query_terms: rewritten_terms,
    })
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn normalize_query_terms<I>(terms: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    terms
        .into_iter()
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}
