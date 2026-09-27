use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
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
    Finished(u64, Result<(Action, Instant), String>),
    Opened(Result<(), String>),
}

struct Active {
    id: u64,
    snapshot: Snapshot,
    start: Sender<()>,
    cancel: Arc<AtomicBool>,
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

    fn begin(&mut self, id: u64, snapshot: Snapshot, start: Sender<()>) -> Arc<AtomicBool> {
        debug_assert!(self.active.is_none());
        let cancel = Arc::new(AtomicBool::new(false));
        self.active = Some(Active {
            id,
            snapshot,
            start,
            cancel: cancel.clone(),
        });
        cancel
    }

    fn toggle_pause(&mut self, now: Instant) -> bool {
        if self.active.is_some() {
            return false;
        }
        if self.timer.is_paused() {
            self.timer.resume(now)
        } else {
            self.timer.pause(now)
        }
    }

    fn reload(&mut self, path: &Path, now: Instant) -> Result<(), String> {
        let replacement = config::load(path).map_err(|error| error.to_string())?;
        self.timer
            .reload(replacement.interval, replacement.display, now);
        self.config = replacement;
        Ok(())
    }

    fn status_text(&self, now: Instant) -> String {
        if self.active.is_some() {
            return "Break in progress".into();
        }
        if self.timer.is_paused() {
            return "Paused".into();
        }
        let Some(deadline) = self.timer.deadline() else {
            return "Break in progress".into();
        };
        let remaining = deadline.saturating_duration_since(now);
        let minutes = remaining.as_nanos().div_ceil(60_000_000_000);
        if minutes == 0 {
            "Next break now".into()
        } else if minutes == 1 {
            "Next break in 1 min".into()
        } else {
            format!("Next break in {minutes} min")
        }
    }

