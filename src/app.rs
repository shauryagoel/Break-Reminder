use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::WindowId,
};

use crate::{
    config::{self, Config},
    protocol::{self, Action, Snapshot},
    timing::{Completion, Tick, Timer},
};

enum AppEvent {
    Menu(MenuEvent),
    Ready(u64),
    Finished(u64, Result<Action, String>),
}

struct Active {
    id: u64,
    snapshot: Snapshot,
    start: Sender<()>,
}

struct AppState {
    config: Config,
    timer: Timer,
    active: Option<Active>,
}

impl AppState {
    fn new(config: Config, now: Instant) -> Self {
        let timer = Timer::new(config.interval, config.display, now);
        Self {
            config,
            timer,
            active: None,
        }
    }

    fn begin(&mut self, id: u64, snapshot: Snapshot, start: Sender<()>) {
        debug_assert!(self.active.is_none());
        self.active = Some(Active {
            id,
            snapshot,
            start,
        });
    }

    fn ready(&mut self, id: u64, now: Instant) -> bool {
        let Some(active) = self.active.as_ref().filter(|active| active.id == id) else {
            return false;
        };
        if !self.timer.visible(id, now) {
            return false;
        }
        active.start.send(()).is_ok()
    }

    fn finish(&mut self, id: u64, action: Result<Action, String>, now: Instant) -> bool {
        let Some(active) = self.active.as_ref().filter(|active| active.id == id) else {
            return false;
        };
        let completion = match action {
            Ok(Action::Elapsed) => Completion::Elapsed,
            Ok(Action::Skip) => Completion::Skip,
            Ok(Action::Postpone(index)) => active
                .snapshot
                .postpone_minutes
                .get(index)
                .map(|minutes| Completion::Postpone(Duration::from_secs(u64::from(*minutes) * 60)))
                .unwrap_or(Completion::Failed),
            Err(error) => {
                eprintln!("overlay {id}: {error}");
                Completion::Failed
            }
        };
        if !self.timer.complete(id, completion, now) {
            self.timer.complete(id, Completion::Failed, now);
        }
        self.active = None;
        true
    }
}

struct App {
    state: AppState,
    proxy: EventLoopProxy<AppEvent>,
    executable: PathBuf,
    tray: Option<TrayIcon>,
    error: Option<String>,
}

impl App {
    fn initialize_tray(&mut self) -> Result<(), String> {
        let quit = MenuItem::with_id("quit", "Quit Break Reminder", true, None);
        let menu = Menu::with_items(&[&quit]).map_err(|error| error.to_string())?;
        let icon = tray_icon()?;
        self.tray = Some(
            TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Break Reminder")
                .with_icon(icon)
                .with_icon_as_template(cfg!(target_os = "macos"))
                .build()
                .map_err(|error| format!("cannot create tray icon: {error}"))?,
        );
        Ok(())
    }

    fn launch(&mut self, id: u64) {
        let snapshot = Snapshot::from_config(&self.state.config);
        let (sender, receiver) = mpsc::channel();
        self.state.begin(id, snapshot.clone(), sender);
        let executable = self.executable.clone();
        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = run_child(&executable, id, &snapshot, &proxy, receiver);
            let _ = proxy.send_event(AppEvent::Finished(id, result));
        });
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init)
            && let Err(error) = self.initialize_tray()
        {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Menu(event) if event.id() == "quit" => event_loop.exit(),
            AppEvent::Menu(_) => {}
            AppEvent::Ready(id) => {
                self.state.ready(id, Instant::now());
            }
            AppEvent::Finished(id, action) => {
                self.state.finish(id, action, Instant::now());
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(Tick::LaunchOverlay(id)) = self.state.timer.tick(Instant::now()) {
            self.launch(id);
        }
        event_loop.set_control_flow(match self.state.timer.deadline() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        });
    }
}

fn tray_icon() -> Result<Icon, String> {
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            let dx = x - 16;
            let dy = y - 16;
            let radius = dx * dx + dy * dy;
            let ring = (69..=144).contains(&radius);
            let hands = (x == 16 && (9..=17).contains(&y)) || (y == 16 && (16..=22).contains(&x));
            rgba.extend_from_slice(&[24, 143, 118, if ring || hands { 255 } else { 0 }]);
        }
    }
    Icon::from_rgba(rgba, 32, 32).map_err(|error| error.to_string())
}

