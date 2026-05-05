#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide

use color_eyre::eyre::Result;

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<()> {
    use clap::Parser;
    use color_eyre::Report;

    #[derive(Parser, Debug)]
    struct Args {
        /// Input file
        file: Option<String>,

        /// hashcodes.h
        #[arg(long, short = 't')]
        hashcodes: Option<String>,
    }
    let args = Args::parse();

    // Force enable backtraces
    std::env::set_var("RUST_BACKTRACE", "1");

    eurochef_gui::panic_dialog::setup();

    let log_buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    #[derive(Clone)]
    struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let s = String::from_utf8_lossy(buf).into_owned();
            if let Ok(mut logs) = self.0.lock() {
                for line in s.lines() {
                    logs.push(line.to_string());
                }
                let len = logs.len();
                if len > 1000 {
                    logs.drain(0..len - 1000);
                }
            }
            print!("{}", s);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            use std::io::Write;
            std::io::stdout().flush()
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    tracing_subscriber::fmt()
        .with_writer(LogBuffer(log_buffer.clone()))
        .with_ansi(false)
        .init();

    let native_options = eframe::NativeOptions {
        initial_window_size: Some([1280., 1024.].into()),
        depth_buffer: 24,
        multisampling: 0,
        ..Default::default()
    };
    let res = eframe::run_native(
        "Eurochef",
        native_options,
        Box::new(|cc| {
            Box::new(eurochef_gui::EurochefApp::new(
                args.file,
                args.hashcodes,
                cc,
                log_buffer,
            ))
        }),
    );

    match res {
        Ok(()) => Ok(()),
        Err(e) => Err(Report::msg(e.to_string())),
    }
}

// when compiling to web using trunk.
#[cfg(target_arch = "wasm32")]
fn main() {
    // Make sure panics are logged using `console.error`.
    console_error_panic_hook::set_once();

    // Redirect tracing to console.log and friends:
    tracing_wasm::set_as_global_default();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "the_canvas_id", // hardcode it
                web_options,
                Box::new(|cc| Box::new(eurochef_gui::EurochefApp::new(None, None, cc, std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))))),
            )
            .await
            .expect("failed to start eframe");
    });
}
