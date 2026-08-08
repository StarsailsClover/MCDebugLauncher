// Lightweight bilingual message support (Alpha 7).
//
// The launcher logs and user-facing strings can be rendered in English
// (default) or Chinese via `--lang zh` / `MDL_LANG=zh`. This is a small
// table-driven helper: call sites provide both languages and `t()` picks
// the active one, so adding Chinese never breaks existing English output.

use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = en, 1 = zh
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(code: &str) {
    let v = match code {
        "zh" | "zh-CN" | "zh-Hans" | "chinese" => 1,
        _ => 0,
    };
    LANG.store(v, Ordering::Relaxed);
}

pub fn is_zh() -> bool {
    LANG.load(Ordering::Relaxed) == 1
}

/// Return the message in the active language.
pub fn t(en: &str, zh: &str) -> String {
    if is_zh() {
        zh.to_string()
    } else {
        en.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_switch() {
        set_lang("en");
        assert_eq!(t("hello", "你好"), "hello");
        set_lang("zh");
        assert_eq!(t("hello", "你好"), "你好");
        set_lang("en"); // reset for other tests
    }
}
