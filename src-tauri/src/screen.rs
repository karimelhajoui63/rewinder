use mouse_position::mouse_position::Mouse;
use std::{
    fs::{self},
    io::Cursor,
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

pub fn delete_db() {
    // remove the sqlite db file
    fs::remove_file(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))
        .expect("Unable to delete file");
    init_sqlite().unwrap();
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
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))
            .unwrap();

    let is_encryption_enabled = is_encryption_enabled();

    if is_encryption_enabled {
        cursor = Cursor::new(encrypt_text(cursor.into_inner()).unwrap());
    }

    let img_data = ImageData::new(
        timestamp as i64,
        cursor.into_inner(),
        Some(is_encryption_enabled),
    );

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
                let mut output_path = APP_DATA_DIR.lock().unwrap().clone();
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
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))?;

    let img_data = retrieve_image(&conn, timestamp)?;

    Ok(img_data.data)
}

pub fn setup_handler(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    match app.handle().path_resolver().app_data_dir() {
        Some(app_data_dir) => {
            *APP_DATA_DIR.lock().unwrap() = app_data_dir;
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

fn encrypt_text(plain_text: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    let key = KEY.lock().unwrap().clone();
    let nonce = NONCE.lock().unwrap().clone();

    let cipher = XChaCha20Poly1305::new(&key.into());

    let cipher_text = cipher
        .encrypt(&nonce.into(), plain_text.as_ref())
        .map_err(|err| anyhow!("Encrypting small file: {}", err))?;

    Ok(cipher_text)
}

fn decrypt_text(cipher_text: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    let key = KEY.lock().unwrap().clone();
    let nonce = NONCE.lock().unwrap().clone();

    let cipher = XChaCha20Poly1305::new(&key.into());

    let plain_text = cipher
        .decrypt(&nonce.into(), cipher_text.as_ref())
        .map_err(|err| anyhow!("Decrypting small file: {}", err))?;

    Ok(plain_text)
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

// ======================================= SQLITE ======================================

extern crate image;
use image::{codecs::jpeg::JpegEncoder, DynamicImage};
use rusqlite::{params, Connection, Result};

struct ImageData {
    timestamp: i64,
    data: Vec<u8>,
    encrypted: Option<bool>,
}

impl ImageData {
    fn new(timestamp: i64, data: Vec<u8>, encrypted: Option<bool>) -> Self {
        Self {
            timestamp,
            data,
            encrypted,
        }
    }
}

fn init_sqlite() -> Result<()> {
    // Connect to SQLite database
    let conn: Connection =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))?;

    // Create table if not exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS images (
                  id INTEGER PRIMARY KEY,
                  timestamp INTEGER,
                  data BLOB,
                  encrypted INTEGER
                  )",
        [],
    )?;

    Ok(())
}

fn insert_image(conn: &Connection, img_data: &ImageData) -> Result<()> {
    // Insert image into database
    println!("Inserting image into database...");
    conn.execute(
        "INSERT INTO images (timestamp, data, encrypted) VALUES (?1, ?2, ?3)",
        params![img_data.timestamp, img_data.data, img_data.encrypted],
    )?;

    Ok(())
}

fn retrieve_image(conn: &Connection, timestamp: u64) -> Result<ImageData> {
    // Retrieve the image from the database
    let mut stmt =
        conn.prepare("SELECT timestamp, data, encrypted FROM images WHERE timestamp = ?1")?;
    let img_row = stmt.query_row(params![timestamp], |row| {
        let timestamp: i64 = row.get(0)?;
        let data: Vec<u8> = row.get(1)?;
        let encrypted: bool = row.get(2)?;
        Ok((timestamp, data, encrypted))
    })?;

    let (timestamp, mut data, encrypted) = img_row;

    if encrypted {
        let decrypted_data = decrypt_text(data);
        match decrypted_data {
            Ok(decrypted_data) => {
                data = decrypted_data;
            }
            Err(_) => {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }
    }

    Ok(ImageData::new(timestamp, data, None))
}
