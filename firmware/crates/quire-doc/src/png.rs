//! Placeholder until the decoder lands in this segment.
use quire_fs::ReadAt;
use quire_gfx::Bitmap;

use crate::image::Fit;
use crate::DocError;
/// Not yet implemented.
pub fn decode<R: ReadAt>(_src: &R, _fit: Fit) -> Result<Bitmap, DocError> {
    Err(DocError::Unsupported("png"))
}
