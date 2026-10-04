//! OpenThunder TUI launcher.
//!
//! A small, dependency-light terminal UI (raw ANSI escapes + `libc` termios) that
//! lets the player launch the game and rebind keys. Keybinds are shared with the
//! game through [`openthunder::keybinds`], so anything changed here takes effect
//! the next time the game starts.
//!
//! Run it with `cargo run --bin launcher`.

use std::io::{self, Write};
use std::process::Command;

use openthunder::keybinds::{self, ACTIONS, Keybinds, SUPPORTED_KEYS, key_display};
use openthunder::planes;
use openthunder::servers::{ServerList, servers_path};
use openthunder::settings::Settings;

// ---------------------------------------------------------------------------
// Terminal handling (raw mode via libc; no external TUI crate required)
// ---------------------------------------------------------------------------

struct Terminal {
    original: libc::termios,
    raw: bool,
    alt: bool,
}

impl Terminal {
    fn new() -> io::Result<Self> {
        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                original: termios,
                raw: false,
                alt: false,
            })
        }
    }

    fn enable_raw(&mut self) -> io::Result<()> {
        unsafe {
            let mut raw = self.original;
            // No line buffering, no echo, no Ctrl-C signal (we handle it ourselves).
            raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            raw.c_iflag &= !(libc::IXON | libc::ICRNL);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            self.raw = true;
            Ok(())
        }
    }

    fn disable_raw(&mut self) {
        if self.raw {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
            }
            self.raw = false;
        }
    }

    /// Switch to the alternate screen buffer and hide the cursor.
    fn enter_alt(&mut self) {
        print!("\x1b[?1049h\x1b[?25l\x1b[2J");
        let _ = io::stdout().flush();
        self.alt = true;
    }

    fn leave_alt(&mut self) {
        if self.alt {
            print!("\x1b[?25h\x1b[?1049l");
            let _ = io::stdout().flush();
            self.alt = false;
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.disable_raw();
        self.leave_alt();
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Backspace,
    Char(char),
    CtrlC,
    Other,
}

fn poll_input(timeout_ms: i32) -> bool {
    unsafe {
        let mut fds = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        libc::poll(&mut fds, 1, timeout_ms) > 0
    }
}

fn read_byte() -> Option<u8> {
    unsafe {
        let mut byte: u8 = 0;
        let count = libc::read(
            libc::STDIN_FILENO,
            &mut byte as *mut u8 as *mut libc::c_void,
            1,
        );
        if count == 1 { Some(byte) } else { None }
    }
}

fn read_key() -> Key {
    let Some(byte) = read_byte() else {
        return Key::Other;
    };
    match byte {
        // Escape, or the start of a CSI arrow sequence.
        0x1b => {
            if !poll_input(25) {
                return Key::Esc;
            }
            if read_byte() != Some(b'[') || !poll_input(25) {
                return Key::Esc;
            }
            match read_byte() {
                Some(b'A') => Key::Up,
                Some(b'B') => Key::Down,
                Some(b'C') => Key::Right,
                Some(b'D') => Key::Left,
                _ => Key::Esc,
            }
        }
        0x03 => Key::CtrlC,
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        b if (0x20..=0x7e).contains(&b) => Key::Char(b as char),
        _ => Key::Other,
    }
}

/// Maps a key event to a canonical key name accepted by the config.
fn key_name(key: &Key) -> Option<&'static str> {
    match key {
        Key::Up => Some("ArrowUp"),
        Key::Down => Some("ArrowDown"),
        Key::Left => Some("ArrowLeft"),
        Key::Right => Some("ArrowRight"),
        Key::Enter => Some("Enter"),
        Key::Backspace => Some("Backspace"),
        Key::Char(c) => {
            if *c == ' ' {
                return Some("Space");
            }
            let upper = c.to_ascii_uppercase();
            if !(upper.is_ascii_uppercase() || upper.is_ascii_digit()) {
                return None;
            }
            let candidate = upper.to_string();
            SUPPORTED_KEYS
                .iter()
                .copied()
                .find(|name| *name == candidate.as_str())
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(lines: &[String]) {
    let mut out = String::from("\x1b[2J\x1b[H");
    for (index, line) in lines.iter().enumerate() {
        out.push_str(&format!("\x1b[{};1H\x1b[2K{}", index + 1, line));
    }
    print!("{out}");
    let _ = io::stdout().flush();
}

fn menu_lines(
    selected: usize,
    status: &str,
    plane: &str,
    server: &str,
    fullscreen: bool,
) -> Vec<String> {
    let display = if fullscreen { "Fullscreen" } else { "Windowed" };
    let items = [
        "Launch game".to_string(),
        "Select aircraft".to_string(),
        format!("Server: {server}"),
        format!("Display: {display}  (Enter to switch)"),
        "Edit keybinds".to_string(),
        "Reset keybinds to defaults".to_string(),
        "Quit".to_string(),
    ];
    let mut lines = vec![
        String::new(),
        "  \x1b[1;36mOpenThunder\x1b[0m  -  War Thunder Air RB prototype".to_string(),
        format!("  \x1b[2mAircraft: {plane}   Server: {server}\x1b[0m"),
        "  \x1b[2mUp/Down to move, Enter to select\x1b[0m".to_string(),
        String::new(),
    ];
    for (index, item) in items.iter().enumerate() {
        if index == selected {
            lines.push(format!("  \x1b[7m> {item}\x1b[0m"));
        } else {
            lines.push(format!("    {item}"));
        }
    }
    lines.push(String::new());
    if !status.is_empty() {
        lines.push(format!("  \x1b[2m{status}\x1b[0m"));
    }
    lines.push(String::new());
    lines.push(format!(
        "  \x1b[2mConfig: {}\x1b[0m",
        keybinds::keybinds_path().display()
    ));
    lines
}

/// `(name, address)` for every selectable server, with single-player first.
fn server_entries(servers: &ServerList) -> Vec<(String, String)> {
    let mut entries = vec![("Single-player".to_string(), String::new())];
    for server in &servers.servers {
        entries.push((server.name.clone(), server.address.clone()));
    }
    entries
}

fn server_lines(
    entries: &[(String, String)],
    current: &str,
    selected: usize,
    status: &str,
) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "  \x1b[1;36mSelect server\x1b[0m".to_string(),
        "  \x1b[2mUp/Down select   Enter choose   Esc back\x1b[0m".to_string(),
        String::new(),
    ];
    for (index, (name, address)) in entries.iter().enumerate() {
        let marker = if address == current {
            "  < selected"
        } else {
            ""
        };
        let text = format!("{name:<22} {address}{marker}");
        if index == selected {
            lines.push(format!("  \x1b[7m> {text}\x1b[0m"));
        } else {
            lines.push(format!("    {text}"));
        }
    }
    lines.push(String::new());
    if !status.is_empty() {
        lines.push(format!("  \x1b[2m{status}\x1b[0m"));
    }
    lines.push(String::new());
    lines.push(format!(
        "  \x1b[2mAdd your own servers in {}\x1b[0m",
        servers_path().display()
    ));
    lines
}

fn plane_lines(current: &str, selected: usize, status: &str) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "  \x1b[1;36mSelect aircraft\x1b[0m".to_string(),
        "  \x1b[2mUp/Down select   Enter choose   Esc back\x1b[0m".to_string(),
        String::new(),
    ];
    for (index, plane) in planes::PLANES.iter().enumerate() {
        let marker = if plane.id == current {
            "  < selected"
        } else {
            ""
        };
        let label = format!("{:<22}", plane.label);
        let text = format!("{label} [{}]{marker}", plane.nation);
        if index == selected {
            lines.push(format!("  \x1b[7m> {text}\x1b[0m"));
        } else {
            lines.push(format!("    {text}"));
        }
        lines.push(format!("      \x1b[2m{}\x1b[0m", plane.description));
    }
    lines.push(String::new());
    if !status.is_empty() {
        lines.push(format!("  \x1b[2m{status}\x1b[0m"));
    }
    lines
}

