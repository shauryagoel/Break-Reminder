use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, RichText, Stroke};
use winit::window::Window;

use crate::protocol::{self, Action, Fit, Output, Snapshot};

mod image;

#[derive(Clone, Copy)]
struct Screen {
    #[cfg(target_os = "linux")]
    position: [f32; 2],
    size: [f32; 2],
    #[cfg(target_os = "macos")]
    frame: objc2_foundation::NSRect,
    #[cfg(target_os = "linux")]
    physical_position: (i32, i32),
    #[cfg(target_os = "linux")]
    physical_size: (u32, u32),
}

#[derive(Debug, PartialEq, Eq)]
enum Readiness {
    Ready,
    Retry,
    DropPending,
    Fail,
}

fn readiness(root_ready: bool, pending: bool, exhausted: bool) -> Readiness {
    if !root_ready {
        if exhausted {
            Readiness::Fail
        } else {
            Readiness::Retry
        }
    } else if pending {
        if exhausted {
            Readiness::DropPending
        } else {
            Readiness::Retry
        }
    } else {
        Readiness::Ready
    }
}

#[cfg(any(target_os = "linux", test))]
fn physical_to_logical(origin: (i32, i32), size: (u32, u32), scale: f64) -> ([f32; 2], [f32; 2]) {
    (
        [
            origin.0 as f32 / scale as f32,
            origin.1 as f32 / scale as f32,
        ],
        [size.0 as f32 / scale as f32, size.1 as f32 / scale as f32],
    )
}

fn screens(root: &Window) -> Result<Vec<Screen>, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = root;
        crate::macos_window::screens().map(|frames| {
            frames
                .into_iter()
                .map(|frame| Screen {
                    size: [frame.size.width as f32, frame.size.height as f32],
                    frame,
                })
                .collect()
        })
    }
    #[cfg(target_os = "linux")]
    {
        let mut monitors: Vec<_> = root.available_monitors().collect();
        if let Some(primary) = root.primary_monitor()
            && let Some(index) = monitors.iter().position(|monitor| *monitor == primary)
        {
            monitors.swap(0, index);
        }
        if monitors.is_empty() {
            return Err("no X11 displays are available".into());
        }
        Ok(monitors
            .into_iter()
            .map(|monitor| {
                let origin = monitor.position();
                let size = monitor.size();
                let physical_position = (origin.x, origin.y);
                let physical_size = (size.width, size.height);
                let (position, size) =
                    physical_to_logical(physical_position, physical_size, monitor.scale_factor());
                Screen {
                    position,
                    size,
                    physical_position,
                    physical_size,
                }
            })
            .collect())
    }
}

fn place_root(root: &Window, screen: Screen) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos_window::configure_root(root, screen.frame)
    }
    #[cfg(target_os = "linux")]
    {
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        root.request_inner_size(PhysicalSize::new(
            screen.physical_size.0,
            screen.physical_size.1,
        ));
        root.set_outer_position(PhysicalPosition::new(
            screen.physical_position.0,
            screen.physical_position.1,
        ));
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn root_ready(root: &Window, screen: Screen) -> bool {
    root.outer_position().is_ok_and(|position| {
        (position.x, position.y) == screen.physical_position && {
            let size = root.inner_size();
            (size.width, size.height) == screen.physical_size
        }
    })
}

#[cfg(target_os = "linux")]
fn align_child(context: &egui::Context, screen: Screen) -> bool {
    let pixels_per_point = context.pixels_per_point();
    let (position, size) = physical_to_logical(
        screen.physical_position,
        screen.physical_size,
        f64::from(pixels_per_point),
    );
    let (outer, inner) = context.input(|input| {
        let viewport = input.viewport();
        (viewport.outer_rect, viewport.inner_rect)
    });
    let near = |actual: f32, expected: f32| ((actual - expected) * pixels_per_point).abs() <= 1.0;
    let aligned = outer
        .is_some_and(|rect| near(rect.min.x, position[0]) && near(rect.min.y, position[1]))
        && inner.is_some_and(|rect| near(rect.width(), size[0]) && near(rect.height(), size[1]));
    if !aligned {
        context.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            position[0],
            position[1],
        )));
        context.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            size[0], size[1],
        )));
    }
    aligned
}

