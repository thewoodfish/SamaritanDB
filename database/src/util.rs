// Copyright (c) 2023 Algorealm, Inc.

use crate::{cli, prelude::*};
use async_std::sync::Mutex;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::KeyInit;
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce}; // Or `XChaCha20Poly1305`
use log::{error, info};
use reqwest;
use ring::digest::{digest, SHA256};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use simplelog::{CombinedLogger, Config, LevelFilter, WriteLogger};
use sp_core_hashing::blake2_256;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    net::TcpStream,
};

lazy_static::lazy_static! {
    static ref LOGGER_INITIALIZED: Mutex<bool> = Mutex::new(false);
}

pub fn acknowledge() {
    println!("-----------------------------------------------------------------------------------------------------------");
    println!("SamaritanDB v1.0 (Chevalier)");
    println!("SamaritanDB helps you store and keep soveriegnty of your data in the most efficient way. It is a no-brainer!");
    println!("For more information, visit https://github.com/algorealmInc/samaritan-db");
    println!(
        r#"
         _____                             _ _              _____  ____
        / ____|                           (_) |            |  __ \|  _ \ 
        | (___   __ _ _ __ ___   __ _ _ __ _| |_ __ _ _ __ | |  | | |_) |
         \___ \ / _` | '_ ` _ \ / _` | '__| | __/ _` | '_ \| |  | |  _ < 
         ____) | (_| | | | | | | (_| | |  | | || (_| | | | | |__| | |_) |
        |_____/ \__,_|_| |_| |_|\__,_|_|  |_|\__\__,_|_| |_|_____/|____/ 
        "#
    );
    println!("Designed by Algorealm with 🤍.");
    println!("Copyright (c) 2023, Algorealm Inc.");
    println!("-----------------------------------------------------------------------------------------------------------");
    println!();
}

/// initialize logger
pub async fn initialize_logger() {
    // Check if the logger has already been initialized
    let mut initialized = LOGGER_INITIALIZED.lock().await;
    if *initialized {
        return;
    }

    // Open the log file in "create or append" mode
    let file = create_or_append(".resource/sam_db.log");
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        Config::default(),
        file,
    )])
    .expect_variant("could not initialize logger");

    // Set the initialized flag to true
    *initialized = true;
}

pub fn log_error(err: &str) {
    error!("{}", err);
}

pub fn log_info(data: &str) {
    info!("{}", data);
}

// Open the file or create it if it doesn't exist
pub fn open_or_create(file_path: &str) -> File {
    OpenOptions::new()
        .read(true)
        .write(true) // Add the write option to enable file creation
        .create(true)
        .open(file_path)
        .expect_variant("Failed to open or create file")
}

// Create or append to a file
pub fn create_or_append(file_path: &str) -> File {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .expect_variant("Failed to open or append to file")
}

pub fn is_ipfs_daemon_running() -> bool {
    if let Ok(stream) = TcpStream::connect("127.0.0.1:5001") {
        // Connection successful, IPFS daemon is running.
        drop(stream); // Close the connection.
        true
    } else {
        // Connection failed, IPFS daemon is not running.
        false
    }
}

