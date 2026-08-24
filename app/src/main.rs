//! Entry point — gpui-component bootstrap.

use chm_app::config::{Cli, CliError, load_config};
use chm_app::pages::settings::{appearance_from_cfg, apply_appearance};
use chm_app::shell::{OpenSettings, Refresh, Shell, ToggleSidebar};
use gpui::{
    App, AppContext as _, Bounds, Focusable as _, KeyBinding, Menu, MenuItem, SharedString,
    TitlebarOptions, WindowBounds, WindowOptions, actions, px, size,
};
use gpui_component::Root;

actions!(chm_app, [Quit]);

fn main() {
    match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => chm_app::config::install_cli(cli),
        Err(CliError::Help) => {
            print!("{}", chm_app::config::HELP);
            return;
        }
        Err(CliError::Version) => {
            println!("chm-app {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(CliError::Unknown(arg)) => {
            eprintln!("unknown argument: {arg}\n{}", chm_app::config::HELP);
            std::process::exit(2);
        }
    }

    // Smoke gate: prove init runs to completion headless before any window is
    // opened. CHM_SMOKE also selects MockDataSource inside Shell (see shell.rs).
    let smoke = std::env::var("CHM_SMOKE").is_ok();
    if smoke {
        println!("shell ready");
    }

    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("cmd-b", ToggleSidebar, None),
                KeyBinding::new("cmd-r", Refresh, None),
                KeyBinding::new("cmd-q", Quit, None),
            ]);
            // Without a menu item cmd-q does nothing — no nib ships with a gpui app.
            cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
            cx.set_menus(vec![
                Menu::new("chmonitor").items(vec![
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit", Quit),
                ]),
                Menu::new("View").items(vec![
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Refresh", Refresh),
                ]),
            ]);

            let appearance = appearance_from_cfg(load_config().ui.appearance.as_deref());
            let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from("chmonitor")),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    apply_appearance(appearance, window, cx);
                    let shell = cx.new(|cx| Shell::new(window, cx));
                    let focus = shell.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                    cx.new(|cx| Root::new(shell, window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
