//! On Windows, files and folders dropped from Explorer are delivered via WM_DROPFILES.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

use anyhow::bail;
use futures::StreamExt as _;
use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Ole::RevokeDragDrop,
        },
        UI::{
            Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP},
            WindowsAndMessaging::{
                CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_COPYDATA,
                WM_DROPFILES, WM_NCDESTROY, WNDPROC,
            },
        },
    },
    core::BOOL,
};

use super::Workspace;

/// A channel sender for each window's dropped paths.
static SENDERS: LazyLock<
    Mutex<HashMap<isize, futures::channel::mpsc::UnboundedSender<Vec<PathBuf>>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The previous window procedure for each hooked window.
static PREV_PROCS: LazyLock<Mutex<HashMap<isize, isize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The window procedure: intercepts WM_DROPFILES and forwards the rest.
unsafe extern "system" fn drop_wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DROPFILES => {
            let hdrop = wparam as HDROP;
            let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, std::ptr::null_mut(), 0) };
            let mut paths = Vec::with_capacity(count as usize);
            for index in 0..count {
                let len = unsafe { DragQueryFileW(hdrop, index, std::ptr::null_mut(), 0) };
                let mut buffer = vec![0u16; len as usize + 1];
                unsafe { DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), len + 1) };
                buffer.pop();
                paths.push(PathBuf::from(String::from_utf16_lossy(&buffer)));
            }
            unsafe { DragFinish(hdrop) };
            log::info!("windows: {count} paths dropped via WM_DROPFILES");
            if let Some(sender) = SENDERS.lock().unwrap().get(&(window as isize)) {
                sender.unbounded_send(paths).ok();
            }
            0
        }
        WM_NCDESTROY => {
            SENDERS.lock().unwrap().remove(&(window as isize));
            let prev = PREV_PROCS
                .lock()
                .unwrap()
                .remove(&(window as isize))
                .unwrap_or(0);
            if prev != 0 {
                let proc: WNDPROC = unsafe { std::mem::transmute(prev) };
                unsafe { CallWindowProcW(proc, window, message, wparam, lparam) }
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        _ => {
            let prev = PREV_PROCS
                .lock()
                .unwrap()
                .get(&(window as isize))
                .copied()
                .unwrap_or(0);
            if prev != 0 {
                let proc: WNDPROC = unsafe { std::mem::transmute(prev) };
                unsafe { CallWindowProcW(proc, window, message, wparam, lparam) }
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
    }
}

/// The HWND gpui made for `window`.
fn native(window: &Window) -> anyhow::Result<HWND> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("windows drop: no native handle: {error:?}"))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        bail!("windows drop: not a Win32 window");
    };
    Ok(win32.hwnd.get() as HWND)
}

/// Listens for file drops from Explorer on `window` and forwards them to the active session.
pub(super) fn listen(window: &Window, cx: &mut Context<Workspace>) -> anyhow::Result<()> {
    let hwnd = native(window)?;
    unsafe {
        RevokeDragDrop(hwnd);
        DragAcceptFiles(hwnd, 1);
        let user32 = GetModuleHandleW(windows_sys::w!("user32.dll"));
        if !user32.is_null() {
            let filter = GetProcAddress(user32, windows_sys::s!("ChangeWindowMessageFilterEx"));
            if let Some(filter) = filter {
                type FilterFn = unsafe extern "system" fn(HWND, u32, u32, *mut ()) -> BOOL;
                let filter: FilterFn = std::mem::transmute(filter);
                filter(hwnd, WM_DROPFILES, 1, std::ptr::null_mut());
                filter(hwnd, WM_COPYDATA, 1, std::ptr::null_mut());
                filter(hwnd, 0x0049, 1, std::ptr::null_mut());
            }
        }
    }
    let (sender, mut receiver) = futures::channel::mpsc::unbounded();
    SENDERS.lock().unwrap().insert(hwnd as isize, sender);
    let prev = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, drop_wndproc as *const () as isize) };
    anyhow::ensure!(prev != 0, "windows drop: SetWindowLongPtrW failed");
    PREV_PROCS.lock().unwrap().insert(hwnd as isize, prev);

    cx.spawn_in(window, async move |workspace, cx| {
        while let Some(paths) = receiver.next().await {
            let ok = cx.update(|window, cx| {
                workspace
                    .update(cx, |this, cx| {
                        this.drop_external_paths(&paths, window, cx);
                    })
                    .ok();
            });
            if ok.is_err() {
                break;
            }
        }
    })
    .detach();

    log::info!("windows: drop target registered for HWND {hwnd:?}");
    Ok(())
}

impl Workspace {
    /// Pastes `paths` dropped from Windows Explorer into the active session.
    pub(super) fn drop_external_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get(self.active) {
            session.terminal.update(cx, |view, cx| {
                view.paste_paths(paths, window, cx);
            });
        }
    }
}