    fn next_status_refresh(&self, now: Instant) -> Option<Instant> {
        let deadline = self.timer.deadline()?;
        let minutes = deadline
            .saturating_duration_since(now)
            .as_nanos()
            .div_ceil(60_000_000_000);
        if minutes == 0 {
            return Some(deadline);
        }
        Some(deadline - Duration::from_secs((minutes as u64 - 1) * 60))
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

    fn finish(&mut self, id: u64, action: Result<(Action, Instant), String>, now: Instant) -> bool {
        let Some(active) = self.active.as_ref().filter(|active| active.id == id) else {
            return false;
        };
        let (completion, at) = match action {
            Ok((Action::Elapsed, at)) => (Completion::Elapsed, at),
            Ok((Action::Skip, at)) => (Completion::Skip, at),
            Ok((Action::Postpone(index), at)) => {
                match active.snapshot.postpone_minutes.get(index) {
                    Some(minutes) => (
                        Completion::Postpone(Duration::from_secs(u64::from(*minutes) * 60)),
                        at,
                    ),
                    None => {
                        eprintln!("overlay {id}: POSTPONE index is out of range");
                        (Completion::Failed, now)
                    }
                }
            }
            Err(error) => {
                eprintln!("overlay {id}: {error}");
                (Completion::Failed, now)
            }
        };
        if !self.timer.complete(id, completion, at) {
            eprintln!("overlay {id}: outcome arrived before its display deadline");
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
    config_path: PathBuf,
    tray: Option<TrayIcon>,
    menu: Option<TrayMenu>,
    quitting: bool,
    error: Option<String>,
}

struct TrayMenu {
    status: MenuItem,
    pause: MenuItem,
    note: MenuItem,
}

impl TrayMenu {
    fn new() -> Result<(Self, Menu), String> {
        let status = MenuItem::with_id("status", "Next break", false, None);
        let pause = MenuItem::with_id("pause", "Pause", true, None);
        let reload = MenuItem::with_id("reload", "Reload Config", true, None);
        let open = MenuItem::with_id("open", "Open Config", true, None);
        let note = MenuItem::with_id("note", "Settings loaded", false, None);
        let quit = MenuItem::with_id("quit", "Quit Break Reminder", true, None);
        let menu = Menu::with_items(&[
            &status,
            &pause,
            &PredefinedMenuItem::separator(),
            &reload,
            &open,
            &note,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|error| error.to_string())?;
        Ok((
            Self {
                status,
                pause,
                note,
            },
            menu,
        ))
    }
}

impl App {
    fn initialize_tray(&mut self) -> Result<(), String> {
        let (items, menu) = TrayMenu::new()?;
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
        self.menu = Some(items);
        self.refresh_menu(Instant::now());
        Ok(())
    }

    fn refresh_menu(&self, now: Instant) {
        let Some(menu) = &self.menu else {
            return;
        };
        let status = if self.quitting {
            "Closing reminder...".into()
        } else {
            self.state.status_text(now)
        };
        if menu.status.text() != status {
            menu.status.set_text(status);
        }
        let pause = if self.state.timer.is_paused() {
            "Resume"
        } else {
            "Pause"
        };
        if menu.pause.text() != pause {
            menu.pause.set_text(pause);
        }
        let enabled = !self.quitting && self.state.active.is_none();
        if menu.pause.is_enabled() != enabled {
            menu.pause.set_enabled(enabled);
        }
    }

    fn feedback(&self, message: &str) {
        if let Some(menu) = &self.menu {
            menu.note.set_text(message);
        }
    }

    fn report_error(&self, prefix: &str, error: &str) {
        eprintln!("{prefix}: {error}");
        let detail = error.split_once(": ").map_or(error, |(_, detail)| detail);
        let detail = detail.replace('\n', " ");
        let short = format!("{prefix}: {}", detail.chars().take(64).collect::<String>());
        self.feedback(&short);
    }

    fn open_config(&self) {
        let proxy = self.proxy.clone();
        let path = self.config_path.clone();
        thread::spawn(move || {
            #[cfg(target_os = "macos")]
            let opener = "open";
            #[cfg(target_os = "linux")]
            let opener = "xdg-open";
            let result = Command::new(opener)
                .arg(path)
                .stdout(Stdio::null())
                .status()
                .map_err(|error| format!("cannot start {opener}: {error}"))
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("{opener} exited with {status}"))
                    }
                });
            let _ = proxy.send_event(AppEvent::Opened(result));
        });
    }

    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.quitting = true;
        if let Some(active) = &self.state.active {
            active.cancel.store(true, Ordering::Relaxed);
            self.refresh_menu(Instant::now());
        } else {
            event_loop.exit();
        }
    }

    fn launch(&mut self, id: u64) {
        let snapshot = Snapshot::from_config(&self.state.config);
        let (sender, receiver) = mpsc::channel();
        let cancel = self.state.begin(id, snapshot.clone(), sender);
        let executable = self.executable.clone();
        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = run_child(
                &executable,
                id,
                &snapshot,
                receiver,
                cancel.as_ref(),
                WorkerTimeouts::default(),
                || {
                    proxy
                        .send_event(AppEvent::Ready(id))
                        .map_err(|_| "parent event loop closed".to_owned())
                },
            );
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
            AppEvent::Menu(event) if event.id() == "quit" => self.quit(event_loop),
            AppEvent::Menu(event) if !self.quitting => {
                match event.id().0.as_str() {
                    "pause" => {
                        self.state.toggle_pause(Instant::now());
                    }
                    "reload" => match self.state.reload(&self.config_path, Instant::now()) {
                        Ok(()) => self.feedback("Settings reloaded"),
                        Err(error) => self.report_error("Reload failed", &error),
                    },
                    "open" => self.open_config(),
                    _ => {}
                }
                self.refresh_menu(Instant::now());
            }
            AppEvent::Menu(_) => {}
            AppEvent::Ready(id) => {
                if !self.quitting {
                    self.state.ready(id, Instant::now());
                }
            }
            AppEvent::Finished(id, action) => {
                if self.quitting {
                    if self
                        .state
                        .active
                        .as_ref()
                        .is_some_and(|active| active.id == id)
                    {
                        self.state.active = None;
                        event_loop.exit();
                    }
                } else {
                    self.state.finish(id, action, Instant::now());
                    self.refresh_menu(Instant::now());
                }
            }
            AppEvent::Opened(result) => match result {
                Ok(()) => self.feedback("Configuration opened"),
                Err(error) => self.report_error("Open failed", &error),
            },
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if !self.quitting
            && self.state.active.is_none()
            && let Some(Tick::LaunchOverlay(id)) = self.state.timer.tick(now)
        {
            self.launch(id);
        }
        self.refresh_menu(now);
        event_loop.set_control_flow(match self.state.next_status_refresh(now) {
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

fn read_line(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    let count = (&mut *reader)
        .take(128)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') {
        return Err("overlay closed without a complete protocol line".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
struct WorkerTimeouts {
    ready: Duration,
    action_grace: Duration,
    exit_grace: Duration,
}

impl Default for WorkerTimeouts {
    fn default() -> Self {
        Self {
            ready: Duration::from_secs(30),
            action_grace: Duration::from_secs(30),
            exit_grace: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LineEvent {
    Ready,
    Action,
    Ignored,
    Invalid,
}

struct ChildObservation {
    postpone_count: usize,
    display: Duration,
    timeouts: WorkerTimeouts,
    ready: bool,
    first: Option<(Action, Instant)>,
    fault: Option<String>,
    invalid_output: bool,
    eof: bool,
    abandoned_output: bool,
    exit_success: Option<bool>,
    deadline: Instant,
    timed_out: bool,
}

impl ChildObservation {
    fn new(
        postpone_count: usize,
        display: Duration,
        now: Instant,
        timeouts: WorkerTimeouts,
    ) -> Self {
        Self {
            postpone_count,
            display,
            timeouts,
            ready: false,
            first: None,
            fault: None,
            invalid_output: false,
            eof: false,
            abandoned_output: false,
            exit_success: None,
            deadline: now + timeouts.ready,
            timed_out: false,
        }
    }

    fn line(&mut self, line: &str, now: Instant) -> LineEvent {
        if self.first.is_some() || self.invalid_output {
            return LineEvent::Ignored;
        }
        if now >= self.deadline {
            self.timed_out = true;
            self.invalid_output = true;
            let phase = if self.ready { "action" } else { "READY" };
            self.fail(format!("overlay timed out waiting for {phase}"));
            return LineEvent::Invalid;
        }
        if !self.ready {
            if line != "READY\n" {
                self.invalid_output = true;
                self.fail("overlay did not report READY".into());
                return LineEvent::Invalid;
            }
            self.ready = true;
            self.timed_out = false;
            self.deadline = now + self.display + self.timeouts.action_grace;
            return LineEvent::Ready;
        }
        match Action::parse_line(line, self.postpone_count) {
            Ok(action) => {
                self.first = Some((action, now));
                self.timed_out = false;
                self.deadline = now + self.timeouts.exit_grace;
                LineEvent::Action
            }
            Err(error) => {
                self.invalid_output = true;
                self.fail(format!("invalid overlay output: {error}"));
                LineEvent::Invalid
            }
        }
    }

    fn fail(&mut self, error: String) {
        if self.fault.is_none() {
            self.fault = Some(error);
        }
    }

    fn eof(&mut self) {
        self.eof = true;
    }

    fn abandon_output(&mut self) {
        self.abandoned_output = true;
        self.fail("overlay stdout remained open after process exit".into());
    }

    fn exited(&mut self, success: bool) {
        self.exit_success = Some(success);
    }

    fn outcome(&self) -> Option<Result<(Action, Instant), String>> {
        if !(self.eof || self.abandoned_output) || self.exit_success.is_none() {
            return None;
        }
        Some(match self.first {
            Some(action) => Ok(action),
            None => Err(self
                .fault
                .clone()
                .unwrap_or_else(|| match self.exit_success {
                    Some(false) => "overlay process failed without an action".into(),
                    _ if !self.ready => "overlay closed before READY".into(),
                    _ => "overlay closed without an action".into(),
                })),
        })
    }

    fn timeout(&mut self, now: Instant) -> bool {
        if self.timed_out || now < self.deadline {
            return false;
        }
        self.timed_out = true;
        if self.first.is_none() {
            let phase = if self.ready { "action" } else { "READY" };
            self.fail(format!("overlay timed out waiting for {phase}"));
        }
        true
    }
}

enum PipeEvent {
    Line(String, Instant),
    ReadError(String),
    Eof,
    WriteError(String),
}

const MAX_PROTOCOL_LINES: usize = 16;

fn read_stdout(stdout: ChildStdout, events: Sender<PipeEvent>) {
    let mut stdout = BufReader::new(stdout);
    let mut lines = 0;
    loop {
        if lines == MAX_PROTOCOL_LINES {
            let _ = events.send(PipeEvent::ReadError(
                "overlay sent too many protocol lines".into(),
            ));
            break;
        }
        match read_line(&mut stdout) {
            Ok(Some(line)) => {
                lines += 1;
                if events.send(PipeEvent::Line(line, Instant::now())).is_err() {
                    return;
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = events.send(PipeEvent::ReadError(error));
                break;
            }
        }
    }
    let _ = events.send(PipeEvent::Eof);
}

fn write_stdin(
    mut stdin: ChildStdin,
    snapshot: Snapshot,
    start: Receiver<()>,
    events: Sender<PipeEvent>,
) {
    let write = (|| {
        protocol::write_snapshot(&mut stdin, &snapshot)?;
        if start.recv().is_err() {
            return Ok(());
        }
        stdin.write_all(b"START\n")?;
        stdin.flush()
    })();
    if let Err(error) = write {
        let _ = events.send(PipeEvent::WriteError(error.to_string()));
    }
}

fn run_child(
    executable: &Path,
    id: u64,
    snapshot: &Snapshot,
    start: Receiver<()>,
    cancel: &AtomicBool,
    timeouts: WorkerTimeouts,
    mut on_ready: impl FnMut() -> Result<(), String>,
) -> Result<(Action, Instant), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("overlay cancelled for Quit".into());
    }
    let mut child = Command::new(executable)
        .arg("--overlay")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot launch overlay: {error}"))?;
    let stdin = child.stdin.take().expect("piped child stdin");
    let stdout = child.stdout.take().expect("piped child stdout");
    let (events, received) = mpsc::channel();
    thread::spawn({
        let events = events.clone();
        move || read_stdout(stdout, events)
    });
    thread::spawn({
        let events = events.clone();
        let snapshot = snapshot.clone();
        move || write_stdin(stdin, snapshot, start, events)
    });
    drop(events);

    let mut observed = ChildObservation::new(
        snapshot.postpone_minutes.len(),
        Duration::from_secs(u64::from(snapshot.duration_seconds)),
        Instant::now(),
        timeouts,
    );
    let mut exited_at = None;
    let mut post_exit_drained = 0;
    loop {
        if cancel.load(Ordering::Relaxed) && observed.exit_success.is_none() {
            let _ = child.kill();
        }
        if observed.exit_success.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        eprintln!("overlay {id} exited with {status}");
                    }
                    observed.exited(status.success());
                    exited_at = Some(Instant::now());
                }
                Ok(None) => {}
                Err(error) => observed.fail(format!("cannot inspect overlay process: {error}")),
            }
        }
        if cancel.load(Ordering::Relaxed) && observed.exit_success.is_some() {
            return Err("overlay cancelled for Quit".into());
        }
        if let Some(result) = observed.outcome() {
            return result;
        }
        if observed.timeout(Instant::now()) {
            eprintln!(
                "overlay {id}: {}",
                observed
                    .fault
                    .as_deref()
                    .unwrap_or("did not exit after its action")
            );
            if observed.exit_success.is_none() {
                let _ = child.kill();
            }
        }
        let wait = if observed.timed_out {
            Duration::from_millis(50)
        } else {
            observed
                .deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50))
        };
        if observed.eof {
            thread::sleep(wait);
            continue;
        }
        let next = if exited_at.is_some_and(|at| Instant::now() >= at + timeouts.exit_grace) {
            match (post_exit_drained < MAX_PROTOCOL_LINES)
                .then(|| received.try_recv().ok())
                .flatten()
            {
                Some(event) => {
                    post_exit_drained += 1;
                    Ok(event)
                }
                None => {
                    eprintln!("overlay {id}: stdout remained open after process exit");
                    // ponytail: only a descendant can hold this pipe; its reader thread ends
                    // when that descendant closes stdout. Use cancellable I/O if children fork.
                    observed.abandon_output();
                    continue;
                }
            }
        } else {
            received.recv_timeout(wait)
        };
        match next {
            Ok(PipeEvent::Line(line, at)) => match observed.line(&line, at) {
                LineEvent::Ready => {
                    if let Err(error) = on_ready() {
                        observed.fail(error);
                        let _ = child.kill();
                    }
                }
                LineEvent::Invalid => {
                    let _ = child.kill();
                }
                LineEvent::Action | LineEvent::Ignored => {}
            },
            Ok(PipeEvent::ReadError(error)) => {
                observed.fail(error);
                let _ = child.kill();
            }
            Ok(PipeEvent::Eof) => {
                observed.eof();
                if observed.first.is_none() && observed.exit_success.is_none() {
                    let _ = child.kill();
                }
            }
            Ok(PipeEvent::WriteError(error)) => {
                observed.fail(format!("cannot write to overlay: {error}"));
                let _ = child.kill();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                observed.fail("overlay pipes disconnected unexpectedly".into());
                observed.eof();
                let _ = child.kill();
            }
        }
    }
}

pub fn run(config_path: &Path) -> Result<(), String> {
    let config_path = std::path::absolute(config_path).map_err(|error| error.to_string())?;
    let config = config::load(&config_path).map_err(|error| error.to_string())?;
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
        config_path,
        tray: None,
        menu: None,
        quitting: false,
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
        fs,
        io::Cursor,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config,
        protocol::{Action, Snapshot},
        timing::Tick,
    };

    use super::{AppState, ChildObservation, LineEvent, WorkerTimeouts, read_line, run_child};

    #[test]
    fn protocol_lines_are_bounded_and_newline_delimited() {
        let mut input = Cursor::new(b"READY\nSKIP\n");
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("READY\n"));
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("SKIP\n"));
        assert_eq!(read_line(&mut input).unwrap(), None);
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
        assert!(app.finish(
            id,
            Ok((Action::Postpone(1), start + Duration::from_secs(65))),
            start + Duration::from_secs(70),
        ));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(965)));
    }

    #[test]
    fn menu_status_and_pause_follow_the_remaining_interval() {
        let mut config = config::load("assets/default-config.yaml".as_ref()).unwrap();
        config.interval = Duration::from_secs(120);
        let start = Instant::now();
        let mut app = AppState::new(config, start);
        assert_eq!(app.status_text(start), "Next break in 2 min");
        assert_eq!(
            app.next_status_refresh(start),
            Some(start + Duration::from_secs(60))
        );
        assert_eq!(
            app.status_text(start + Duration::from_secs(60)),
            "Next break in 1 min"
        );

        assert!(app.toggle_pause(start + Duration::from_secs(70)));
        assert_eq!(app.status_text(start + Duration::from_secs(90)), "Paused");
        assert_eq!(
            app.next_status_refresh(start + Duration::from_secs(90)),
            None
        );
        assert!(app.toggle_pause(start + Duration::from_secs(500)));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(550)));
        let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(550)) else {
            panic!("break was not due");
        };
        let (sender, _receiver) = mpsc::channel();
        app.begin(id, Snapshot::from_config(&app.config), sender);
        assert_eq!(
            app.status_text(start + Duration::from_secs(550)),
            "Break in progress"
        );
        assert!(!app.toggle_pause(start + Duration::from_secs(551)));
    }

    #[test]
    fn reload_replaces_valid_config_but_keeps_invalid_settings_and_active_snapshot() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "break-reminder-reload-{}-{suffix}.yaml",
            std::process::id()
        ));
        fs::write(&path, "interval_minutes: 2\npostpone_minutes: [10, 15]\n").unwrap();
        let start = Instant::now();
        let config = config::load(&path).unwrap();
        let mut app = AppState::new(config, start);

        fs::write(&path, "interval_minutes: 0\n").unwrap();
        assert!(app.reload(&path, start + Duration::from_secs(30)).is_err());
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(120)));
        assert_eq!(app.config.postpone[1], Duration::from_secs(900));

        fs::write(&path, "interval_minutes: 1\npostpone_minutes: [5, 20]\n").unwrap();
        app.reload(&path, start + Duration::from_secs(30)).unwrap();
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(90)));
        let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(90)) else {
            panic!("break was not due");
        };
        let (sender, receiver) = mpsc::channel();
        app.begin(id, Snapshot::from_config(&app.config), sender);
        fs::write(&path, "interval_minutes: 3\npostpone_minutes: [2, 7]\n").unwrap();
        app.reload(&path, start + Duration::from_secs(91)).unwrap();
        assert_eq!(app.config.postpone[1], Duration::from_secs(420));
        assert!(app.ready(id, start + Duration::from_secs(92)));
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(app.finish(
            id,
            Ok((Action::Postpone(1), start + Duration::from_secs(93))),
            start + Duration::from_secs(93)
        ));
        assert_eq!(
            app.timer.deadline(),
            Some(start + Duration::from_secs(1293))
        );
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_a_running_child_without_waiting_for_its_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "break-reminder-cancel-{}-{suffix}",
            std::process::id()
        ));
        fs::write(&path, "#!/bin/sh\nexec /bin/sleep 5\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let snapshot =
            Snapshot::from_config(&config::load("assets/default-config.yaml".as_ref()).unwrap());
        let (_, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Relaxed);
        });
        let begin = Instant::now();
        let result = run_child(
            &path,
            1,
            &snapshot,
            receiver,
            cancel.as_ref(),
            WorkerTimeouts::default(),
            || Ok(()),
        );
        fs::remove_file(path).unwrap();
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(begin.elapsed() < Duration::from_secs(3));
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
            assert!(app.finish(
                id,
                Ok((action, start + Duration::from_secs(action_at))),
                start + Duration::from_secs(action_at),
            ));
            assert_eq!(
                app.timer.deadline(),
                Some(start + Duration::from_secs(next_due))
            );
        }
    }

    #[test]
    fn exit_before_buffered_action_waits_for_eof_and_preserves_first_choice() {
        let at = Instant::now();
        let mut child =
            ChildObservation::new(2, Duration::from_secs(30), at, WorkerTimeouts::default());
        child.exited(true);
        assert!(child.outcome().is_none());
        assert_eq!(child.line("READY\n", at), LineEvent::Ready);
        assert_eq!(child.line("POSTPONE 1\n", at), LineEvent::Action);
        assert_eq!(child.line("SKIP\n", at), LineEvent::Ignored);
        assert!(child.outcome().is_none());
        child.eof();
        assert_eq!(child.outcome(), Some(Ok((Action::Postpone(1), at))));
    }

    #[test]
    fn early_elapsed_falls_back_once_and_stale_events_do_not_move_deadline() {
        let mut config = config::load("assets/default-config.yaml".as_ref()).unwrap();
        config.interval = Duration::from_secs(60);
        config.display = Duration::from_secs(30);
        let start = Instant::now();
        let mut app = AppState::new(config, start);
        let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(60)) else {
            panic!("break was not due");
        };
        let (sender, _receiver) = mpsc::channel();
        app.begin(id, Snapshot::from_config(&app.config), sender);
        assert!(app.ready(id, start + Duration::from_secs(61)));
        assert!(app.finish(
            id,
            Ok((Action::Elapsed, start + Duration::from_secs(62))),
            start + Duration::from_secs(63),
        ));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(123)));
        assert!(!app.finish(
            id,
            Ok((Action::Postpone(0), start + Duration::from_secs(64))),
            start + Duration::from_secs(65),
        ));
        assert!(!app.ready(id, start + Duration::from_secs(66)));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(123)));
    }

    #[test]
    fn missing_ready_and_malformed_output_resolve_after_eof_and_exit() {
        let at = Instant::now();
        let mut child =
            ChildObservation::new(2, Duration::from_secs(30), at, WorkerTimeouts::default());
        assert_eq!(child.line("SKIP\n", at), LineEvent::Invalid);
        child.eof();
        assert!(child.outcome().is_none());
        child.exited(true);
        assert!(child.outcome().unwrap().is_err());

        let mut child =
            ChildObservation::new(2, Duration::from_secs(30), at, WorkerTimeouts::default());
        assert_eq!(child.line("READY\n", at), LineEvent::Ready);
        assert_eq!(child.line("POSTPONE 9\n", at), LineEvent::Invalid);
        child.eof();
        child.exited(true);
        assert!(child.outcome().unwrap().is_err());

        let mut child =
            ChildObservation::new(2, Duration::from_secs(30), at, WorkerTimeouts::default());
        assert_eq!(child.line("READY\n", at), LineEvent::Ready);
        child.eof();
        child.exited(true);
        assert_eq!(
            child.outcome(),
            Some(Err("overlay closed without an action".into()))
        );

        let mut child =
            ChildObservation::new(2, Duration::from_secs(30), at, WorkerTimeouts::default());
        assert_eq!(child.line("READY\n", at), LineEvent::Ready);
        child.eof();
        child.exited(false);
        assert_eq!(
            child.outcome(),
            Some(Err("overlay process failed without an action".into()))
        );
    }

    #[test]
    fn missing_ready_action_and_post_action_timeouts_are_bounded() {
        let at = Instant::now();
        let timeouts = WorkerTimeouts {
            ready: Duration::from_secs(2),
            action_grace: Duration::from_secs(5),
            exit_grace: Duration::from_secs(3),
        };
        let mut missing_ready = ChildObservation::new(2, Duration::from_secs(30), at, timeouts);
        assert!(!missing_ready.timeout(at + Duration::from_secs(1)));
        assert!(missing_ready.timeout(at + Duration::from_secs(2)));
        assert!(!missing_ready.timeout(at + Duration::from_secs(3)));
        missing_ready.eof();
        missing_ready.exited(false);
        assert!(missing_ready.outcome().unwrap().is_err());

        let mut no_action = ChildObservation::new(2, Duration::from_secs(30), at, timeouts);
        assert_eq!(no_action.line("READY\n", at), LineEvent::Ready);
        assert!(!no_action.timeout(at + Duration::from_secs(34)));
        assert!(no_action.timeout(at + Duration::from_secs(35)));

        let mut no_exit = ChildObservation::new(2, Duration::from_secs(30), at, timeouts);
        assert_eq!(no_exit.line("READY\n", at), LineEvent::Ready);
        assert_eq!(no_exit.line("SKIP\n", at), LineEvent::Action);
        assert!(!no_exit.timeout(at + Duration::from_secs(2)));
        assert!(no_exit.timeout(at + Duration::from_secs(3)));
        no_exit.eof();
        no_exit.exited(false);
        assert_eq!(no_exit.outcome(), Some(Ok((Action::Skip, at))));
    }

    #[test]
    fn late_action_is_rejected_but_predeadline_buffered_action_wins() {
        let at = Instant::now();
        let timeouts = WorkerTimeouts {
            ready: Duration::from_secs(2),
            action_grace: Duration::from_secs(5),
            exit_grace: Duration::from_secs(3),
        };
        let mut late = ChildObservation::new(2, Duration::from_secs(30), at, timeouts);
        assert_eq!(late.line("READY\n", at), LineEvent::Ready);
        assert_eq!(
            late.line("SKIP\n", at + Duration::from_secs(35)),
            LineEvent::Invalid
        );
        late.eof();
        late.exited(true);
        assert!(late.outcome().unwrap().is_err());

        let mut buffered = ChildObservation::new(2, Duration::from_secs(30), at, timeouts);
        assert_eq!(buffered.line("READY\n", at), LineEvent::Ready);
        assert!(buffered.timeout(at + Duration::from_secs(35)));
        assert_eq!(
            buffered.line("POSTPONE 1\n", at + Duration::from_secs(34)),
            LineEvent::Action
        );
        buffered.eof();
        buffered.exited(false);
        assert_eq!(
            buffered.outcome(),
            Some(Ok((Action::Postpone(1), at + Duration::from_secs(34))))
        );
    }

    #[test]
    fn spawn_failure_schedules_one_full_interval() {
        let mut config = config::load("assets/default-config.yaml".as_ref()).unwrap();
        config.interval = Duration::from_secs(60);
        let start = Instant::now();
        let mut app = AppState::new(config, start);
        let Some(Tick::LaunchOverlay(id)) = app.timer.tick(start + Duration::from_secs(60)) else {
            panic!("break was not due");
        };
        let (sender, _receiver) = mpsc::channel();
        app.begin(id, Snapshot::from_config(&app.config), sender);
        assert!(app.finish(
            id,
            Err("cannot launch overlay".into()),
            start + Duration::from_secs(61)
        ));
        assert_eq!(app.timer.deadline(), Some(start + Duration::from_secs(121)));
        assert_eq!(app.timer.tick(start + Duration::from_secs(62)), None);
    }

    #[cfg(unix)]
    #[test]
    fn fake_child_failures_are_bounded_and_reaped() {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "break-reminder-hung-child-{}-{suffix}",
            std::process::id()
        ));
        fs::write(&path, "#!/bin/sh\nprintf 'BROKEN\\n'\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let snapshot =
            Snapshot::from_config(&config::load("assets/default-config.yaml".as_ref()).unwrap());
        let (start, receiver) = mpsc::channel();
        drop(start);
        assert!(
            run_child(
                &path,
                1,
                &snapshot,
                receiver,
                &AtomicBool::new(false),
                WorkerTimeouts::default(),
                || Ok(()),
            )
            .is_err()
        );

        fs::write(
            &path,
            "#!/bin/sh\nprintf 'READY\\nPOSTPONE 1\\n'\ni=0\nwhile [ \"$i\" -lt 1000 ]; do printf 'SKIP\\n'; i=$((i+1)); done\n",
        )
        .unwrap();
        let (start, receiver) = mpsc::channel();
        let outcome = run_child(
            &path,
            1,
            &snapshot,
            receiver,
            &AtomicBool::new(false),
            WorkerTimeouts::default(),
            || start.send(()).map_err(|error| error.to_string()),
        );
        assert!(matches!(outcome, Ok((Action::Postpone(1), _))));

        fs::write(&path, "#!/bin/sh\nexec /bin/sleep 5\n").unwrap();
        let (start, receiver) = mpsc::channel();
        drop(start);
        let begin = Instant::now();
        let outcome = run_child(
            &path,
            1,
            &snapshot,
            receiver,
            &AtomicBool::new(false),
            WorkerTimeouts {
                ready: Duration::from_millis(200),
                action_grace: Duration::from_millis(200),
                exit_grace: Duration::from_millis(200),
            },
            || Ok(()),
        );
        assert!(outcome.unwrap_err().contains("READY"));
        assert!(begin.elapsed() < Duration::from_secs(3));

        fs::write(
            &path,
            "#!/bin/sh\nprintf 'READY\\n'\n/bin/sleep 3 &\n/bin/sleep 1\n",
        )
        .unwrap();
        let (start, receiver) = mpsc::channel();
        let begin = Instant::now();
        let outcome = run_child(
            &path,
            2,
            &snapshot,
            receiver,
            &AtomicBool::new(false),
            WorkerTimeouts {
                ready: Duration::from_secs(2),
                action_grace: Duration::from_secs(5),
                exit_grace: Duration::from_millis(200),
            },
            || start.send(()).map_err(|error| error.to_string()),
        );
        fs::remove_file(&path).unwrap();
        assert!(outcome.unwrap_err().contains("stdout remained open"));
        assert!(begin.elapsed() < Duration::from_secs(2));
    }
}