fn keybind_lines(
    keybinds: &Keybinds,
    selected: usize,
    capturing: bool,
    status: &str,
) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "  \x1b[1;36mKeybinds\x1b[0m".to_string(),
        "  \x1b[2mUp/Down select   Enter rebind   Backspace reset one   Esc back\x1b[0m"
            .to_string(),
        "  \x1b[2mChanges are saved automatically.\x1b[0m".to_string(),
        String::new(),
    ];
    for (index, action) in ACTIONS.iter().enumerate() {
        let key = key_display(keybinds.get(index));
        let label = format!("{:<22}", action.label);
        if index == selected {
            if capturing {
                lines.push(format!("  \x1b[7m> {label} [ press a key... ]\x1b[0m"));
            } else {
                lines.push(format!("  \x1b[7m> {label} {key}\x1b[0m"));
            }
        } else {
            lines.push(format!("    {label} {key}"));
        }
    }
    lines.push(String::new());
    if !status.is_empty() {
        lines.push(format!("  \x1b[2m{status}\x1b[0m"));
    }
    lines
}

// ---------------------------------------------------------------------------
// Launching the game
// ---------------------------------------------------------------------------

/// Prefer the already-built game binary next to this launcher; fall back to
/// `cargo run` so the launcher works during development too. The chosen
/// aircraft and server are passed through as `--plane` / `--server`.
fn game_command(plane: &str, server: &str) -> Command {
    let mut args: Vec<String> = vec!["--plane".to_string(), plane.to_string()];
    if !server.trim().is_empty() {
        args.push("--server".to_string());
        args.push(server.to_string());
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(format!("openthunder{}", std::env::consts::EXE_SUFFIX));
            if candidate.exists() {
                let mut command = Command::new(candidate);
                command.args(&args);
                return command;
            }
        }
    }
    let mut command = Command::new("cargo");
    command.args(["run", "--bin", "openthunder", "--"]);
    command.args(&args);
    command
}

