//! The proof registry.
//!
//! A proof is generated and uploaded by the compositor; this is where it is
//! recorded, so a reviewer holding a token number can find the seed, the count
//! and the config it came from. The record's shape lives in `viewer-proof`.

use serde::Deserialize;
use viewer_proof::{ProofRecord, ProofSubmission};
use worker_stack::js_sys;
use worker_stack::prelude::*;

use crate::auth;

/// Re-posting the same slug replaces its record: the compositor retries the
/// registration until it gets a response, and a retry must not fail on the row
/// it just wrote.
const UPSERT: &str = "\
INSERT INTO proofs (slug, collection, visibility, created_at, record)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT(slug) DO UPDATE SET
    collection = excluded.collection,
    visibility = excluded.visibility,
    created_at = excluded.created_at,
    record = excluded.record";

/// `POST /api/proofs` — record a published proof.
pub async fn create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Some(rejection) = auth::authorize(&req, &ctx.env)? {
        return Ok(rejection);
    }

    let body = req.text().await?;
    let submission: ProofSubmission = match serde_json::from_str(&body) {
        Ok(submission) => submission,
        Err(error) => {
            return Response::error(format!("invalid proof record: {error}"), 400);
        }
    };
    if let Err(message) = submission.validate() {
        return Response::error(message, 400);
    }

    let record = ProofRecord::from_submission(
        submission,
        String::from(js_sys::Date::new_0().to_iso_string()),
    );
    let stored = serde_json::to_string(&record)?;

    ctx.env
        .d1("DB")?
        .prepare(UPSERT)
        .bind(&[
            JsValue::from_str(&record.slug),
            JsValue::from_str(&record.collection),
            JsValue::from_str(record.visibility.as_str()),
            JsValue::from_str(&record.created_at),
            JsValue::from_str(&stored),
        ])?
        .run()
        .await?;

    Response::from_json(&record)
}

#[derive(Deserialize)]
struct StoredRecord {
    record: String,
}

/// `GET /api/proofs/:slug` — the record a published slug was registered with.
pub async fn get(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(slug) = ctx.param("slug") else {
        return Response::error("missing slug", 400);
    };

    let row: Option<StoredRecord> = ctx
        .env
        .d1("DB")?
        .prepare("SELECT record FROM proofs WHERE slug = ?1")
        .bind(&[JsValue::from_str(slug)])?
        .first(None)
        .await?;

    let Some(row) = row else {
        return Response::error("no such proof", 404);
    };
    let record: ProofRecord = serde_json::from_str(&row.record)?;
    Response::from_json(&record)
}
