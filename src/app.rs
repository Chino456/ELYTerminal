use std::{borrow::Cow, cell::RefCell, ffi::OsString, path::PathBuf, rc::Rc, time::Duration};

use anyhow::Context as _;
use ely_gpui_component::theme::ActiveTheme;
use gpui::{
    App, AppContext, Bounds, Context, Pixels, Size, Task, TitlebarOptions, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowOptions, px, size,
};

#[cfg(target_os = "macos")]
use crate::term::mac;
use crate::{
    blur, menus,
    settings::{self, SettingsStore},
    term::{ICONS, Launch, MONO, config, input, shell::ShellIntegration, spawn},
    themes, updates,
    workspace::{self, TITLE, Workspace},
};

/// How long the window's size holds before it is kept, so a drag saves once.
const SETTLE: Duration = Duration::from_millis(500);

/// The id the macOS bundle and Linux desktop entry share.
pub const APP_ID: &str = "com.zacharyzhang.elyterminal";

/// Fonts, settings, integration scripts and the chosen font, each checked before a window opens.
pub fn start(cx: &mut App) -> anyhow::Result<()> {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf")),
            Cow::Borrowed(include_bytes!(
                "../assets/fonts/JetBrainsMono-BoldItalic.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf"
            )),
        ])
        .context("app: the bundled fonts failed to register")?;
    let names = cx.text_system().all_font_names();
    for family in [MONO, ICONS] {
        anyhow::ensure!(
            names.iter().any(|name| name == family),
            "app: {family} is not among the registered fonts"
        );
    }
    SettingsStore::init(cx)?;
    updates::start(cx);
    ShellIntegration::init(cx)?;
    input::bind_keys(cx);
    workspace::bind_keys(cx);
    settings::window::bind_keys(cx);
    menus::init(cx);
    cx.set_app_identity(APP_ID, TITLE);
    // A press on a notification goes to the window whose session posted it, or to a newer release's page.
    cx.on_system_notification_response(|response, cx| {
        let tag = response.tag;
        if updates::pressed(&tag, cx) {
            return;
        }
        let pressed = cx
            .windows()
            .into_iter()
            .filter_map(|window| window.downcast::<Workspace>())
            .any(|workspace| {
                workspace
                    .update(cx, |this, window, cx| this.pressed(&tag, window, cx))
                    .unwrap_or(false)
            });
        if !pressed {
            log::info!("app: notification {tag} pressed, but no session holds it");
        }
    });
    // Settings alone hold no terminal, so they go with the last one.
    cx.on_window_closed(|cx, _| {
        let windows = cx.windows();
        if !windows
            .iter()
            .any(|window| window.downcast::<Workspace>().is_some())
        {
            log::info!("app: no terminal window left; quitting");
            cx.quit();
        }
    })
    .detach();
    #[cfg(target_os = "macos")]
    {
        mac::repeat_held_keys();
        mac::deliver_command_key_ups()?;
    }
    #[cfg(target_os = "linux")]
    themes::omarchy::start(cx)?;
    themes::apply(cx);
    let chosen = |settings: &settings::Settings| {
        let theme = (settings.theme.clone(), settings.dark_theme.clone());
        (theme, settings.background_opacity)
    };
    let mut worn = chosen(SettingsStore::get(cx));
    cx.observe_global::<SettingsStore>(move |cx| {
        let now = chosen(SettingsStore::get(cx));
        if now != worn {
            worn = now;
            themes::apply(cx);
        }
    })
    .detach();
    Ok(())
}

