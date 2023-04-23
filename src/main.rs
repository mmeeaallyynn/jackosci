use std::collections::VecDeque;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use itertools::Itertools;

use eframe::egui;


fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Osci", native_options, Box::new(|cc| Box::new(OsciApp::new(cc))))
        .expect("Cant start!");
}

struct Notifications;
impl jack::NotificationHandler for Notifications {}

struct Processor {
    input_port: jack::Port<jack::AudioIn>,
    tx: Sender<Vec<f32>>
}

impl jack::ProcessHandler for Processor {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let input = self.input_port.as_slice(ps);
        let _ = self.tx.send(Vec::from(input));
        jack::Control::Continue
    }
}

#[derive(Clone, Copy)]
enum TriggerMode {
    RisingEdge,
    FallingEdge
}

impl TriggerMode {
    fn test(&self, level: f32, prev: f32, value: f32) -> bool {
        match self {
            Self::RisingEdge => value > prev && level >= prev && level < value,
            Self::FallingEdge => prev > value && level >= value && level < prev
        }
    }
}

struct OsciConfig {
    trigger_mode: TriggerMode,
    trigger_level: f32,
    buffer_size: usize
}

struct OsciApp {
    config: Arc<Mutex<OsciConfig>>,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    _active_client: jack::AsyncClient<Notifications, Processor>
}

impl OsciApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = mpsc::channel();
        let (client, _status) =
            jack::Client::new("small_osci", jack::ClientOptions::NO_START_SERVER).unwrap();

        let input_port = client
            .register_port("input", jack::AudioIn::default())
            .unwrap();
        let processor = Processor {
            input_port,
            tx
        };
        let buffer = Arc::new(Mutex::new(VecDeque::from([0.0; 1000])));
        let config = Arc::new(Mutex::new(OsciConfig {
            trigger_mode: TriggerMode::RisingEdge,
            trigger_level: 0.0,
            buffer_size: 1000
        }));

        let moved_buffer = buffer.clone();
        let moved_config = config.clone();
        let _updater = thread::spawn(move || {
            let mut sliding_buffer = VecDeque::from([0.0; 1000]);
            let buffer = moved_buffer;
            let mut previous_last = 0.0;
            let config = moved_config;
            loop {
                let OsciConfig {
                    trigger_mode,
                    trigger_level, ..
                } = *config.lock().unwrap();

                let mut input = rx.recv().expect("there is nothing!");
                input.insert(0, previous_last);
                for (&prev, &item) in input.iter().tuple_windows() {
                    if trigger_mode.test(trigger_level, prev, item) {
                        let mut buffer = buffer.lock().unwrap();
                        let _ = std::mem::replace(&mut *buffer, sliding_buffer.clone());
                    }
                    let _ = sliding_buffer.pop_front();
                    sliding_buffer.push_back(item);
                }
                previous_last = *input.last().unwrap();
            }
        });

        let moved_context = cc.egui_ctx.clone();
        let _ = thread::spawn(move || {
            let context = moved_context;
            loop {
                context.request_repaint();
                thread::sleep(Duration::from_millis(20));
            }
        });

        let active_client = client.activate_async(Notifications, processor).unwrap();

        Self {
            config,
            buffer,
            _active_client: active_client
        }
    }

    fn draw_trigger(&self, ui: &egui::Ui, frame: &eframe::Frame) {
        let window_size = frame.info().window_info.size;
        let stroke = egui::Stroke::new(1.0, egui::Color32::LIGHT_YELLOW);
        let painter = ui.painter();
        let OsciConfig { trigger_level, .. } = *self.config.lock().unwrap();

        painter.line_segment(
            [egui::Pos2 { x: 0.0, y: (1.0 - trigger_level) * window_size.y - window_size.y / 2.0 },
             egui::Pos2 { x: window_size.x, y: (1.0 - trigger_level) * window_size.y - window_size.y / 2.0 }],
            stroke
        );
    }

    fn draw_line(&self, ui: &egui::Ui, frame: &eframe::Frame) {
        let window_size = frame.info().window_info.size;
        let stroke = egui::Stroke::new(1.0, egui::Color32::YELLOW);
        let painter = ui.painter();
        let buffer = self.buffer.lock().unwrap().clone();
        let coords = std::iter::zip(0..1000, buffer);

        for ((x1, y1), (x2, y2)) in coords.tuple_windows() {
            let x1 = x1 as f32 / 1000.0 * window_size.x;
            let x2 = x2 as f32 / 1000.0 * window_size.x;
            painter.line_segment(
                [egui::Pos2 { x: x1, y: (1.0 - y1) * window_size.y - window_size.y / 2.0 },
                 egui::Pos2 { x: x2, y: (1.0 - y2) * window_size.y - window_size.y / 2.0 }],
                stroke
            );
        }
    }
}

impl eframe::App for OsciApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let mut config = self.config.lock().unwrap();
                let mut trigger_level = config.trigger_level;
                let current_trigger_name = match config.trigger_mode {
                    TriggerMode::RisingEdge => "Trigger: Rising Edge",
                    TriggerMode::FallingEdge => "Trigger: Falling Edge"
                };

                ui.menu_button(current_trigger_name, |ui| {
                    if ui.button("Rising Edge").clicked() {
                        config.trigger_mode = TriggerMode::RisingEdge;
                    }
                    else if ui.button("Falling Edge").clicked() {
                        config.trigger_mode = TriggerMode::FallingEdge;
                    }
                });

                ui.add(egui::Slider::new(&mut trigger_level, -1.0..=1.0).text("trigger level"));
                config.trigger_level = trigger_level;
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Osci");
            self.draw_line(&ui, &frame); 
            self.draw_trigger(&ui, &frame);
       });
    }
}
