//! Placeholder until the parser lands in this segment.
use quire_fs::ReadAt;

use crate::{DocError, Sink};
/// Not yet implemented.
pub fn ingest<R: ReadAt>(_file: &R, _name: &str, _sink: &mut dyn Sink) -> Result<(), DocError> {
    Err(DocError::Unsupported("pdf"))
}