/// Opens the main window running `launch`.
pub fn open(launch: Launch, cx: &mut App) -> anyhow::Result<WindowHandle<Workspace>> {
    let kept = opening_size(cx);
    // macOS grows a window by a point on a side whose edge falls on a half point.
    let centered = Bounds::centered(None, kept, cx);
    let bounds = Bounds::new(centered.origin.map(|at| at.round()), kept);
    let background = blur::appearance(SettingsStore::get(cx));
    let least = px(*settings::WINDOW.start());
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // No smaller than the settings keep.
        window_min_size: Some(size(least, least)),
        titlebar: Some(TitlebarOptions {
            title: Some(TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(cx.theme().traffic_light_origin()),
        }),
        // Ely's columns draw the controls; only Linux reads this, and must not draw its own.
        window_decorations: Some(WindowDecorations::Client),
        window_background: background,
        ..Default::default()
    };
    let (pty, events) = spawn(&launch, (80, 24), (8, 16), config(SettingsStore::get(cx)))?;
    let window = cx.open_window(options, |window, cx| {
        window.set_app_id(APP_ID);
        window
            .observe_window_appearance(|_, cx| themes::apply(cx))
            .detach();
        cx.new(|cx| {
            keep_size(window, cx);
            Workspace::new(pty, events, launch, window, cx)
        })
    })?;
    crate::pet::follow(window, cx);
    log::info!(
        "app: main window open at {} by {}, blur: {background:?} on {}",
        f32::from(kept.width),
        f32::from(kept.height),
        std::env::consts::OS
    );
    Ok(window)
}

/// The kept size, within the primary display's visible area where that is known.
fn opening_size(cx: &App) -> Size<Pixels> {
    let settings = SettingsStore::get(cx);
    let room = cx
        .primary_display()
        .map(|display| display.visible_bounds().size);
    let fit = |want: f32, side: Option<Pixels>| match side.map(f32::from) {
        Some(side) if side > 0.0 && want > side => {
            log::info!(
                "app: a kept side of {want} px passes the screen's {side}; the window takes {side}"
            );
            px(side)
        }
        _ => px(want),
    };
    size(
        fit(settings.window_width, room.map(|area| area.width)),
        fit(settings.window_height, room.map(|area| area.height)),
    )
}

/// Keeps the window's size in the settings once it settles; maximized, full screen or hidden, the size before stays.
fn keep_size(window: &mut Window, cx: &mut Context<Workspace>) {
    let settling: Rc<RefCell<Option<Task<()>>>> = Rc::default();
    let settle = move |window: &mut Window, cx: &mut Context<Workspace>| {
        let task = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(SETTLE).await;
            // Read once settled: X11 marks a window maximized late, and unmaximized while hidden.
            let kept = cx.update(|window, cx| {
                if !window.is_visible() || window.is_maximized() || window.is_fullscreen() {
                    log::debug!(
                        "app: the window is hidden, maximized or full screen; its kept size stays"
                    );
                    return Ok(());
                }
                let area = window.window_bounds().get_bounds().size;
                let (width, height) = (
                    f32::from(area.width).round(),
                    f32::from(area.height).round(),
                );
                SettingsStore::update(cx, |settings| {
                    settings.window_width = width;
                    settings.window_height = height;
                })
                .with_context(|| {
                    format!("app: the window's size, {width} by {height}, is not kept")
                })
            });
            match kept {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::error!("{error:#}"),
                Err(_) => log::debug!("app: the window closed before its size settled"),
            }
        });
        settling.replace(Some(task));
    };
    let shown = settle.clone();
    cx.observe_window_bounds(window, move |_, window, cx| settle(window, cx))
        .detach();
    // A size skipped while the window hid is read again once it shows.
    cx.observe_window_visibility(window, move |_, visibility, window, cx| {
        if visibility.is_visible() {
            shown(window, cx);
        }
    })
    .detach();
}

/// Finds the folder for the first session from command-line arguments, CWD or home.
pub fn start_folder(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<PathBuf> {
    if let Some(target) = folder_from_args(args)? {
        return Ok(target);
    }
    #[cfg(not(windows))]
    if let Ok(cwd) = std::env::current_dir() {
        #[cfg(unix)]
        let is_root = cwd == std::path::Path::new("/");
        #[cfg(not(unix))]
        let is_root = false;
        if !is_root && cwd.is_dir() {
            return Ok(cwd);
        }
    }
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("app: no home folder"))
}

