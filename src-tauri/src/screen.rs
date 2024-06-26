use base64::{engine::general_purpose::STANDARD, Engine as _};
use mouse_position::mouse_position::Mouse;
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use xcap::Monitor;

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

fn save_monitor_screen(monitor: Monitor) -> Result<(), anyhow::Error> {
    let image = monitor.capture_image().unwrap();

    // Convert the Rgba to Rgb in order to use Jpeg format
    let image = DynamicImage::ImageRgba8(image).to_rgb8();

    // Compress the image with JPEG
    let mut cursor = Cursor::new(Vec::new());
    // TODO: allow the user to set the quality
    let encoder = JpegEncoder::new_with_quality(&mut cursor, 95);
    image.write_with_encoder(encoder)?;

    // Thumbnail the image
    let thumbnail = DynamicImage::ImageRgb8(image).thumbnail(200, 200).to_rgb8();
    let mut thumbnail_cursor = Cursor::new(Vec::new());
    let thumbnail_encoder = JpegEncoder::new_with_quality(&mut thumbnail_cursor, 95);
    thumbnail.write_with_encoder(thumbnail_encoder)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let conn =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))
            .unwrap();

    let base64 = STANDARD.encode(&cursor.into_inner());
    let thumbnail_base64 = STANDARD.encode(&thumbnail_cursor.into_inner());

    let img_dto = ImageDTO::new(timestamp, base64, thumbnail_base64);

    let _ = insert_image(&conn, &img_dto);

    return Ok(());
}

fn try_encrypt_text(plain_text: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    let encrypted_text;
    match encrypt_text(plain_text.clone()) {
        Ok(encrypted_data) => {
            encrypted_text = encrypted_data;
        }
        Err(_) => {
            // Try to fetch or generate key and nonce again
            match fetch_or_generate_key_and_nonce() {
                Ok(_) => match encrypt_text(plain_text) {
                    Ok(encrypted_data) => {
                        encrypted_text = encrypted_data;
                    }
                    Err(_) => {
                        return Err(anyhow!("Error encrypting image"));
                    }
                },
                Err(_) => {
                    println!("Error fetching or generating key and nonce, desabling encryption...");
                    return Err(anyhow!("Error fetching or generating key and nonce"));
                }
            }
        }
    }
    return Ok(encrypted_text);
}

fn should_capture_screen() -> bool {
    true
}

pub fn is_periodic_capture_enabled() -> bool {
    let config_json: serde_json::Value = get_config_json();
    return config_json["periodic_capture_enabled"].as_bool().unwrap();
}

pub fn toggle_encryption(enable: bool) -> Result<bool, String> {
    if enable {
        match fetch_or_generate_key_and_nonce() {
            Ok(_) => {}
            Err(_) => {
                return Err("Error fetching or generating key and nonce".into());
            }
        }
    }
    update_settings_in_config_file("encryption_enabled", enable);
    return Ok(enable);
}

pub fn toggle_periodic_capture(enable: bool) -> Result<bool, String> {
    update_settings_in_config_file("periodic_capture_enabled", enable);
    Ok(enable)
}

pub fn toggle_click_event(enable: bool) -> Result<bool, String> {
    update_settings_in_config_file("click_event_enabled", enable);
    Ok(enable)
}

