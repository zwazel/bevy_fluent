use crate::{resources::Settings, FluentAsset};
use bevy::{
    prelude::*,
    utils::tracing::{self, instrument},
};
use fluent::{bundle::FluentBundle, FluentResource};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use intl_memoizer::concurrent::IntlLangMemoizer;
use std::{collections::HashMap, ops::Deref, sync::Arc};
use unic_langid::LanguageIdentifier;

#[instrument(skip(assets, handles))]
fn build_bundle(
    assets: &Assets<FluentAsset>,
    handles: &[Handle<FluentAsset>],
    locale: Option<LanguageIdentifier>,
) -> FluentBundle<Arc<FluentResource>, IntlLangMemoizer> {
    let mut fluent_bundle = FluentBundle::new_concurrent(locale.into_iter().collect());
    for handle in handles {
        let asset = assets.get(handle).unwrap();
        if let Err(errors) = fluent_bundle.add_resource(asset.0.clone()) {
            warn_span!("add_resource").in_scope(|| {
                for error in errors {
                    warn!(%error);
                }
            });
        }
    }
    fluent_bundle
}

fn request_locales<'a>(
    available_locales: &[&'a LanguageIdentifier],
    default_locale: &'a Option<LanguageIdentifier>,
    requested_locales: &[LanguageIdentifier],
) -> Vec<&'a LanguageIdentifier> {
    let default_locale = default_locale.as_ref();
    let supported_locales = negotiate_languages(
        requested_locales,
        available_locales,
        default_locale.as_ref(),
        NegotiationStrategy::Filtering,
    );
    supported_locales.into_iter().copied().collect()
}

/// Snapshot
///
/// Note: if locale fallback chain is empty then it is interlocale bundle.
pub struct Snapshot(
    HashMap<Option<LanguageIdentifier>, FluentBundle<Arc<FluentResource>, IntlLangMemoizer>>,
);

impl Snapshot {
    pub fn locales(&self) -> impl Iterator<Item = Option<&LanguageIdentifier>> {
        self.keys().map(Option::as_ref)
    }
}

impl FromWorld for Snapshot {
    fn from_world(world: &mut World) -> Self {
        let Settings {
            default_locale,
            fallback_locale_chain,
            ..
        } = world
            .get_resource::<Settings>()
            .expect("get `Settings` resource");
        let fluent_assets = world
            .get_resource::<Assets<FluentAsset>>()
            .expect("get `Assets<Resource>` resource");

        #[cfg(feature = "implicit")]
        let available_locale_handles = implicit::retrieve_locale_handles(world);
        #[cfg(not(feature = "implicit"))]
        let available_locale_handles = explicit::retrieve_locale_handles(world);
        let available_locales: Vec<_> = available_locale_handles.keys().flatten().collect();
        let supported_locales =
            request_locales(&available_locales, default_locale, fallback_locale_chain);
        debug!(
            available_locales =
                ?|| -> Vec<_> { available_locales.iter().map(ToString::to_string).collect() }(),
            supported_locales =
                ?|| -> Vec<_> { supported_locales.iter().map(ToString::to_string).collect() }(),
        );
        let supported_locale_handles =
            available_locale_handles
                .iter()
                .filter(|(locale, _)| match locale {
                    None => true,
                    Some(locale) => supported_locales.contains(&locale),
                });
        let mut bundles = HashMap::new();
        for (locale, handles) in supported_locale_handles {
            let bundle = build_bundle(fluent_assets, handles, locale.clone());
            bundles.insert(locale.clone(), bundle);
        }
        debug!("`Snapshot` is initialized");
        Snapshot(bundles)
    }
}

impl Deref for Snapshot {
    type Target =
        HashMap<Option<LanguageIdentifier>, FluentBundle<Arc<FluentResource>, IntlLangMemoizer>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "implicit")]
mod implicit {
    use crate::{resources::Settings, FluentAsset};
    use bevy::{
        asset::{AssetPath, AssetServerSettings},
        prelude::*,
        utils::tracing::{self, instrument},
    };
    use std::{collections::HashMap, ffi::OsStr, path::Path};
    use unic_langid::LanguageIdentifier;
    use walkdir::WalkDir;

