//! Bevy fluent
//!
//! Bevy plugin for localization using Fluent.

#[doc(inline)]
pub use self::{
    assets::{load, BundleAsset, Data, ResourceAsset},
    exts::bevy::AssetServerExt,
    plugins::FluentPlugin,
    resources::{Locale, Localization},
    systems::parameters::LocalizationBuilder,
};

/// `use bevy_fluent::prelude::*;` to import common assets, components and plugins
pub mod prelude {
    #[doc(inline)]
    pub use super::{
        load, AssetServerExt, BundleAsset, Data, FluentPlugin, Locale, Localization,
        LocalizationBuilder,
    };
}

pub mod assets;
pub mod exts;
pub mod plugins;
pub mod resources;
pub mod systems;