pub fn write_string_to_file(content: &str, file_path: &str) -> std::io::Result<()> {
    let mut file = File::create(file_path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

// accepts a string and returns its hexadecimal format
pub fn str_to_hex(data: &str) -> String {
    format!("{}{}", "0x", hex::encode(data))
}

// parses contract return data and returns it as a human readable string
pub fn parse_contract_return_data(binding: &str) -> String {
    let output = binding.split("elems: ").skip(1).next().unwrap_or_default();
    let mut collator: Vec<u8> = Vec::new();
    // lets get all the numbers out
    let parsed = output
        .as_bytes()
        .to_vec()
        .iter()
        .filter(|&&e| e == b',' || e.is_ascii_digit())
        .map(|e| e.clone())
        .collect::<Vec<u8>>();

    let _ = String::from_utf8(parsed)
        .unwrap_or_default()
        .split(',')
        .map(|e| {
            collator.push(e.parse::<u8>().unwrap_or_default());
        })
        .collect::<()>();

    String::from_utf8(collator).unwrap_or_default()
}

// parses contract return data and returns it as a human readable string
// pub fn parse_contract_return_data(input: &str) -> String {
//     let code_list: Vec<u8> = input
//         .split("[")
//         .nth(1)
//         .and_then(|part| part.split("]").next())
//         .map(|code_str| {
//             code_str
//                 .split(",")
//                 .filter_map(|num| num.trim().parse().ok())
//                 .collect()
//         })
//         .unwrap_or_else(Vec::new);

//     let result: String = code_list.iter().map(|&code| code as char).collect();

//     result
// }

pub fn parse_multiaddresses(addresses: &str) -> HashMap<String, String> {
    addresses
        .split("$$$")
        .filter(|&s| !s.is_empty())
        .map(|address| {
            let parts: Vec<&str> = address.split("/p2p/").collect();
            match parts.len() {
                2 => (parts[1].to_string(), parts[0].to_string()),
                _ => (Default::default(), parts[0].to_string()),
            }
        })
        .collect()
}

pub fn fetch_hashtable(cid: &str) -> Result<SimpleHashTable, Error> {
    let raw_data = cli::fetch_from_ipfs(cid)?;
    // try to parse hashtable from IPFS String
    let hashtable: SimpleHashTable = serde_json::from_str(&raw_data)?;
    Ok(hashtable)
}

pub fn encrypt_data(data: &[u8]) -> Result<Vec<u8>, chacha20poly1305::Error> {
    let key = Key::from_slice(CRYPTO_KEY); // 32-bytes
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = Nonce::from_slice(CRYPTO_NONCE);
    cipher.encrypt(nonce, data)
}

pub fn decrypt_data(ciphertext: Vec<u8>) -> Result<Vec<u8>, chacha20poly1305::Error> {
    let key = Key::from_slice(CRYPTO_KEY); // 32-bytes
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = Nonce::from_slice(CRYPTO_NONCE);
    cipher.decrypt(nonce, ciphertext.as_ref())
}

// Function to read any deserializable data structure from a file
#[allow(dead_code)]
pub async fn read_from_file<T>(filename: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    // Open the file for reading
    let mut file = File::open(filename)?;

    // Read the entire file contents into a String
    let mut serialized_data = String::new();
    file.read_to_string(&mut serialized_data)?;

    // Deserialize the data from the JSON string
    let deserialized_data: T = serde_json::from_str(&serialized_data)?;

    Ok(deserialized_data)
}

/// Generate a unique signature to authenticate accounts
pub fn generate_auth_material(password: &str, secret_key: &[u8]) -> String {
    // Concatenate the password and salt to create the input for the hash function
    let input = format!("{}{}", password, CRYPTOPGRAPHIC_SALT);

    // Create a HMAC (Hash-based Message Authentication Code) using SHA-256 hash function
    let hmac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret_key);
    let hmac_value = ring::hmac::sign(&hmac_key, input.as_bytes());

    // Generate a SHA-256 hash of the HMAC value
    let hash_value = digest(&SHA256, &hmac_value.as_ref());

    // Convert the hash to a hexadecimal string representation
    let hex_hash = hash_value
        .as_ref()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<Vec<String>>()
        .join("");

    hex_hash
}

/// Function to write application data to IPFS
pub fn write_to_ipfs<T>(data: &T) -> Result<String, Error>
where
    T: Serialize,
{
    let json_data = serde_json::to_string(data)?;
    // encrypt the data for privacy
    if let Ok(encrypted_data) = encrypt_data(json_data.as_bytes()) {
        Ok(cli::write_to_ipfs(&encrypted_data)?)
    } else {
        Err(Box::new(DBError::EncryptionError))
    }
}

pub fn current_unix_epoch() -> u64 {
    let now = SystemTime::now();
    let since_epoch = now
        .duration_since(UNIX_EPOCH)
        .expect_variant("Time went backwards");
    since_epoch.as_secs()
}

pub fn send_post_request(
    url: &str,
    field_name: &str,
    cid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();

    let mut params = std::collections::HashMap::new();
    params.insert(field_name, cid);

    let response = client.post(url).json(&params).send()?;
    println!("Response:\n{}", response.text()?);

    Ok(())
}

pub fn is_contract_address(address: &str) -> Result<(), ()> {
    if address.len() != 48 {
        return Err(());
    } else {
        return Ok(());
    }
}

/// setup json error to return to database client
pub fn bake_error(err: DBError) -> serde_json::Value {
    json!({
        "error": err.to_string(),
        "status": match err {
            DBError::InitializationError => 104,
            DBError::CommandParseError => 115,
            DBError::AccessDenied => 120,
            DBError::EntryNotFound => 404,
            _ => 500
        }
    })
}

/// Setup JSON response for the DB server
pub fn bake_response(res: DBServerResponse) -> serde_json::Value {
    json!({
        "status": 200,
        "data": match res {
            DBServerResponse::Empty => json!({}),
            DBServerResponse::Exists(found) => json!({
                "exists": found,
            }),
            DBServerResponse::Get { key, value } =>
                json!({
                    "key": key,
                    "value": value,
                    "length": 1
                }),
            DBServerResponse::Keys(keys) =>
                json!({
                    "keys": keys,
                }),
        },
    })
}

/// blake2
pub fn hash_string(input: &str) -> String {
    let hash = blake2_256(input.as_bytes());
    let hex_string: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    format!("0x{}", hex_string)
}

pub fn init_hashtable(did: &str, cid: &str) -> Result<SimpleHashTable, Error> {
    // This hashtable entry is the entry containing the applications dedicated storage
    let json_data = format!(
        r#"{{
            "{did}": {{
                "cid": "{cid}",
                "access": true
            }}
        }}"#,
        did = did,
        cid = cid
    );

    let hashtable: SimpleHashTable = serde_json::from_str(&json_data)?;
    Ok(hashtable)
}

pub fn extract_dids(input: &str) -> Vec<String> {
    input
        .split("$$$")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn disjoint_strings(vec1: &Vec<String>, vec2: &Vec<String>) -> Vec<String> {
    vec2.iter()
        .filter(|item| !vec1.contains(item))
        .cloned()
        .collect()
}