    #[instrument(skip(world))]
    pub(super) fn retrieve_locale_handles(
        world: &World,
    ) -> HashMap<Option<LanguageIdentifier>, Vec<Handle<FluentAsset>>> {
        let AssetServerSettings { asset_folder } = world
            .get_resource::<AssetServerSettings>()
            .expect("get AssetServerSettings resource");
        let Settings { locales_folder, .. } = world
            .get_resource::<Settings>()
            .expect("get Settings resource");
        let asset_server = world
            .get_resource::<AssetServer>()
            .expect("get AssetServer resource");
        let mut locale_handles = HashMap::new();
        for entry in WalkDir::new(Path::new(asset_folder).join(locales_folder)) {
            match entry {
                Ok(entry) => {
                    let mut path = entry.path();
                    if path.extension() == Some(OsStr::new("ftl")) {
                        path = path.strip_prefix(asset_folder).unwrap();
                        trace!(?path);
                        let asset_path = AssetPath::new(path.to_path_buf(), None);
                        let handle: Handle<FluentAsset> = asset_server.get_handle(asset_path);
                        path = path.strip_prefix(locales_folder).unwrap();
                        let locale = parse_locale(path);
                        locale_handles
                            .entry(locale)
                            .or_insert_with(Vec::new)
                            .push(handle);
                    }
                }
                Err(error) => error!(%error),
            }
        }
        locale_handles
    }

    fn parse_locale(path: &Path) -> Option<LanguageIdentifier> {
        // TODO: https://github.com/rust-lang/rust/issues/68537
        let mut language_identifiers = path
            .iter()
            .map(|component| {
                component
                    .to_str()
                    .map(|component| component.strip_suffix(".ftl").unwrap_or(component))?
                    .parse()
                    .ok()
            })
            .take_while(Option::is_some)
            .map(Option::unwrap);
        let mut parent: LanguageIdentifier = language_identifiers.next()?;
        for child in language_identifiers {
            if parent.matches(&child, true, false) {
                parent = child;
            } else {
                break;
            }
        }
        Some(parent)
    }
}

#[cfg(not(feature = "implicit"))]
mod explicit {
    use crate::{
        assets::{FluentAsset, LocaleAssets},
        resources::Settings,
    };
    use bevy::{
        asset::AssetServerSettings,
        prelude::*,
        utils::tracing::{self, instrument},
    };
    use std::{collections::HashMap, ffi::OsStr, path::Path};
    use unic_langid::LanguageIdentifier;
    use walkdir::WalkDir;

    #[instrument(skip(world))]
    pub(super) fn retrieve_locale_handles(
        world: &World,
    ) -> HashMap<Option<LanguageIdentifier>, Vec<Handle<FluentAsset>>> {
        let AssetServerSettings { asset_folder } = world
            .get_resource::<AssetServerSettings>()
            .expect("get `AssetServerSettings` resource");
        let Settings { locales_folder, .. } = world
            .get_resource::<Settings>()
            .expect("get `Settings` resource");
        let asset_server = world
            .get_resource::<AssetServer>()
            .expect("get `AssetServer` resource");
        let locale_assets = world
            .get_resource::<Assets<LocaleAssets>>()
            .expect("get `Assets<Bundle>` resource");
        let mut locale_handles = HashMap::new();
        for entry in WalkDir::new(Path::new(asset_folder).join(locales_folder)) {
            match entry {
                Ok(entry) => {
                    let mut path = entry.path();
                    if path.file_name() == Some(OsStr::new("locale.ron")) {
                        path = path.strip_prefix(asset_folder).unwrap();
                        trace!(?path);
                        let handle: Handle<LocaleAssets> = asset_server.load(path);
                        path = path.strip_prefix(locales_folder).unwrap();
                        let locale = parse_locale(path);
                        let locale_assets = locale_assets.get(handle).unwrap();
                        let handles = locale_assets.iter().cloned().collect();
                        locale_handles.insert(locale, handles);
                    }
                }
                Err(error) => error!(%error),
            }
        }
        locale_handles
    }

    fn parse_locale(path: &Path) -> Option<LanguageIdentifier> {
        path.iter().rev().nth(1)?.to_str()?.parse().ok()
    }
}