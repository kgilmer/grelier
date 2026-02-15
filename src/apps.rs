use crate::bar::AppIconCache;
use elbey_cache::{AppDescriptor, Cache};
use freedesktop_desktop_entry::desktop_entries;
use locale_config::Locale;

pub fn load_desktop_apps() -> Vec<AppDescriptor> {
    let locales: Vec<String> = Locale::user_default()
        .tags()
        .map(|(_, tag)| tag.to_string())
        .collect();
    desktop_entries(&locales)
        .into_iter()
        .map(AppDescriptor::from)
        .collect()
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

    let app_icons = if workspace_app_icons {
        AppIconCache::from_app_descriptors_ref(&apps)
    } else {
        AppIconCache::default()
    };

    let top_apps = if top_count > 0 {
        cache
            .top_apps(top_count)
            .unwrap_or_default()
            .into_iter()
            .filter(|app| app.exec_count > 0)
            .collect()
    } else {
        Vec::new()
    };

    (apps, app_icons, top_apps)
}
