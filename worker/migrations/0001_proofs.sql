-- The proof registry: one row per published proof, written by the compositor
-- after its bundle is fully uploaded.
--
-- `record` holds the whole ProofRecord as JSON so the wire contract has one
-- definition (viewer-proof); the columns beside it are the fields worth
-- querying without parsing.
CREATE TABLE IF NOT EXISTS proofs (
    slug TEXT PRIMARY KEY,
    collection TEXT NOT NULL,
    visibility TEXT NOT NULL,
    created_at TEXT NOT NULL,
    record TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS proofs_collection ON proofs (collection);