fn read_line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut bytes = Vec::new();
    (&mut *reader)
        .take(128)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.last() != Some(&b'\n') {
        return Err("overlay closed without a complete protocol line".into());
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn run_child(
    executable: &Path,
    id: u64,
    snapshot: &Snapshot,
    proxy: &EventLoopProxy<AppEvent>,
    start: Receiver<()>,
) -> Result<Action, String> {
    let mut child = Command::new(executable)
        .arg("--overlay")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot launch overlay: {error}"))?;
    let result = (|| {
        let mut stdin = child.stdin.take().ok_or("overlay stdin is unavailable")?;
        let stdout = child.stdout.take().ok_or("overlay stdout is unavailable")?;
        let mut stdout = BufReader::new(stdout);
        protocol::write_snapshot(&mut stdin, snapshot).map_err(|error| error.to_string())?;
        if read_line(&mut stdout)? != "READY\n" {
            return Err("overlay did not report READY".into());
        }
        proxy
            .send_event(AppEvent::Ready(id))
            .map_err(|_| "parent event loop closed".to_owned())?;
        start
            .recv()
            .map_err(|_| "parent closed before START".to_owned())?;
        stdin
            .write_all(b"START\n")
            .map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())?;
        drop(stdin);
        let action = Action::parse_line(&read_line(&mut stdout)?, snapshot.postpone_minutes.len())?;
        io::copy(&mut stdout, &mut io::sink()).map_err(|error| error.to_string())?;
        Ok(action)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if !status.success() {
        eprintln!("overlay {id} exited with {status}");
    }
    result
}

pub fn run(config_path: &Path) -> Result<(), String> {
    let config = config::load(config_path).map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut builder = EventLoop::<AppEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false)
            .with_activate_ignoring_other_apps(false);
    }
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = builder.build().map_err(|error| error.to_string())?;
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(AppEvent::Menu(event));
        }
    }));
    let mut app = App {
        state: AppState::new(config, Instant::now()),
        proxy,
        executable,
        tray: None,
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use crate::{
        config,
        protocol::{Action, Snapshot},
        timing::Tick,
    };

    use super::{AppState, read_line};

    #[test]
    fn protocol_lines_are_bounded_and_newline_delimited() {
        let mut input = Cursor::new(b"READY\nSKIP\n");
        assert_eq!(read_line(&mut input).unwrap(), "READY\n");
        assert_eq!(read_line(&mut input).unwrap(), "SKIP\n");
        assert!(read_line(&mut Cursor::new(b"READY")).is_err());
        assert!(read_line(&mut Cursor::new(vec![b'x'; 128])).is_err());
    }

    #[test]
    fn ready_starts_child_and_postpone_uses_launch_snapshot() {
        let mut config = config::load("assets/default-config.yaml".as_ref()).unwrap();
        config.interval = Duration::from_secs(60);
        config.display = Duration::from_secs(30);
        let start = Instant::now();
        let mut app = AppState::new(config, start);
        let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(60)) else {
            panic!("break was not due");
        };
        let snapshot = Snapshot::from_config(&app.config);
        let (sender, receiver) = mpsc::channel();
        app.begin(id, snapshot, sender);

        assert!(app.ready(id, start + Duration::from_secs(63)));
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(app.finish(id, Ok(Action::Postpone(1)), start + Duration::from_secs(65)));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(965)));
    }

    #[test]
    fn skip_and_elapsed_schedule_full_intervals() {
        let mut config = config::load("assets/default-config.yaml".as_ref()).unwrap();
        config.interval = Duration::from_secs(60);
        config.display = Duration::from_secs(30);
        let start = Instant::now();
        let mut app = AppState::new(config, start);
        for (due, ready, action_at, action, next_due) in [
            (60, 61, 62, Action::Skip, 122),
            (122, 123, 153, Action::Elapsed, 213),
        ] {
            let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(due))
            else {
                panic!("break was not due");
            };
            let snapshot = Snapshot::from_config(&app.config);
            let (sender, receiver) = mpsc::channel();
            app.begin(id, snapshot, sender);
            assert!(app.ready(id, start + Duration::from_secs(ready)));
            assert_eq!(receiver.try_recv(), Ok(()));
            assert!(app.finish(id, Ok(action), start + Duration::from_secs(action_at)));
            assert_eq!(
                app.timer.deadline(),
                Some(start + Duration::from_secs(next_due))
            );
        }
    }
}