fn child_title(index: usize) -> String {
    format!("Break reminder {} screen {index}", std::process::id())
}

fn base_viewport(size: [f32; 2], title: String) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_resizable(false)
        .with_always_on_top()
}

fn viewport(screen: Screen, title: String) -> egui::ViewportBuilder {
    let viewport = base_viewport(screen.size, title);
    #[cfg(target_os = "macos")]
    {
        viewport.with_visible(false)
    }
    #[cfg(target_os = "linux")]
    {
        viewport.with_position(screen.position)
    }
}

pub fn run() -> Result<(), String> {
    let mut stdin = io::stdin();
    let snapshot = protocol::read_snapshot(&mut stdin)
        .map_err(|error| format!("invalid overlay settings: {error}"))?;
    let mut options = eframe::NativeOptions {
        viewport: base_viewport([900.0, 600.0], "Break reminder".into()),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_activation_policy(ActivationPolicy::Accessory);
            builder.with_default_menu(false);
        }));
    }
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
    eframe::run_native(
        "break-reminder-overlay",
        options,
        Box::new(move |creation| {
            let root = creation
                .winit_window()
                .ok_or_else(|| io::Error::other("overlay root window is unavailable"))?
                .clone();
            let screens = screens(&root).map_err(io::Error::other)?;
            place_root(&root, screens[0]).map_err(io::Error::other)?;
            let context = creation.egui_ctx.clone();
            Overlay::install_visuals(&context, &snapshot);
            let (sender, receiver) = mpsc::channel();
            thread::spawn(move || {
                let result = protocol::read_start(&mut stdin).map_err(|error| error.to_string());
                let _ = sender.send(result);
                context.request_repaint();
            });
            Ok(Box::new(Overlay::new(snapshot, receiver, root, screens)))
        }),
    )
    .map_err(|error| format!("cannot open reminder: {error}"))
}

struct Overlay {
    snapshot: Snapshot,
    image: Option<(egui::TextureHandle, Fit)>,
    image_attempted: bool,
    start: Receiver<Result<(), String>>,
    deadline: Option<Instant>,
    output: Output<io::Stdout>,
    root: std::sync::Arc<Window>,
    root_screen: Screen,
    secondary: Vec<(usize, Screen)>,
    first_frame: Option<u64>,
    #[cfg(target_os = "macos")]
    readiness_retry: bool,
    #[cfg(target_os = "linux")]
    readiness_attempts: u8,
    ready: bool,
    finished: bool,
}

