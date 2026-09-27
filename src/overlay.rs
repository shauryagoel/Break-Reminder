use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, RichText};

use crate::protocol::{self, Action, Output, Snapshot};

pub fn run() -> Result<(), String> {
    let mut stdin = io::stdin();
    let snapshot = protocol::read_snapshot(&mut stdin)
        .map_err(|error| format!("invalid overlay settings: {error}"))?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Break reminder")
            .with_inner_size([900.0, 600.0])
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "break-reminder-overlay",
        options,
        Box::new(move |creation| {
            let context = creation.egui_ctx.clone();
            let (sender, receiver) = mpsc::channel();
            thread::spawn(move || {
                let result = protocol::read_start(&mut stdin).map_err(|error| error.to_string());
                let _ = sender.send(result);
                context.request_repaint();
            });
            Ok(Box::new(Overlay::new(snapshot, receiver)))
        }),
    )
    .map_err(|error| format!("cannot open reminder: {error}"))
}

struct Overlay {
    snapshot: Snapshot,
    start: Receiver<Result<(), String>>,
    deadline: Option<Instant>,
    output: Output<io::Stdout>,
    first_frame: Option<u64>,
    ready: bool,
    finished: bool,
}

impl Overlay {
    fn new(snapshot: Snapshot, start: Receiver<Result<(), String>>) -> Self {
        Self {
            snapshot,
            start,
            deadline: None,
            output: Output::new(io::stdout()),
            first_frame: None,
            ready: false,
            finished: false,
        }
    }

    fn finish(&mut self, action: Action, context: &egui::Context) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Err(error) = self.output.terminal(action) {
            eprintln!("cannot send reminder action: {error}");
        }
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn color(hex: &str) -> Color32 {
        Color32::from_rgb(
            u8::from_str_radix(&hex[1..3], 16).expect("validated color"),
            u8::from_str_radix(&hex[3..5], 16).expect("validated color"),
            u8::from_str_radix(&hex[5..7], 16).expect("validated color"),
        )
    }
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        if self.ready && self.deadline.is_none() {
            match self.start.try_recv() {
                Ok(Ok(())) => {
                    self.deadline = Some(
                        Instant::now()
                            + Duration::from_secs(u64::from(self.snapshot.duration_seconds)),
                    );
                }
                Ok(Err(error)) => {
                    eprintln!("invalid overlay control: {error}");
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    eprintln!("overlay control pipe closed before START");
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let now = Instant::now();
        let background = Self::color(&self.snapshot.background_color);
        let foreground = Self::color(&self.snapshot.text_color);
        let accent = Self::color(&self.snapshot.accent_color);
        let mut choice = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(background))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space((ui.available_height() - 310.0).max(24.0) / 2.0);
                ui.vertical_centered(|ui| {
                    let seconds = self
                        .deadline
                        .map(|deadline| {
                            deadline
                                .saturating_duration_since(now)
                                .as_millis()
                                .div_ceil(1000)
                        })
                        .unwrap_or(u128::from(self.snapshot.duration_seconds));
                    ui.label(
                        RichText::new(format!("{:02}:{:02}", seconds / 60, seconds % 60))
                            .size(54.0)
                            .color(accent),
                    );
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new(&self.snapshot.title)
                            .size(34.0)
                            .strong()
                            .color(foreground),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(&self.snapshot.message)
                            .size(20.0)
                            .color(foreground),
                    );
                    ui.add_space(36.0);
                    ui.add_enabled_ui(self.deadline.is_some(), |ui| {
                        if ui.button("Skip break").clicked() {
                            choice = Some(Action::Skip);
                        }
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            for (index, minutes) in
                                self.snapshot.postpone_minutes.iter().enumerate()
                            {
                                if ui.button(format!("Postpone {minutes} min")).clicked() {
                                    choice = Some(Action::Postpone(index));
                                }
                            }
                        });
                    });
                });
            });

        if let Some(action) = choice {
            self.finish(action, &context);
            return;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.finish(Action::Elapsed, &context);
            return;
        }
        if self.first_frame.is_none() {
            self.first_frame = Some(context.cumulative_frame_nr());
            context.request_repaint();
        } else if !self.ready
            && self
                .first_frame
                .is_some_and(|first| context.cumulative_frame_nr() > first)
        {
            if let Err(error) = self.output.ready() {
                eprintln!("cannot send reminder readiness: {error}");
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            self.ready = true;
            context.request_repaint();
        } else if !self.ready {
            context.request_repaint();
        }
        if let Some(deadline) = self.deadline {
            context.request_repaint_after(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(250)),
            );
        }
    }
}
