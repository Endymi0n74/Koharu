mod archive;
mod github;
mod hugging_face;
mod pypi;

pub use hugging_face::HuggingFaceFile;

pub(crate) use archive::extract;
pub(crate) use github::release_asset;
pub(crate) use pypi::{Platform, wheel};