/// Reads any folder argument, failing if an explicit path does not exist.
pub fn folder_from_args(
    args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<Option<PathBuf>> {
    let mut iter = args.into_iter();
    iter.next();
    let mut raw_path = None;
    while let Some(arg) = iter.next() {
        let string = arg.to_string_lossy();
        if string == "--working-directory" || string == "-d" {
            let next = iter
                .next()
                .ok_or_else(|| anyhow::anyhow!("app: {string} needs a folder path"))?;
            raw_path = Some(next.to_string_lossy().into_owned());
            break;
        }
        if let Some(rest) = string.strip_prefix("--working-directory=") {
            raw_path = Some(rest.to_string());
            break;
        }
        if let Some(rest) = string.strip_prefix("-d=") {
            raw_path = Some(rest.to_string());
            break;
        }
        if !string.starts_with('-') && raw_path.is_none() {
            raw_path = Some(string.into_owned());
            break;
        }
    }
    match raw_path {
        Some(raw) => resolve_path(&raw).map(Some),
        None => Ok(None),
    }
}

/// Parses a path string or file:// URI into an existing folder.
fn resolve_path(raw: &str) -> anyhow::Result<PathBuf> {
    let unescaped = decode_percent(
        raw.strip_prefix("file://localhost")
            .or_else(|| raw.strip_prefix("file://"))
            .unwrap_or(raw),
    );
    #[cfg(windows)]
    let unescaped = match unescaped.strip_prefix('/') {
        Some(rest) if rest.chars().nth(1) == Some(':') => rest.to_string(),
        _ => unescaped,
    };
    let path = PathBuf::from(unescaped);
    if !path.exists() {
        anyhow::bail!("app: folder {} does not exist", path.display());
    }
    if path.is_file() {
        Ok(path.parent().unwrap_or(&path).to_path_buf())
    } else {
        Ok(path)
    }
}

/// Decodes %XX escapes in URI paths.
fn decode_percent(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..=i + 2]).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn folder_from_flag_and_positional() {
        let temp = std::env::temp_dir();
        let path_str = temp.display().to_string();
        let from_flag = folder_from_args([
            OsString::from("elyterminal"),
            OsString::from("-d"),
            OsString::from(&path_str),
        ])
        .unwrap();
        assert_eq!(from_flag, Some(temp.clone()));
        let from_long_flag = folder_from_args([
            OsString::from("elyterminal"),
            OsString::from(format!("--working-directory={path_str}")),
        ])
        .unwrap();
        assert_eq!(from_long_flag, Some(temp.clone()));
        let from_pos =
            folder_from_args([OsString::from("elyterminal"), OsString::from(&path_str)]).unwrap();
        assert_eq!(from_pos, Some(temp));
    }

    #[test]
    fn folder_from_file_returns_parent() {
        let temp_file = std::env::temp_dir().join("elyterminal_test_file.txt");
        fs::write(&temp_file, "test").unwrap();
        let res = folder_from_args([
            OsString::from("elyterminal"),
            OsString::from(temp_file.display().to_string()),
        ])
        .unwrap();
        let _ = fs::remove_file(&temp_file);
        assert_eq!(res, Some(std::env::temp_dir()));
    }

    #[test]
    fn folder_from_uri() {
        let temp = std::env::temp_dir();
        let uri = format!("file://{}", temp.display());
        let res = folder_from_args([OsString::from("elyterminal"), OsString::from(uri)]).unwrap();
        assert_eq!(res, Some(temp));
    }

    #[test]
    fn folder_nonexistent_fails() {
        let bogus = std::env::temp_dir().join("ely_nonexistent_folder_xyz123");
        let res = folder_from_args([
            OsString::from("elyterminal"),
            OsString::from(bogus.display().to_string()),
        ]);
        assert!(res.is_err());
    }

    #[test]
    fn folder_empty_args_returns_none() {
        let res = folder_from_args([OsString::from("elyterminal")]).unwrap();
        assert_eq!(res, None);
    }
}
