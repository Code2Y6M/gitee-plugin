//! Text the plugin builds at run time.
//!
//! Manifest text (command titles, secret labels, settings) is localized by
//! `locales/<tag>.json`; strings the code assembles — panel labels, status
//! lines, notifications — pick a branch from the IDE's UI language here.

use openidea_plugin::host;

/// Whether the IDE's UI language is Chinese.
pub fn chinese() -> bool {
    host::locale().to_ascii_lowercase().starts_with("zh")
}

/// `zh` or `en`, by the IDE's UI language.
pub fn t(zh: &str, en: &str) -> String {
    if chinese() { zh.to_owned() } else { en.to_owned() }
}
