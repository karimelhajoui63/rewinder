use mouse_position::mouse_position::Mouse;
use std::{
    fs::{self, File},
    io::{Cursor, Read},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use xcap::{image::ImageError, Monitor};

use anyhow::anyhow;
use chacha20poly1305::{
    aead::{Aead, NewAead},
    XChaCha20Poly1305,
};
use rand::{rngs::OsRng, RngCore};

use tokio::{task::spawn, time::interval};

use rdev::{listen, Event};

use once_cell::sync::Lazy;

static SCREEN_DIR: Lazy<Mutex<PathBuf>> = Lazy::new(|| Mutex::new(PathBuf::new()));
static APP_DATA_DIR: Lazy<Mutex<PathBuf>> = Lazy::new(|| Mutex::new(PathBuf::new()));
static KEY: Lazy<Mutex<[u8; 32]>> = Lazy::new(|| Mutex::new([0u8; 32]));
static NONCE: Lazy<Mutex<[u8; 24]>> = Lazy::new(|| Mutex::new([0u8; 24]));

fn get_current_monitor() -> Monitor {
    let position = Mouse::get_mouse_position();
    match position {
        Mouse::Position { x, y } => {
            return Monitor::from_point(x, y).unwrap();
        }
        Mouse::Error => panic!("Error getting mouse position"),
    }
}

pub fn clear_screen_dir() {
    let screen_dir = SCREEN_DIR.lock().unwrap().clone();
    let _ = fs::remove_dir_all(&screen_dir);
    let _ = fs::create_dir_all(&screen_dir);
}

fn save_monitor_screen(monitor: Monitor) -> Result<(), ImageError> {
    let image = monitor.capture_image().unwrap();

    // Convert the Rgba to Rgb in order to use Jpeg format
    let image = DynamicImage::ImageRgba8(image).to_rgb8();

    // Compress the image with JPEG
    let mut cursor = Cursor::new(Vec::new());
    // TODO: allow the user to set the quality
    let encoder = JpegEncoder::new_with_quality(&mut cursor, 95);
    image.write_with_encoder(encoder)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let conn =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("rewinder.db"))
            .unwrap();

    let img_data = ImageData::new(timestamp as i64, cursor.into_inner());

    let _ = insert_image(&conn, &img_data);

    return Ok(());
}

fn should_capture_screen() -> bool {
    true
}

pub fn is_periodic_capture_enabled() -> bool {
    let config_json: serde_json::Value = get_config_json();
    return config_json["periodic_capture_enabled"].as_bool().unwrap();
}

pub fn toggle_settings(setting: &str, enable: bool) {
    let mut config_json: serde_json::Value = get_config_json();
    config_json[setting] = serde_json::Value::Bool(enable);
    fs::write(
        get_config_path(),
        serde_json::to_string_pretty(&config_json).unwrap(),
    )
    .unwrap();
}

pub fn is_encryption_enabled() -> bool {
    let config_json: serde_json::Value = get_config_json();
    return config_json["encryption_enabled"].as_bool().unwrap();
}

// fn that return the json of the config file and create it if it doesn't exist yet
fn get_config_json() -> serde_json::Value {
    let path = get_config_path();
    if !path.exists() {
        let default_config = serde_json::json!({
            "encryption_enabled": false,
            "periodic_capture_enabled": false,
            "click_event_enabled": false
        });
        fs::write(path, serde_json::to_string_pretty(&default_config).unwrap()).unwrap();
        return default_config;
    }

    let config_content = fs::read_to_string(path).unwrap();
    return serde_json::from_str(&config_content).unwrap();
}

fn get_config_path() -> PathBuf {
    PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("config.json")
}

pub fn is_click_event_enabled() -> bool {
    let config_json: serde_json::Value = get_config_json();
    return config_json["click_event_enabled"].as_bool().unwrap();
}

fn capture_screen() {
    let monitor = get_current_monitor();
    match save_monitor_screen(monitor) {
        Ok(_) => println!("Screen captured!"),
        Err(e) => println!("Error: {}", e),
    }
}

