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

/// On Windows, force the console input/output code page to UTF-8 (65001) so that
/// Chinese log messages render correctly instead of showing GBK mojibake.
///
/// The default Windows console code page is the system ANSI page (e.g. GBK on
/// zh-CN), which corrupts UTF-8 bytes. Switching to UTF-8 before any logging
/// output avoids that. On non-Windows platforms this is a no-op.
///
/// Alpha 9: Now sets both input and output code pages, and also attempts to
/// configure the console mode for UTF-8 processing at a lower level.
///
/// Best-effort: if the Win32 call fails (rare), we silently continue.
pub fn enable_utf8_console() {
    #[cfg(windows)]
    {
        // SAFETY: Console API calls only change console state;
        // safe to call from the single-threaded startup path.
        unsafe {
            extern "system" {
                fn SetConsoleOutputCP(code_page: u32) -> i32;
                fn SetConsoleCP(code_page: u32) -> i32;
                fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
                fn SetConsoleMode(handle: isize, mode: u32) -> i32;
                fn GetStdHandle(std_handle: u32) -> isize;
            }
            
            const STD_OUTPUT_HANDLE: u32 = 0xFFFFFFF5_u32; // -11 as u32
            const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4_u32; // -12 as u32
            const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
            
            // Set console code pages to UTF-8
            let _ = SetConsoleOutputCP(65001);
            let _ = SetConsoleCP(65001);
            
            // Enable virtual terminal processing for both stdout and stderr
            // This is needed for progress bars (which use stderr) to work correctly
            for handle_id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let handle = GetStdHandle(handle_id);
                if handle != -1 && handle != 0 {
                    let mut mode: u32 = 0;
                    if GetConsoleMode(handle, &mut mode) != 0 {
                        let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                    }
                }
            }
        }
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
