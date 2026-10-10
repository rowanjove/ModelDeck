//! Windows 特定运行时实用工具：WebView2 探测、引导安装及致命错误弹窗提示。

#[cfg(windows)]
pub mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            lp_text: *const u16,
            lp_caption: *const u16,
            u_type: u32,
        ) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            h_key: isize,
            lp_sub_key: *const u16,
            ul_options: u32,
            sam_desired: u32,
            phk_result: *mut isize,
        ) -> i32;

        fn RegQueryValueExW(
            h_key: isize,
            lp_value_name: *const u16,
            lp_reserved: *const u32,
            lp_type: *mut u32,
            lp_data: *mut u8,
            lpcb_data: *mut u32,
        ) -> i32;

        fn RegCloseKey(h_key: isize) -> i32;
    }

    const HKEY_LOCAL_MACHINE: isize = 0x80000002u32 as i32 as isize;
    const HKEY_CURRENT_USER: isize = 0x80000001u32 as i32 as isize;
    const KEY_READ: u32 = 0x20019;
    const KEY_WOW64_32KEY: u32 = 0x0200;
    const KEY_WOW64_64KEY: u32 = 0x0100;

    const MB_OK: u32 = 0x00000000;
    const MB_YESNO: u32 = 0x00000004;
    const MB_ICONERROR: u32 = 0x00000010;
    const MB_ICONWARNING: u32 = 0x00000030;
    const IDYES: i32 = 6;

    const WEBVIEW2_DOWNLOAD_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn check_registry_key(root: isize, sub_key: &str, sam: u32) -> bool {
        let wide_sub = to_wide(sub_key);
        let wide_val = to_wide("pv");
        let mut h_key: isize = 0;
        let ret = unsafe {
            RegOpenKeyExW(
                root,
                wide_sub.as_ptr(),
                0,
                KEY_READ | sam,
                &mut h_key as *mut isize,
            )
        };
        if ret != 0 {
            return false;
        }

        let mut data_type: u32 = 0;
        let mut data_len: u32 = 0;
        let ret_len = unsafe {
            RegQueryValueExW(
                h_key,
                wide_val.as_ptr(),
                std::ptr::null(),
                &mut data_type as *mut u32,
                std::ptr::null_mut(),
                &mut data_len as *mut u32,
            )
        };
        unsafe { RegCloseKey(h_key) };

        // pv 键存在且长度大于 2 字节（含结尾空字符）
        ret_len == 0 && data_len > 2
    }

    /// 检查当前系统是否已安装 Microsoft Edge WebView2 Runtime
    pub fn is_webview2_installed() -> bool {
        let subkeys = [
            r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ];

        for &key in &subkeys {
            if check_registry_key(HKEY_LOCAL_MACHINE, key, 0)
                || check_registry_key(HKEY_LOCAL_MACHINE, key, KEY_WOW64_32KEY)
                || check_registry_key(HKEY_LOCAL_MACHINE, key, KEY_WOW64_64KEY)
                || check_registry_key(HKEY_CURRENT_USER, key, 0)
            {
                return true;
            }
        }

        // 检查常见物理安装目录
        let env_vars = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"];
        for var in env_vars {
            if let Ok(prefix) = std::env::var(var) {
                let p = Path::new(&prefix)
                    .join("Microsoft")
                    .join("EdgeWebView")
                    .join("Application");
                if p.is_dir() {
                    return true;
                }
            }
        }

        false
    }

    /// 确保 WebView2 运行时可用，若缺失则弹出中文引导对话框并提供一键下载
    pub fn ensure_webview2() -> Result<(), String> {
        if is_webview2_installed() {
            return Ok(());
        }

        let caption = to_wide("ModelDeck - 缺少运行依赖");
        let text = to_wide(
            "未检测到 Microsoft Edge WebView2 运行时。\n\n\
            ModelDeck 需要 WebView2 运行时以支持界面渲染与原生交互。\n\n\
            是否立即打开微软官方网站下载并安装 WebView2 Runtime？\n\n\
            【是】：打开下载页面\n\
            【否】：退出程序",
        );

        let res = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNO | MB_ICONWARNING,
            )
        };

        if res == IDYES {
            let _ = std::process::Command::new("explorer")
                .arg(WEBVIEW2_DOWNLOAD_URL)
                .spawn();
        }

        Err("未检测到 Microsoft Edge WebView2 运行时".to_string())
    }

    /// 弹出严重错误对话框，避免无控制台时静默闪退
    pub fn show_fatal_error(message: &str) {
        let caption = to_wide("ModelDeck - 运行错误");
        let text = to_wide(message);
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
pub fn ensure_webview2() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn is_webview2_installed() -> bool {
    true
}

#[cfg(not(windows))]
pub fn show_fatal_error(message: &str) {
    eprintln!("Fatal error: {message}");
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn test_webview2_installed_on_windows() {
        // 在 Windows 10/11 开发机上，WebView2 应已正常安装
        assert!(super::is_webview2_installed());
    }
}