fn capture_screen_loop() {
    const INTERVAL_SEC: u64 = 10;

    spawn(async move {
        let mut interval = interval(Duration::from_secs(INTERVAL_SEC));

        loop {
            interval.tick().await;
            if should_capture_screen() {
                let start_time = Instant::now();
                capture_screen();
                let elapsed_time = start_time.elapsed();
                println!("capture_screen took: {:?}", elapsed_time);
                // Save the elapsed time into a file
                let mut output_path = SCREEN_DIR.lock().unwrap().clone();
                output_path.push("elapsed_time.txt");
                fs::write(output_path, format!("{:?}", elapsed_time))
                    .expect("Unable to write file");
            }
        }
    });
}

fn listen_click_event_loop() {
    fn callback(event: Event) {
        if event.event_type == rdev::EventType::ButtonPress(rdev::Button::Left) {
            println!("CLICKED");
            if should_capture_screen() {
                capture_screen();
            }
        }
    }

    spawn(async move {
        // This will block.
        if let Err(error) = listen(callback) {
            println!("Error: {:?}", error)
        }
    });
}

pub fn get_image_from_db(timestamp: u64) -> Result<Vec<u8>, anyhow::Error> {
    let conn =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("rewinder.db"))?;

    let img_data = retrieve_image(&conn, timestamp)?;

    Ok(img_data.data)
}

pub fn setup_handler(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    match app.handle().path_resolver().app_data_dir() {
        Some(app_data_dir) => {
            let mut screen_dir = app_data_dir.clone();
            screen_dir.push("screen");
            fs::create_dir_all(&screen_dir)?;

            *APP_DATA_DIR.lock().unwrap() = app_data_dir;
            *SCREEN_DIR.lock().unwrap() = screen_dir;
            println!(
                "app_local_data_dir: {}",
                SCREEN_DIR.lock().unwrap().display()
            );
        }
        None => {
            // TODO: warn user that app_data_dir is not found or not accessible
            println!("app_local_data_dir: not found");
        }
    }

    if is_encryption_enabled() {
        fetch_or_generate_key_and_nonce();
    }
    if is_click_event_enabled() {
        listen_click_event_loop();
    }
    if is_periodic_capture_enabled() {
        capture_screen_loop();
    }

    init_sqlite().unwrap();

    Ok(())
}

fn encrypt_small_file(filepath: &str, dist: &str) -> Result<(), anyhow::Error> {
    let key = KEY.lock().unwrap().clone();
    let nonce = NONCE.lock().unwrap().clone();

    let cipher = XChaCha20Poly1305::new(&key.into());

    let file_data = fs::read(filepath)?;

    let encrypted_file = cipher
        .encrypt(&nonce.into(), file_data.as_ref())
        .map_err(|err| anyhow!("Encrypting small file: {}", err))?;

    fs::write(&dist, encrypted_file)?;

    Ok(())
}

fn decrypt_small_file(encrypted_file_path: &str, dist: &str) -> Result<(), anyhow::Error> {
    let key = KEY.lock().unwrap().clone();
    let nonce = NONCE.lock().unwrap().clone();

    let cipher = XChaCha20Poly1305::new(&key.into());

    let file_data = fs::read(encrypted_file_path)?;

    let decrypted_file = cipher
        .decrypt(&nonce.into(), file_data.as_ref())
        .map_err(|err| anyhow!("Decrypting small file: {}", err))?;

    fs::write(&dist, decrypted_file)?;

    Ok(())
}

fn generate_random_key_and_nonce() -> ([u8; 32], [u8; 24]) {
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    // Generate random key and nonce
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    (key, nonce)
}

fn delete_key_and_nonce() {
    keytar::delete_password("rewinder", "encryption_key").unwrap();
    keytar::delete_password("rewinder", "encryption_nonce").unwrap();
}

