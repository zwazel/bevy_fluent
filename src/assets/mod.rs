//! Assets
//!
//! Any entity located directly in this module is [`Asset`](bevy::asset::Asset).

#[doc(inline)]
pub use self::{bundle::load, bundle::BundleAsset, bundle::Data, resource::ResourceAsset};

pub mod bundle;
pub mod resource;
