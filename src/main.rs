// A release build on Windows opens no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ely_gpui_component::Assets;
use elyterminal::{
    app,
    settings::SettingsStore,
    term::shell::{ShellIntegration, launch},
};
use gpui::App;

/// A Linux app with no display server fails at start instead of running headless.
#[cfg(target_os = "linux")]
fn application() -> gpui::Application {
    let modes =
        if std::env::var_os("ELY_WAYLAND").is_none() && std::env::var_os("DISPLAY").is_some() {
            gpui::WindowingModes::X11
        } else {
            gpui::WindowingModes::WAYLAND | gpui::WindowingModes::X11
        };
    gpui_platform::linux(modes)
}

#[cfg(not(target_os = "linux"))]
fn application() -> gpui::Application {
    gpui_platform::application()
}

fn run(cx: &mut App) -> anyhow::Result<()> {
    app::start(cx)?;
    let folder = app::start_folder(std::env::args_os())?;
    let launch = launch(
        &SettingsStore::get(cx).shell,
        folder,
        &cx.global::<ShellIntegration>().0,
    )?;
    app::open(launch, cx)?;
    cx.activate(true);
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("elyterminal=info"))
        .init();
    application().with_assets(Assets).run(|cx: &mut App| {
        ely_gpui_component::init(cx).expect("Ely failed to start");
        if let Err(error) = run(cx) {
            log::error!("{error:#}");
            std::process::exit(1);
        }
    });
}
