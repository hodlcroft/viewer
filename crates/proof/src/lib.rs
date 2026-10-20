//! A published proof's registry record.
//!
//! The compositor builds a proof and uploads its bundle; this crate is the
//! contract it posts to `POST /api/proofs`, and the shape the viewer worker
//! stores in D1 and returns from `GET /api/proofs/:slug`.
//!
//! It is a leaf: types and their validation, no I/O, so the worker, the
//! operator CLI and the compositor all read one definition.

use serde::{Deserialize, Serialize};

/// Who may open a proof.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Anyone with the link — the random slug suffix is the only guard.
    #[default]
    Unlisted,
    /// A signed-in member holding a grant. Stored now; enforcement arrives
    /// with the viewer's auth work.
    Private,
}

impl Visibility {
    /// The value stored in the registry's `visibility` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Visibility::Unlisted => "unlisted",
            Visibility::Private => "private",
        }
    }
}

/// What the compositor sends after its bundle is fully uploaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofSubmission {
    /// The viewer slug the bundle lives under (`preview/<slug>/`).
    pub slug: String,
    /// The collection the stills were generated from, for display.
    pub collection: String,
    /// The seed that reproduces the run.
    pub seed: u64,
    /// How many tokens the run generated — not how many stills were published:
    /// a proof of a subset is still reproduced from the whole collection.
    pub count: u32,
    /// SHA-256 of the config the proof was generated from, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,
    /// Who may open it. Defaults to unlisted.
    #[serde(default)]
    pub visibility: Visibility,
}

impl ProofSubmission {
    /// Reject a record that could not be served back by slug.
    pub fn validate(&self) -> Result<(), String> {
        if self.slug.is_empty() {
            return Err("slug is empty".into());
        }
        if self.slug.len() > 80 {
            return Err(format!("slug {:?} is longer than 80 characters", self.slug));
        }
        if !self
            .slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(format!(
                "slug {:?} must be ASCII letters, digits and '-'",
                self.slug
            ));
        }
        if self.slug.starts_with('-') || self.slug.ends_with('-') {
            return Err(format!(
                "slug {:?} has a leading or trailing '-'",
                self.slug
            ));
        }
        if self.collection.trim().is_empty() {
            return Err("collection is empty".into());
        }
        if self.count == 0 {
            return Err("count must be greater than zero".into());
        }
        if let Some(hash) = &self.config_hash
            && hash.is_empty()
        {
            return Err("config_hash is present but empty".into());
        }
        Ok(())
    }
}

/// The record as stored and served: the submission plus the server's time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofRecord {
    pub slug: String,
    pub collection: String,
    pub seed: u64,
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,
    pub visibility: Visibility,
    /// When the worker accepted the record (ISO 8601, server clock).
    pub created_at: String,
}

impl ProofRecord {
    /// Stamp a submission with the time the worker accepted it. The server
    /// owns the clock so a proof's age does not depend on the publisher.
    pub fn from_submission(submission: ProofSubmission, created_at: impl Into<String>) -> Self {
        Self {
            slug: submission.slug,
            collection: submission.collection,
            seed: submission.seed,
            count: submission.count,
            config_hash: submission.config_hash,
            visibility: submission.visibility,
            created_at: created_at.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission() -> ProofSubmission {
        ProofSubmission {
            slug: "mini-perps-jwbl8h3t".into(),
            collection: "Mini Perps".into(),
            seed: u64::MAX,
            count: 1000,
            config_hash: Some("a".repeat(64)),
            visibility: Visibility::Unlisted,
        }
    }

    #[test]
    fn a_submission_round_trips_without_losing_the_seed() {
        let json = serde_json::to_string(&submission()).unwrap();
        let parsed: ProofSubmission = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, submission());
        assert_eq!(parsed.seed, u64::MAX, "the seed is not narrowed by JSON");
    }

    #[test]
    fn visibility_defaults_to_unlisted() {
        let json = r#"{
            "slug": "mini-perps-jwbl8h3t",
            "collection": "Mini Perps",
            "seed": 42,
            "count": 1000
        }"#;
        let parsed: ProofSubmission = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.visibility, Visibility::Unlisted);
        assert_eq!(parsed.config_hash, None);
    }

    #[test]
    fn a_record_takes_the_submission_and_the_servers_time() {
        let record = ProofRecord::from_submission(submission(), "2026-10-21T09:00:00.000Z");
        assert_eq!(record.slug, "mini-perps-jwbl8h3t");
        assert_eq!(record.count, 1000);
        assert_eq!(record.created_at, "2026-10-21T09:00:00.000Z");
        assert_eq!(record.visibility.as_str(), "unlisted");
    }

    #[test]
    fn records_that_could_not_be_served_are_refused() {
        for (slug, why) in [
            ("", "empty"),
            ("mini perps", "space"),
            ("mini_perps", "underscore"),
            ("-mini-perps", "leading dash"),
            ("mini-perps-", "trailing dash"),
            ("a/b", "slash"),
        ] {
            let mut record = submission();
            record.slug = slug.into();
            assert!(record.validate().is_err(), "{why} should be refused");
        }

        let mut record = submission();
        record.collection = "  ".into();
        assert!(record.validate().is_err());

        let mut record = submission();
        record.count = 0;
        assert!(record.validate().is_err());

        let mut record = submission();
        record.config_hash = Some(String::new());
        assert!(record.validate().is_err());

        assert!(submission().validate().is_ok());
    }
}