fn update_settings_in_config_file(setting: &str, enable: bool) {
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
                output_path.push("test_elapsed_time.txt");
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

pub fn get_image_base64_from_db(timestamp: u64) -> Result<String, anyhow::Error> {
    let conn =
        Connection::open(PathBuf::from(APP_DATA_DIR.lock().unwrap().clone()).join("sqlite.db"))?;

    let img_dto = retrieve_image(&conn, timestamp)?;

    Ok(img_dto.base64)
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
        match fetch_or_generate_key_and_nonce() {
            Ok(_) => {}
            Err(_) => {
                println!("Error fetching or generating key and nonce, desabling encryption...");
                update_settings_in_config_file("encryption_enabled", false);
            }
        }
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

    if key == [0u8; 32] || nonce == [0u8; 24] {
        return Err(anyhow!("Key or nonce is empty"));
    }

    println!("Key: {:?}", to_string(&key));
    println!("Nonce: {}", to_string(&nonce));

    let cipher = XChaCha20Poly1305::new(&key.into());

    let cipher_text: Vec<u8> = cipher
        .encrypt(&nonce.into(), plain_text.as_ref())
        .map_err(|err| anyhow!("Encrypting small file: {}", err))?;

    Ok(cipher_text)
}

fn decrypt_text(cipher_text: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    let key = KEY.lock().unwrap().clone();
    let nonce = NONCE.lock().unwrap().clone();

    if key == [0u8; 32] || nonce == [0u8; 24] {
        return Err(anyhow!("Key or nonce is empty"));
    }

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

pub fn delete_key_and_nonce() {
    println!("Deleting key and nonce...");
    keytar::delete_password("rewinder", "encryption_key").unwrap();
    keytar::delete_password("rewinder", "encryption_nonce").unwrap();
    // reset the key and nonce
    *KEY.lock().unwrap() = [0u8; 32];
    *NONCE.lock().unwrap() = [0u8; 24];
}

fn fetch_or_generate_key_and_nonce() -> Result<(), anyhow::Error> {
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
        Err(_) => {
            println!("Error getting key");
            return Err(anyhow!("Error getting key"));
            // should_generate_key_and_nonce = true;
        }
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
        Err(_) => {
            println!("Error getting nonce");
            return Err(anyhow!("Error getting key"));
            // should_generate_key_and_nonce = true;
        }
    }

    if should_generate_key_and_nonce {
        println!("Generating key and nonce...");
        let (key, nonce) = generate_random_key_and_nonce();
        match keytar::set_password("rewinder", "encryption_key", &to_string(&key)) {
            Ok(_) => {}
            Err(_) => {
                println!("Error setting key");
            }
        }
        match keytar::set_password("rewinder", "encryption_nonce", &to_string(&nonce)) {
            Ok(_) => {}
            Err(_) => {
                println!("Error setting nonce");
            }
        }
    }

    *KEY.lock().unwrap() = key;
    *NONCE.lock().unwrap() = nonce;

    Ok(())
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

struct ImageDTO {
    timestamp: u64,
    base64: String,
    thumbnail_base64: String,
}

impl ImageDTO {
    fn new(timestamp: u64, base64: String, thumbnail_base64: String) -> Self {
        Self {
            timestamp,
            base64,
            thumbnail_base64,
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
                  base64 BLOB,
                  thumbnail_base64 BLOB,
                  encrypted INTEGER
                  )",
        [],
    )?;

    Ok(())
}

fn insert_image(conn: &Connection, img_dto: &ImageDTO) -> Result<()> {
    let mut base64 = Vec::new();
    let mut thumbnail_base64 = Vec::new();

    let is_encryption_enabled = is_encryption_enabled();

    if is_encryption_enabled {
        match try_encrypt_text(img_dto.base64.clone().into_bytes()) {
            Ok(encrypted_base64) => {
                base64 = encrypted_base64;
            }
            Err(_) => {
                println!("disabling encryption...");
                update_settings_in_config_file("encryption_enabled", false);
            }
        }
        match try_encrypt_text(img_dto.thumbnail_base64.clone().into_bytes()) {
            Ok(encrypted_thumbnail_base64) => {
                thumbnail_base64 = encrypted_thumbnail_base64;
            }
            Err(_) => {
                println!("desabling encryption...");
                update_settings_in_config_file("encryption_enabled", false);
            }
        }
    } else {
        base64 = img_dto.base64.clone().into_bytes();
        thumbnail_base64 = img_dto.thumbnail_base64.clone().into_bytes();
    }

    // Insert image into database
    println!("Inserting image into database...");
    conn.execute(
        "INSERT INTO images (timestamp, base64, thumbnail_base64, encrypted) VALUES (?1, ?2, ?3, ?4)",
        params![img_dto.timestamp, base64, thumbnail_base64, is_encryption_enabled],
    )?;

    Ok(())
}

fn retrieve_image(conn: &Connection, timestamp: u64) -> Result<ImageDTO, anyhow::Error> {
    // Retrieve the image from the database
    let mut stmt = conn.prepare(
        "SELECT timestamp, base64, thumbnail_base64, encrypted FROM images WHERE timestamp = ?1",
    )?;
    let img_row = stmt
        .query_row(params![timestamp], |row| {
            let timestamp: u64 = row.get(0)?;
            let base64: Vec<u8> = row.get(1)?;
            let thumbnail_base64: Vec<u8> = row.get(2)?;
            let encrypted: bool = row.get(3)?;
            Ok((timestamp, base64, thumbnail_base64, encrypted))
        })
        .expect("Error retrieving image from database");

    let (timestamp, mut base64, mut thumbnail_base64, encrypted) = img_row;

    if encrypted {
        match decrypt_text(base64) {
            Ok(decrypted_base64) => {
                base64 = decrypted_base64;
            }
            Err(_) => {
                return Err(anyhow!("Error decrypting image base64"));
            }
        }
        match decrypt_text(thumbnail_base64) {
            Ok(decrypted_thumbnail_base64) => {
                thumbnail_base64 = decrypted_thumbnail_base64;
            }
            Err(_) => {
                return Err(anyhow!("Error decrypting thumbnail base64"));
            }
        }
    }

    Ok(ImageDTO::new(
        timestamp,
        String::from_utf8(base64).expect("Error converting base64 to string"),
        String::from_utf8(thumbnail_base64).expect("Error converting thumbnail base64 to string"),
    ))
}