fn fetch_or_generate_key_and_nonce() {
    let mut should_generate_key_and_nonce = false;
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];

    match keytar::get_password("rewinder", "encryption_key") {
        Ok(encryption_key) => {
            if encryption_key.password.is_empty() {
                should_generate_key_and_nonce = true;
            } else {
                // TODO: handle unwrap
                key = to_bytes(&encryption_key.password).try_into().unwrap();
                println!("Key: {:?}", to_string(&key));
            }
        }
        Err(_) => should_generate_key_and_nonce = true,
    }

    match keytar::get_password("rewinder", "encryption_nonce") {
        Ok(encryption_nonce) => {
            if encryption_nonce.password.is_empty() {
                should_generate_key_and_nonce = true;
            } else {
                nonce = to_bytes(&encryption_nonce.password).try_into().unwrap();
                println!("Nonce: {}", to_string(&nonce));
            }
        }
        Err(_) => should_generate_key_and_nonce = true,
    }

    if should_generate_key_and_nonce {
        println!("Generating key and nonce...");
        let (key, nonce) = generate_random_key_and_nonce();
        keytar::set_password("rewinder", "encryption_key", &to_string(&key)).unwrap();
        keytar::set_password("rewinder", "encryption_nonce", &to_string(&nonce)).unwrap();
    }

    *KEY.lock().unwrap() = key;
    *NONCE.lock().unwrap() = nonce;
}

fn to_string(v: &[u8]) -> String {
    v.to_vec().iter().map(|b| *b as char).collect::<String>()
}

// reverse of `to_string` function
fn to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

fn example_of_how_to_use_encryption() -> Result<(), anyhow::Error> {
    let start = Instant::now();

    println!("Encrypting image.png to image.encrypted");
    encrypt_small_file("src/image.png", "src/image.encrypted")?;

    println!("Decrypting image.encrypted to image.decrypted");
    decrypt_small_file("src/image.encrypted", "src/image.decrypted.png")?;

    let duration = start.elapsed();
    println!("Encryption/Decryption Time: {:?}", duration);

    Ok(())
}

// ======================================= SQLITE ======================================

extern crate image;
use image::{codecs::jpeg::JpegEncoder, DynamicImage};
use rusqlite::{params, Connection, Result};

struct ImageData {
    timestamp: i64,
    data: Vec<u8>,
}

impl ImageData {
    fn new(timestamp: i64, data: Vec<u8>) -> Self {
        Self { timestamp, data }
    }
}

fn init_sqlite() -> Result<()> {
    // Connect to SQLite database
    let conn: Connection =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("rewinder.db"))?;

    // Create table if not exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS images (
                  id INTEGER PRIMARY KEY,
                  timestamp INTEGER,
                  data BLOB
                  )",
        [],
    )?;

    Ok(())
}

fn insert_image(conn: &Connection, img_data: &ImageData) -> Result<()> {
    // Insert image into database
    conn.execute(
        "INSERT INTO images (timestamp, data) VALUES (?1, ?2)",
        params![img_data.timestamp, img_data.data],
    )?;

    Ok(())
}

fn retrieve_image(conn: &Connection, timestamp: u64) -> Result<ImageData> {
    // Retrieve the image from the database
    let mut stmt = conn.prepare("SELECT timestamp, data FROM images WHERE timestamp = ?1")?;
    let img_row = stmt.query_row(params![timestamp], |row| {
        let timestamp: i64 = row.get(0)?;
        let data: Vec<u8> = row.get(1)?;
        Ok((timestamp, data))
    })?;

    let (timestamp, data) = img_row;
    Ok(ImageData::new(timestamp, data))

    // let mut stmt =
    //     conn.prepare("SELECT timestamp, data FROM images ORDER BY timestamp DESC LIMIT 1")?;
    // let img_row = stmt.query_row([], |row| {
    //     let timestamp: i64 = row.get(0)?;
    //     let data: Vec<u8> = row.get(1)?;
    //     Ok((timestamp, data))
    // })?;

    // let (timestamp, data) = img_row;
    // Ok(ImageData::new(timestamp, data))
}
