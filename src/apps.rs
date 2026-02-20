use crate::bar::AppIconCache;
use elbey_cache::{AppDescriptor, Cache};
use freedesktop_desktop_entry::desktop_entries;
use locale_config::Locale;
use std::collections::HashSet;

fn dedup_apps(apps: Vec<AppDescriptor>) -> Vec<AppDescriptor> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(apps.len());
    for app in apps {
        if seen.insert(app.appid.clone()) {
            deduped.push(app);
        }
    }
    deduped
}

fn select_top_apps(cache: &mut Cache, top_count: usize) -> Vec<AppDescriptor> {
    if top_count == 0 {
        return Vec::new();
    }

    cache
        .top_apps(top_count)
        .unwrap_or_default()
        .into_iter()
        .filter(|app| app.exec_count > 0)
        .collect()
}

pub fn load_desktop_apps() -> Vec<AppDescriptor> {
    let locales: Vec<String> = Locale::user_default()
        .tags()
        .map(|(_, tag)| tag.to_string())
        .collect();
    dedup_apps(
        desktop_entries(&locales)
            .into_iter()
            .map(AppDescriptor::from)
            .collect(),
    )
}

pub fn load_cached_apps_from_cache(
    cache: &mut Cache,
    top_count: usize,
    workspace_app_icons: bool,
) -> (Vec<AppDescriptor>, AppIconCache, Vec<AppDescriptor>) {
    let apps = if workspace_app_icons || top_count > 0 {
        cache.load_apps()
    } else {
        Vec::new()
    };
    let apps = dedup_apps(apps);

    let app_icons = if workspace_app_icons {
        AppIconCache::from_app_descriptors_ref(&apps)
    } else {
        AppIconCache::default()
    };

    let top_apps = select_top_apps(cache, top_count);

    (apps, app_icons, top_apps)
}
