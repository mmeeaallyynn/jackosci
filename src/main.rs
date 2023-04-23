use std::collections::VecDeque;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use itertools::Itertools;
use jack::{
    Port, AudioIn
};
use eframe::egui;
use eframe::egui::widgets::plot;

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
        //println!("{:?}", input);
        self.tx.send(Vec::from(input));
        jack::Control::Continue
    }
}

struct OsciApp {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    active_client: jack::AsyncClient<Notifications, Processor>,
}

impl OsciApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = mpsc::channel();
        let (client, _status) =
            jack::Client::new("rust_jack_simple", jack::ClientOptions::NO_START_SERVER).unwrap();

        let input_port = client
            .register_port("input", jack::AudioIn::default())
            .unwrap();
        let processor = Processor {
            input_port,
            tx
        };
        let buffer = Arc::new(Mutex::new(VecDeque::from([0.0; 1000])));

        let moved_buffer = buffer.clone();
        let updater = thread::spawn(move || {
            let mut sliding_buffer = VecDeque::from([0.0; 1000]);
            let buffer = moved_buffer;
            loop {
                let input = rx.recv().expect("there is nothing!");
                {
                    for (item, next) in input.into_iter().tuple_windows() {
                        if item > 0.2 && next > item {
                            let mut buffer = buffer.lock().unwrap();
                            std::mem::replace(&mut *buffer, sliding_buffer.clone());
                        }
                        let _ = sliding_buffer.pop_front();
                        sliding_buffer.push_back(item);
                    }
                }
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
            buffer,
            active_client
        }
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Osci");
            self.draw_line(&ui, &frame); 
       });
    }
}