fn launch_game(terminal: &mut Terminal, keybinds: &Keybinds, plane: &str, server: &str) {
    let _ = keybinds.save();

    // Hand the terminal back to the child process.
    terminal.disable_raw();
    terminal.leave_alt();
    let where_to = if server.trim().is_empty() {
        "single-player".to_string()
    } else {
        format!("server {server}")
    };
    println!(
        "Launching OpenThunder ({plane}, {where_to})... (close the game window to return here)"
    );
    let _ = io::stdout().flush();

    match game_command(plane, server).status() {
        Ok(status) if status.success() => {}
        Ok(status) => println!("Game exited with {status}."),
        Err(err) => eprintln!("Failed to launch the game: {err}"),
    }

    println!("Press Enter to return to the launcher...");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    let _ = terminal.enable_raw();
    terminal.enter_alt();
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

enum Screen {
    Menu,
    Planes,
    Servers,
    Keybinds,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("openthunder launcher: {err}");
        eprintln!("This launcher needs an interactive terminal (TTY).");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut keybinds = Keybinds::load_or_create();
    let mut settings = Settings::load_or_create();
    let entries = server_entries(&ServerList::load_or_create());
    let mut terminal = Terminal::new()?;
    terminal.enable_raw()?;
    terminal.enter_alt();

    let mut screen = Screen::Menu;
    let mut menu_selected = 0usize;
    let mut plane_selected = 0usize;
    let mut server_selected = 0usize;
    let mut keybind_selected = 0usize;
    let mut capturing = false;
    let mut status = String::new();

    loop {
        let server_label = if settings.server.trim().is_empty() {
            "Single-player".to_string()
        } else {
            settings.server.clone()
        };
        match screen {
            Screen::Menu => render(&menu_lines(
                menu_selected,
                &status,
                &settings.plane,
                &server_label,
                settings.fullscreen,
            )),
            Screen::Planes => render(&plane_lines(&settings.plane, plane_selected, &status)),
            Screen::Servers => render(&server_lines(
                &entries,
                &settings.server,
                server_selected,
                &status,
            )),
            Screen::Keybinds => render(&keybind_lines(
                &keybinds,
                keybind_selected,
                capturing,
                &status,
            )),
        }

        let key = read_key();
        if matches!(key, Key::CtrlC) {
            break;
        }

        match screen {
            Screen::Menu => match key {
                Key::Up => menu_selected = menu_selected.saturating_sub(1),
                Key::Down => menu_selected = (menu_selected + 1).min(6),
                Key::Char('q') | Key::Char('Q') => break,
                Key::Enter => match menu_selected {
                    0 => {
                        launch_game(&mut terminal, &keybinds, &settings.plane, &settings.server);
                        status.clear();
                    }
                    1 => {
                        plane_selected = planes::PLANES
                            .iter()
                            .position(|plane| plane.id == settings.plane)
                            .unwrap_or(0);
                        screen = Screen::Planes;
                        status = "Pick an aircraft.".to_string();
                    }
                    2 => {
                        server_selected = entries
                            .iter()
                            .position(|(_, address)| address == &settings.server)
                            .unwrap_or(0);
                        screen = Screen::Servers;
                        status = "Pick a server, or single-player.".to_string();
                    }
                    3 => {
                        settings.fullscreen = !settings.fullscreen;
                        let _ = settings.save();
                        status = format!(
                            "Display set to {}.",
                            if settings.fullscreen {
                                "Fullscreen"
                            } else {
                                "Windowed"
                            }
                        );
                    }
                    4 => {
                        screen = Screen::Keybinds;
                        status = "Changes are saved automatically.".to_string();
                    }
                    5 => {
                        keybinds = Keybinds::default();
                        let _ = keybinds.save();
                        status = "Reset all keybinds to defaults.".to_string();
                    }
                    _ => break,
                },
                _ => {}
            },
            Screen::Planes => match key {
                Key::Up => plane_selected = plane_selected.saturating_sub(1),
                Key::Down => plane_selected = (plane_selected + 1).min(planes::PLANES.len() - 1),
                Key::Enter => {
                    let chosen = planes::PLANES[plane_selected].id;
                    settings.plane = chosen.to_string();
                    let _ = settings.save();
                    status = format!("Selected {chosen}.");
                    screen = Screen::Menu;
                }
                Key::Esc | Key::Char('q') | Key::Char('Q') => {
                    screen = Screen::Menu;
                    status.clear();
                }
                _ => {}
            },
            Screen::Servers => match key {
                Key::Up => server_selected = server_selected.saturating_sub(1),
                Key::Down => {
                    server_selected = (server_selected + 1).min(entries.len().saturating_sub(1))
                }
                Key::Enter => {
                    let (name, address) = &entries[server_selected];
                    settings.server = address.clone();
                    let _ = settings.save();
                    status = if address.is_empty() {
                        "Selected single-player.".to_string()
                    } else {
                        format!("Selected {name} ({address}).")
                    };
                    screen = Screen::Menu;
                }
                Key::Esc | Key::Char('q') | Key::Char('Q') => {
                    screen = Screen::Menu;
                    status.clear();
                }
                _ => {}
            },
            Screen::Keybinds => {
                if capturing {
                    match key {
                        Key::Esc => {
                            capturing = false;
                            status = "Cancelled.".to_string();
                        }
                        _ => {
                            if let Some(name) = key_name(&key) {
                                keybinds.set(keybind_selected, name);
                                let _ = keybinds.save();
                                status =
                                    format!("Bound {} to {name}.", ACTIONS[keybind_selected].label);
                            } else {
                                status = "That key can't be bound. Try a letter, digit or arrow."
                                    .to_string();
                            }
                            capturing = false;
                        }
                    }
                } else {
                    match key {
                        Key::Up => keybind_selected = keybind_selected.saturating_sub(1),
                        Key::Down => {
                            keybind_selected = (keybind_selected + 1).min(ACTIONS.len() - 1)
                        }
                        Key::Enter => {
                            capturing = true;
                            status =
                                format!("Press a key for {}...", ACTIONS[keybind_selected].label);
                        }
                        Key::Backspace => {
                            let default = ACTIONS[keybind_selected].default_key;
                            keybinds.set(keybind_selected, default);
                            let _ = keybinds.save();
                            status =
                                format!("Reset {} to {default}.", ACTIONS[keybind_selected].label);
                        }
                        Key::Esc | Key::Char('q') | Key::Char('Q') => {
                            screen = Screen::Menu;
                            status.clear();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    drop(terminal);
    Ok(())
}
