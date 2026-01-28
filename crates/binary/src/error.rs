//! Error types for binary format operations.

use thiserror::Error;

/// Errors that can occur when reading or writing the binary format.
#[derive(Debug, Error)]
pub enum BinaryFormatError {
    /// Invalid magic bytes in header
    #[error("invalid magic bytes: expected COLL, got {0:?}")]
    InvalidMagic([u8; 4]),

    /// Unsupported format version
    #[error("unsupported version: {0} (current: {1})")]
    UnsupportedVersion(u16, u16),

    /// String table overflow (>65535 strings or >64KB data)
    #[error("string table overflow: {0}")]
    StringTableOverflow(String),

    /// Too many trait:value combinations for any bitmap size
    #[error("too many trait values: {0} (max 1024). Add traits to ignore list.")]
    TooManyTraitValues(usize),

    /// Token count exceeds u16::MAX
    #[error("too many tokens: {0} (max 65535)")]
    TooManyTokens(usize),

    /// Invalid section offset
    #[error("invalid section offset: {0}")]
    InvalidOffset(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// PHF build failed
    #[error("PHF build failed: {0}")]
    PhfBuildFailed(String),
}
