//! The route icons.
//!
//! They are 16-point outlines drawn with `currentColor`, one 1.2-point stroke
//! and round caps, so they match rune-kit's own glyphs at any tint.

use rune::Icon;

macro_rules! icon {
    ($name:ident, $key:literal, $body:literal) => {
        pub static $name: Icon = Icon {
            name: $key,
            view: 16.0,
            body: $body,
        };
    };
}

icon!(
    HOME,
    "notex-home",
    r#"<path d="M2.6 7.2L8 2.6L13.4 7.2V13.2H9.8V9.4H6.2V13.2H2.6Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>"#
);
icon!(
    TASKS,
    "notex-tasks",
    r#"<path d="M3.2 4.6H13M3.2 8H13M3.2 11.4H9" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><path d="M11.2 10.4L12.4 11.6L14.4 9.4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
);
icon!(
    NOTES,
    "notex-notes",
    r#"<path d="M4 2.5H9L12 5.5V13.5H4Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M8.8 2.5V5.8H12M6 8.5H10M6 10.8H10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(
    TAGS,
    "notex-tags",
    r#"<path d="M2.5 7.4V3.2C2.5 2.8 2.8 2.5 3.2 2.5H7.4L13.4 8.5L8.5 13.4Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><circle cx="5.2" cy="5.2" r="0.9" fill="currentColor"/>"#
);
icon!(
    HABITS,
    "notex-habits",
    r#"<circle cx="8" cy="8" r="5.6" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M5.4 8.2L7.2 10L10.7 6.3" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
);
icon!(
    REMINDERS,
    "notex-reminders",
    r#"<path d="M4.2 7.2C4.2 5.1 5.9 3.4 8 3.4C10.1 3.4 11.8 5.1 11.8 7.2V10L13 12H3L4.2 10Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M6.6 12C6.8 13 7.3 13.5 8 13.5C8.7 13.5 9.2 13 9.4 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(
    SEARCH,
    "notex-search",
    r#"<circle cx="7.2" cy="7.2" r="4.2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M10.4 10.4L13.3 13.3" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(
    SETTINGS,
    "notex-settings",
    r#"<path d="M2.8 5.2H13.2M2.8 10.8H13.2" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="6" cy="5.2" r="1.6" fill="none" stroke="currentColor" stroke-width="1.2"/><circle cx="10.2" cy="10.8" r="1.6" fill="none" stroke="currentColor" stroke-width="1.2"/>"#
);
// Not a route: the failure state's mark.
icon!(
    WARNING,
    "notex-warning",
    r#"<path d="M8 2.4L14.4 13.4H1.6Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M8 6.2V9.4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="8" cy="11.5" r="0.75" fill="currentColor"/>"#
);
// Not routes: the list's buttons and the archive's restore.
icon!(
    TRASH,
    "notex-trash",
    r#"<path d="M3.2 4.8H12.8M6.3 4.8V3.4H9.7V4.8M4.8 4.8L5.4 12.6H10.6L11.2 4.8M6.8 7.2V10.4M9.2 7.2V10.4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
);
icon!(
    ARCHIVE,
    "notex-archive",
    r#"<path d="M2.6 3.4H13.4V6H2.6Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M3.6 6V12.6H12.4V6M6.4 8.4H9.6" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
);
icon!(
    RESTORE,
    "notex-restore",
    r#"<path d="M4.2 4.2V7.4H7.4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M4.4 7.2C5.4 4.9 7.4 3.6 9.8 4C11.9 4.4 13.2 6.2 13.2 8.3C13.2 10.5 11.5 12.4 9 12.4C7.3 12.4 6 11.7 5.1 10.5" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
);
icon!(
    PLUS,
    "notex-plus",
    r#"<path d="M8 3.4V12.6M3.4 8H12.6" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
