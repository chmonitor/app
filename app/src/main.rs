//! Entry point — bezel bootstrap (mirrors bezel `apps/hello` exactly).
//!
//! Bootstrap quirks worth remembering:
//! * This gpui fork has no `Application::new()`; the platform facade
//!   `gpui_platform::application()` is the only entry.
//! * A gpui app gets no menu bar for free (no nib), so without a Quit
//!   menu item `cmd-q` does nothing.
//! * Fonts must be registered before the first window paints.

use bezel::gpui::{
    App, AppContext as _, Bounds, Focusable as _, Menu, MenuItem, SharedString, TitlebarOptions,
    WindowBounds, WindowOptions, actions, px, size,
};
use bezel::theme;
use bezel::ui;
use chm_app::config::{Cli, CliError};
use chm_app::shell::Shell;

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

    gpui_platform::application().run(|cx: &mut App| {
        if let Err(err) = ui::register_fonts(cx) {
            eprintln!("FONT REGISTRATION FAILED: {err:?}");
        }
        theme::appearance::init(theme::appearance::AppearanceMode::System, cx);
        // TextField keybindings are opt-in and scoped to the field's key context.
        ui::input::init(cx);
        // Without a menu item cmd-q does nothing — no nib ships with a gpui app.
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        cx.set_menus(vec![
            Menu::new("chmonitor").items([MenuItem::action("Quit", Quit)]),
        ]);

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
            |window, cx| {
                // Follow system light/dark for this window (bezel README step).
                theme::appearance::observe_window(window, cx).detach();
                let shell = cx.new(Shell::new);
                // Root takes focus so 1-8 / r keys land in the shell's subtree
                // from the first frame (gallery's pattern).
                let focus = shell.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                shell
            },
        )
        .unwrap(); // unwrap allowed here: same bootstrap shape as bezel examples
        cx.activate(true);
    });
}