impl Overlay {
    fn new(
        snapshot: Snapshot,
        start: Receiver<Result<(), String>>,
        root: std::sync::Arc<Window>,
        screens: Vec<Screen>,
    ) -> Self {
        Self {
            snapshot,
            image: None,
            image_attempted: false,
            start,
            deadline: None,
            output: Output::new(io::stdout()),
            root,
            root_screen: screens[0],
            secondary: screens.into_iter().enumerate().skip(1).collect(),
            first_frame: None,
            #[cfg(target_os = "macos")]
            readiness_retry: false,
            #[cfg(target_os = "linux")]
            readiness_attempts: 0,
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
        context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
    }

    fn drop_pending(&mut self, pending: &[usize], context: &egui::Context) {
        if pending.is_empty() {
            return;
        }
        for index in pending {
            eprintln!(
                "skipping reminder display {index}: window did not stay at its display bounds"
            );
        }
        self.secondary.retain(|(index, _)| !pending.contains(index));
        // Egui closes omitted immediate viewports on the next parent pass.
        context.request_repaint();
    }

    fn color(hex: &str) -> Color32 {
        Color32::from_rgb(
            u8::from_str_radix(&hex[1..3], 16).expect("validated color"),
            u8::from_str_radix(&hex[3..5], 16).expect("validated color"),
            u8::from_str_radix(&hex[5..7], 16).expect("validated color"),
        )
    }

    fn install_visuals(context: &egui::Context, snapshot: &Snapshot) {
        let background = Self::color(&snapshot.background_color);
        let foreground = Self::color(&snapshot.text_color);
        let accent = Self::color(&snapshot.accent_color);
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(foreground);
        visuals.widgets.inactive.bg_fill = background.lerp_to_gamma(foreground, 0.10);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, foreground.gamma_multiply(0.28));
        visuals.widgets.hovered.bg_fill = background.lerp_to_gamma(accent, 0.22);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, accent);
        visuals.widgets.active.bg_fill = background.lerp_to_gamma(accent, 0.34);
        visuals.widgets.active.bg_stroke = Stroke::new(1.5, accent);
        visuals.selection.stroke = Stroke::new(1.5, accent);
        context.set_visuals(visuals);
    }

    fn image_slot(ui: &mut egui::Ui, image: Option<&(egui::TextureHandle, Fit)>) {
        let bounds = egui::vec2(ui.available_width().min(640.0), 280.0);
        let (rect, _) = ui.allocate_exact_size(bounds, egui::Sense::hover());
        if let Some((texture, fit)) = image {
            let (size, uv) = image::geometry(texture.size_vec2(), bounds, *fit);
            egui::Image::from_texture(texture)
                .fit_to_exact_size(size)
                .maintain_aspect_ratio(false)
                .uv(uv)
                .corner_radius(12)
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), size));
        }
    }

    fn focus_ring(ui: &egui::Ui, response: &egui::Response, accent: Color32) {
        if response.enabled() && ui.memory(|memory| memory.has_focus(response.id)) {
            if response.gained_focus() {
                response.scroll_to_me(None);
            }
            ui.painter().rect_stroke(
                response.rect.shrink(1.0),
                8.0,
                Stroke::new(2.0, accent),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn background_fill(snapshot: &Snapshot) -> Color32 {
        let background = Self::color(&snapshot.background_color);
        let opacity_percent = 100 - snapshot.background_transparency_percent;
        let alpha = ((opacity_percent * 255 + 50) / 100) as u8;
        Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), alpha)
    }

    fn centered_column(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        let available = ui.available_rect_before_wrap();
        let width = (available.width() - 40.0).clamp(1.0, 680.0);
        let left = available.center().x - width / 2.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, available.top()),
            egui::vec2(width, available.height()),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            add_contents,
        );
    }

    fn actions(ui: &mut egui::Ui, snapshot: &Snapshot, enabled: bool) -> Option<Action> {
        let width = ui.available_width();
        let background = Self::color(&snapshot.background_color);
        let foreground = Self::color(&snapshot.text_color);
        let accent = Self::color(&snapshot.accent_color);
        let mut choice = None;
        ui.add_enabled_ui(enabled, |ui| {
            let skip = ui.add_sized(
                [width, 48.0],
                egui::Button::new(
                    RichText::new("Skip current break")
                        .size(16.0)
                        .strong()
                        .color(foreground),
                )
                .fill(background.lerp_to_gamma(accent, 0.16))
                .stroke(Stroke::new(1.0, accent.gamma_multiply(0.65)))
                .corner_radius(8),
            );
            let focus_id = egui::Id::new(("initial-skip-focus", ui.ctx().viewport_id()));
            if enabled && !ui.data(|data| data.get_temp::<bool>(focus_id).unwrap_or(false)) {
                skip.request_focus();
                ui.data_mut(|data| data.insert_temp(focus_id, true));
            }
            Self::focus_ring(ui, &skip, accent);
            if skip.clicked()
                || (skip.has_focus()
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            || input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                    }))
            {
                choice = Some(Action::Skip);
            }
            ui.add_space(20.0);
            ui.label(
                RichText::new("OR POSTPONE")
                    .size(12.0)
                    .strong()
                    .color(accent),
            );
            ui.add_space(12.0);
            let columns = if width >= 620.0 {
                4
            } else if width >= 420.0 {
                3
            } else if width >= 260.0 {
                2
            } else {
                1
            }
            .min(snapshot.postpone_minutes.len());
            let gap = 12.0;
            let button_width = (width - gap * (columns - 1) as f32) / columns as f32;
            for (row_index, row) in snapshot.postpone_minutes.chunks(columns).enumerate() {
                let row_width = button_width * row.len() as f32 + gap * (row.len() - 1) as f32;
                ui.allocate_ui_with_layout(
                    egui::vec2(row_width, 44.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (offset, minutes) in row.iter().enumerate() {
                            let index = row_index * columns + offset;
                            let button = ui.add_sized(
                                [button_width, 44.0],
                                egui::Button::new(
                                    RichText::new(format!("{minutes} min"))
                                        .size(16.0)
                                        .color(foreground),
                                )
                                .corner_radius(8),
                            );
                            Self::focus_ring(ui, &button, accent);
                            if button.clicked() {
                                choice = Some(Action::Postpone(index));
                            }
                        }
                    },
                );
                ui.add_space(12.0);
            }
        });
        choice
    }

    fn paint(&self, ui: &mut egui::Ui, now: Instant) -> Option<Action> {
        let foreground = Self::color(&self.snapshot.text_color);
        let accent = Self::color(&self.snapshot.accent_color);
        let mut choice = None;
        if self.deadline.is_some()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            choice = Some(Action::Skip);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Self::background_fill(&self.snapshot)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(32.0);
                        Self::centered_column(ui, |ui| {
                            ui.label(
                                RichText::new("BREAK REMINDER")
                                    .size(12.0)
                                    .strong()
                                    .color(accent),
                            );
                            ui.add_space(16.0);
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
                                    .size(56.0)
                                    .color(accent),
                            );
                            ui.add_space(24.0);
                            Self::image_slot(ui, self.image.as_ref());
                            ui.add_space(24.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.snapshot.title)
                                        .size(32.0)
                                        .strong()
                                        .color(foreground),
                                )
                                .wrap(),
                            );
                            ui.add_space(10.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.snapshot.message)
                                        .size(18.0)
                                        .color(foreground),
                                )
                                .wrap(),
                            );
                            ui.add_space(28.0);
                            choice = choice.or(Self::actions(
                                ui,
                                &self.snapshot,
                                self.deadline.is_some(),
                            ));
                        });
                        ui.add_space(32.0);
                    });
            });
        choice
    }
}

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        if !self.image_attempted {
            self.image_attempted = true;
            if let Some(configured) = &self.snapshot.image {
                match image::load(
                    &configured.path,
                    context.input(|input| input.max_texture_side),
                ) {
                    Ok(pixels) => {
                        self.image = Some((
                            context.load_texture(
                                "break-reminder-image",
                                pixels,
                                egui::TextureOptions::LINEAR
                                    .with_mipmap_mode(Some(egui::TextureFilter::Linear)),
                            ),
                            configured.fit,
                        ));
                    }
                    Err(error) => {
                        eprintln!("cannot display {}: {error}", configured.path.display());
                    }
                }
            }
        }
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
        let mut choice = self.paint(ui, now);
        let mut failed = Vec::new();
        let mut pending = Vec::new();
        for (index, screen) in self.secondary.iter().copied() {
            let title = child_title(index);
            let mut child_error = None;
            #[cfg(target_os = "linux")]
            let mut child_ready = true;
            context.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("break-reminder-screen", index)),
                viewport(screen, title.clone()),
                |ui, class| {
                    if class != egui::ViewportClass::Immediate {
                        child_error = Some("multi-monitor viewports are unavailable".to_owned());
                        return;
                    }
                    #[cfg(target_os = "macos")]
                    if let Err(error) = crate::macos_window::configure_child(&title, screen.frame) {
                        child_error = Some(error);
                        return;
                    }
                    #[cfg(target_os = "linux")]
                    {
                        child_ready = align_child(ui.ctx(), screen);
                    }
                    choice = choice.or_else(|| self.paint(ui, now));
                },
            );
            if let Some(error) = child_error {
                eprintln!("skipping reminder display {index}: {error}");
                failed.push(index);
                continue;
            }
            #[cfg(target_os = "macos")]
            let child_ready = match crate::macos_window::show_child(&title, screen.frame) {
                Ok(visible) => visible,
                Err(error) => {
                    eprintln!("skipping reminder display {index}: {error}");
                    failed.push(index);
                    continue;
                }
            };
            if !child_ready {
                pending.push(index);
            }
        }
        if !failed.is_empty() {
            self.secondary.retain(|(index, _)| !failed.contains(index));
            // A child shown in this pass is removed after a following pass omits it.
            context.request_repaint();
        }
        if self.ready {
            self.drop_pending(&pending, &context);
        }

        if let Some(action) = choice {
            self.finish(action, &context);
            return;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.finish(Action::Elapsed, &context);
            return;
        }
        if self.first_frame.is_none() {
            self.first_frame = Some(context.cumulative_frame_nr_for(egui::ViewportId::ROOT));
            context.request_repaint();
        } else if !self.ready
            && self.first_frame.is_some_and(|first| {
                context.cumulative_frame_nr_for(egui::ViewportId::ROOT) > first
            })
        {
            #[cfg(target_os = "macos")]
            let root_ready = crate::macos_window::root_ready(&self.root, self.root_screen.frame);
            #[cfg(target_os = "linux")]
            let root_ready = root_ready(&self.root, self.root_screen);
            #[cfg(target_os = "macos")]
            let exhausted = self.readiness_retry;
            #[cfg(target_os = "linux")]
            let exhausted = self.readiness_attempts >= 8;
            match readiness(root_ready, !pending.is_empty(), exhausted) {
                Readiness::Retry => {
                    #[cfg(target_os = "macos")]
                    {
                        self.readiness_retry = true;
                    }
                    #[cfg(target_os = "linux")]
                    {
                        self.readiness_attempts += 1;
                    }
                    if let Err(error) = place_root(&self.root, self.root_screen) {
                        eprintln!("cannot reposition reminder root: {error}");
                        context.send_viewport_cmd_to(
                            egui::ViewportId::ROOT,
                            egui::ViewportCommand::Close,
                        );
                        return;
                    }
                    #[cfg(target_os = "macos")]
                    context.request_repaint();
                    #[cfg(target_os = "linux")]
                    context.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                Readiness::DropPending => self.drop_pending(&pending, &context),
                Readiness::Fail => {
                    eprintln!("reminder root did not stay at its display bounds");
                    context
                        .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                    return;
                }
                Readiness::Ready => {}
            }
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

#[cfg(test)]
mod tests {
    use eframe::egui::{self, Rect, pos2, vec2};

    use super::{Overlay, Readiness, physical_to_logical, readiness};
    use crate::{
        config::Config,
        protocol::{Action, Snapshot},
    };

    fn action_frame(
        context: &egui::Context,
        enabled: bool,
        events: Vec<egui::Event>,
    ) -> Option<Action> {
        let snapshot = Snapshot::from_config(&Config::default());
        let mut action = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                Overlay::centered_column(ui, |ui| {
                    action = Overlay::actions(ui, &snapshot, enabled);
                });
            },
        );
        output.drop_without_applying_deltas();
        action
    }

    fn press(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    #[test]
    fn skip_is_initially_focused_and_activates_with_enter_or_space() {
        for key in [egui::Key::Enter, egui::Key::Space] {
            let context = egui::Context::default();
            assert_eq!(action_frame(&context, true, press(key)), Some(Action::Skip));
            let context = egui::Context::default();
            assert_eq!(action_frame(&context, true, vec![]), None);
            assert!(context.memory(|memory| memory.focused().is_some()));
            assert_eq!(action_frame(&context, true, press(key)), Some(Action::Skip));
        }
    }

    #[test]
    fn mouse_movement_and_native_focus_updates_keep_the_selected_action_highlighted() {
        let context = egui::Context::default();
        let snapshot = Snapshot::from_config(&Config::default());
        let accent = Overlay::color(&snapshot.accent_color);
        action_frame(&context, true, vec![]);
        let skip_id = context.memory(|memory| memory.focused()).unwrap();
        let skip_rect = context.read_response(skip_id).unwrap().rect;

        for stage in 0..2 {
            if stage == 1 {
                action_frame(&context, true, press(egui::Key::Tab));
            }
            let selected_id = context.memory(|memory| memory.focused()).unwrap();
            if stage == 1 {
                assert_ne!(selected_id, skip_id);
            }
            for (focused, position) in [
                (true, skip_rect.center()),
                (
                    false,
                    pos2(skip_rect.center().x, skip_rect.bottom() + 100.0),
                ),
                (true, pos2(20.0, 20.0)),
                (false, pos2(40.0, 40.0)),
                (true, skip_rect.center()),
            ] {
                let mut choice = None;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                        focused,
                        events: vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::WindowFocused(focused),
                        ],
                        ..Default::default()
                    },
                    |ui| {
                        Overlay::centered_column(ui, |ui| {
                            choice = Overlay::actions(ui, &snapshot, true);
                        });
                    },
                );
                assert_eq!(choice, None);
                assert_eq!(context.memory(|memory| memory.focused()), Some(selected_id));
                let selected_rect = context.read_response(selected_id).unwrap().rect.shrink(1.0);
                assert!(output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::epaint::Shape::Rect(rect)
                        if rect.rect == selected_rect && rect.stroke == egui::Stroke::new(2.0, accent))
                }), "selection ring disappeared with native focused={focused}");
                output.drop_without_applying_deltas();
            }
        }
        assert_eq!(
            action_frame(&context, true, press(egui::Key::Enter)),
            Some(Action::Postpone(0))
        );
    }

    #[test]
    fn background_transparency_changes_alpha_and_preserves_the_default() {
        let mut snapshot = Snapshot::from_config(&Config::default());
        assert_eq!(Overlay::background_fill(&snapshot).a(), 217);
        for (percent, alpha) in [(0, 255), (15, 217), (35, 166), (50, 128), (100, 0)] {
            snapshot.background_transparency_percent = percent;
            assert_eq!(
                Overlay::background_fill(&snapshot).a(),
                alpha,
                "transparency={percent}%"
            );
        }
    }

    #[test]
    fn every_postpone_row_is_centered_at_all_column_counts() {
        for width in [240.0, 360.0, 480.0, 800.0] {
            for count in 1..=12 {
                let context = egui::Context::default();
                let mut snapshot = Snapshot::from_config(&Config::default());
                snapshot.postpone_minutes = (1..=count).collect();
                let mut center = 0.0;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 900.0))),
                        ..Default::default()
                    },
                    |ui| {
                        Overlay::centered_column(ui, |ui| {
                            center = ui.max_rect().center().x;
                            Overlay::actions(ui, &snapshot, true);
                        });
                    },
                );
                let labels: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::epaint::Shape::Text(text) = &shape.shape
                            && text.galley.text().ends_with(" min")
                        {
                            Some(text.pos + text.galley.size() / 2.0)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(labels.len(), count as usize);
                for label in &labels {
                    let row: Vec<_> = labels
                        .iter()
                        .filter(|other| (other.y - label.y).abs() < 1.0)
                        .collect();
                    let row_center = (row.first().unwrap().x + row.last().unwrap().x) / 2.0;
                    assert!(
                        (row_center - center).abs() < 1.0,
                        "width={width}, choices={count}, row center={row_center}, column center={center}"
                    );
                }
                output.drop_without_applying_deltas();
            }
        }
    }

    #[test]
    fn image_and_no_image_keep_text_at_the_same_lower_position() {
        let context = egui::Context::default();
        let texture = context.load_texture(
            "test-image",
            egui::ColorImage::filled([1600, 1200], egui::Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let picture = (texture, crate::protocol::Fit::Contain);
        let mut positions = Vec::new();
        for image in [None, Some(&picture)] {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 900.0))),
                    ..Default::default()
                },
                |ui| {
                    Overlay::centered_column(ui, |ui| {
                        Overlay::image_slot(ui, image);
                        positions.push(ui.label("Time for a break").rect.top());
                    });
                },
            );
            output.drop_without_applying_deltas();
        }
        assert!(positions[0] >= 280.0);
        assert_eq!(positions[0], positions[1]);
    }

    #[test]
    fn focused_skip_allows_scrolling_back_to_top_on_short_displays() {
        let context = egui::Context::default();
        let snapshot = Snapshot::from_config(&Config::default());
        let mut offset = 0.0;
        for _ in 0..2 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let scroll = egui::ScrollArea::vertical()
                        .animated(false)
                        .vertical_scroll_offset(0.0)
                        .show(ui, |ui| {
                            ui.add_space(700.0);
                            Overlay::centered_column(ui, |ui| {
                                Overlay::actions(ui, &snapshot, true);
                            });
                        });
                    offset = scroll.state.offset.y;
                },
            );
            output.drop_without_applying_deltas();
        }
        assert!(context.memory(|memory| memory.focused().is_some()));
        assert_eq!(
            offset, 0.0,
            "focused Skip must not undo scrolling to the top"
        );
    }

    #[test]
    fn skip_focus_waits_until_start_and_does_not_override_tab_navigation() {
        let context = egui::Context::default();
        assert_eq!(action_frame(&context, false, press(egui::Key::Enter)), None);
        assert!(context.memory(|memory| memory.focused().is_none()));
        action_frame(&context, true, vec![]);
        let skip_id = context.memory(|memory| memory.focused()).unwrap();
        action_frame(&context, true, press(egui::Key::Tab));
        assert_ne!(context.memory(|memory| memory.focused()), Some(skip_id));
        assert_eq!(
            action_frame(&context, true, press(egui::Key::Enter)),
            Some(Action::Postpone(0))
        );
    }

    #[test]
    fn readiness_drops_pending_secondaries_only_after_root_is_ready() {
        for pending in [false, true] {
            assert_eq!(readiness(false, pending, false), Readiness::Retry);
            assert_eq!(readiness(false, pending, true), Readiness::Fail);
        }
        assert_eq!(readiness(true, true, false), Readiness::Retry);
        assert_eq!(readiness(true, true, true), Readiness::DropPending);
        for exhausted in [false, true] {
            assert_eq!(readiness(true, false, exhausted), Readiness::Ready);
        }
    }

    #[test]
    fn reminder_column_is_centered_in_800_point_viewport() {
        let context = egui::Context::default();
        let mut available = Rect::NOTHING;
        let mut column = Rect::NOTHING;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(48.0);
                        available = ui.available_rect_before_wrap();
                        Overlay::centered_column(ui, |ui| {
                            column = ui.max_rect();
                            ui.label("Break reminder");
                        });
                    });
            },
        );
        output.drop_without_applying_deltas();
        assert!((column.center().x - available.center().x).abs() < 1.0);
        assert!(column.width() <= 680.0);
    }

    #[test]
    fn x11_monitor_geometry_preserves_negative_origin_and_scale() {
        let (position, size) = physical_to_logical((-2400, 900), (2400, 1350), 1.5);
        assert_eq!(position, [-1600.0, 600.0]);
        assert_eq!(size, [1600.0, 900.0]);
        let (position, size) = physical_to_logical((-2400, 900), (2400, 1350), 2.0);
        assert_eq!(position, [-1200.0, 450.0]);
        assert_eq!(size, [1200.0, 675.0]);
    }
}
