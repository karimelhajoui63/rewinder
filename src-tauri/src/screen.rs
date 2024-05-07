use xcap::{image::ImageError, Monitor};
use mouse_position::mouse_position::Mouse;
use std::{path::PathBuf, time::{Duration, Instant}};

use tokio::{
    task::spawn,
    time::interval
};

use std::fs;


fn get_current_monitor() -> Monitor {
    let position = Mouse::get_mouse_position();
    match position {
        Mouse::Position { x, y } => {
            return Monitor::from_point(x, y).unwrap();
        },
        Mouse::Error => panic!("Error getting mouse position"),
    }
}

fn save_monitor_screen(monitor: Monitor, to: PathBuf) -> Result<(), ImageError> {
    let _ = fs::create_dir_all(to.clone());

    let image = monitor.capture_image().unwrap();
    return image.save(format!("{}/screenshot.png", to.to_string_lossy()))
}

fn should_capture_screen() -> bool {
    true
}

fn capture_screen(screen_dir: PathBuf) {
    let monitor = get_current_monitor();
    match save_monitor_screen(monitor, screen_dir) {
        Ok(_) => println!("Screen captured!"),
        Err(e) => println!("Error: {}", e)
    }
}


fn capture_screen_loop(screen_dir: PathBuf) {
    const INTERVAL_SEC: u64 = 5;
    
    spawn(async move {
        let mut interval = interval(Duration::from_secs(INTERVAL_SEC));

        loop {
            interval.tick().await;
            if should_capture_screen() {
                let start_time = Instant::now();
                capture_screen(screen_dir.clone());
                let elapsed_time = start_time.elapsed();
                println!("capture_screen took: {:?}", elapsed_time);
                // Save the elapsed time into a file
                let mut output_path = screen_dir.clone();
                output_path.push("elapsed_time.txt");
                fs::write(output_path, format!("{:?}", elapsed_time)).expect("Unable to write file");
            }
        }
    });
}

pub fn setup_handler(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    match app.handle().path_resolver().app_data_dir() {
        Some(app_data_dir) => {
            let mut screen_dir = app_data_dir.clone();
            screen_dir.push("screen");
            println!("app_local_data_dir: {}", screen_dir.display());
            fs::create_dir_all(&screen_dir)?;
            capture_screen_loop(screen_dir);
        },
        None => {
            println!("app_local_data_dir: not found");
        }
    }
    Ok(())
}



