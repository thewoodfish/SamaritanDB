use crate::{
    contract::interface,
    sam_prelude::{Config, HashKey},
    util,
};
use serde_json::{self, json, Value};
use std::{collections::HashMap, fs, process::Command, sync::Arc};

/// This function interfaces with IPFS and handles all operations involving IPFS
pub fn sync_data(
    cfg: &Arc<Config>,
    db_data: &HashMap<String, String>,
    new: bool,
    cid: &str,
    hashkey: HashKey,
    dids: &[String; 2],
    access_bits: &[bool; 2],
) -> Box<HashMap<String, String>> {
    // format the file to json format
    let json_data = json!(db_data);
    // generate random file and write to it
    let file = format!("./ipfs/{}.json", util::gen_random_str(10));

    if new {
        // write to file
        fs::write(&file, serde_json::to_string(&json_data).unwrap_or_default())
            .expect("Failed to create file");

        // initiate IPFS upload
        let (success, cid) = upload_to_ipfs(&file);
        if success && cid.len() > 2 {
            // delete tmp file
            fs::remove_file(&file).unwrap_or_default();
            // update the state of the smart contract
            interface::update_file_meta(&cfg, &cid, hashkey, dids, access_bits);
        }
    } else {
        // first pull from IPFS
        let (success, ipfs_data) = pull_from_ipfs(cid);
        if success && ipfs_data.len() > 2 {
            // format data
            let formatted_data: Value = serde_json::from_str(&ipfs_data).unwrap_or(Value::Null);
            if formatted_data != Value::Null {
                // add the current data in the database
                let kv_map_result: Result<HashMap<String, String>, serde_json::Error> =
                    serde_json::from_value(formatted_data);
                let mut kv_map = kv_map_result.unwrap_or_default();

                // combine this kv_map with the result from the database
                let _ = (*db_data).iter().map(|(k, v)| {
                    kv_map.insert(k.clone(), v.clone());
                }).collect::<()>();

                // encode as JSON
                let encoded_data = json!(kv_map);
                if kv_map.len() > 0 {
                    // write to file
                    fs::write(
                        &file,
                        serde_json::to_string(&encoded_data).unwrap_or_default(),
                    )
                    .expect("Failed to create file");

                    // initiate IPFS upload
                    let (success, cid) = upload_to_ipfs(&file);
                    if success && cid.len() > 2 {
                        // delete tmp file
                        fs::remove_file(&file).unwrap_or_default();
                        // update the state of the smart contract
                        interface::update_file_meta(&cfg, &cid, hashkey, dids, access_bits);

                        // return
                        return Box::new(kv_map);
                    }
                }
            }
        }
    }
    Default::default()
}

/// fetch fresh IPFS data
pub fn fetch_fresh_data(
    sc_data: &Vec<(HashKey, String)>,
) -> Box<Vec<(HashKey, HashMap<String, String>)>> {
    // read data
    let def = HashMap::<String, String>::new();
    let collator = sc_data
        .iter()
        .map(|(hk, s)| {
            // get the file
            let (success, ipfs_data) = pull_from_ipfs(s);
            if success {
                // parse file content and turn it into a key-value hashmap
                let parsed_data: Value = serde_json::from_str(&ipfs_data).unwrap_or(Value::Null);
                if parsed_data != Value::Null {
                    let kv_map_result: Result<HashMap<String, String>, serde_json::Error> =
                        serde_json::from_value(parsed_data);
                    let kv_map = kv_map_result.unwrap_or_default();
                    (hk.clone(), kv_map)
                } else {
                    (0u64, def.clone())
                }
            } else {
                (0u64, def.clone())
            }
        })
        .filter(|hk| hk.0 != 0)
        .collect();
    Box::new(collator)
}

/// accepts a file and pull data from IPFS into it
fn pull_from_ipfs(cid: &str) -> (bool, String) {
    let output = Command::new("ipfs")
        .args(["cat", cid])
        .output()
        .expect("failed to read CID from node");

    let result = String::from_utf8_lossy(&output.stdout);
    println!("ipfs file download, cid: {}", cid);

    (output.status.success(), result.into())
}

/// Initialize the IPFS deamon on the machine
#[allow(dead_code)]
pub fn init_ipfs() -> bool {
    let output = Command::new("ipfs")
        .arg("daemon")
        .current_dir("./ipfs")
        .output()
        .expect("failed to start IPFS daemon");

    output.status.success()
}

/// read file and upload to IPFS
fn upload_to_ipfs(path: &str) -> (bool, String) {
    let output = Command::new("ipfs")
        .args(["add", path])
        .output()
        .expect("failed to upload file to node");

    let binding = String::from_utf8_lossy(&output.stdout);
    let mut ipfs_result = binding.split(' ');
    ipfs_result.next();

    let cid = ipfs_result.next().unwrap_or_default();
    println!("ipfs file upload, cid: {}", cid);

    (output.status.success(), cid.to_owned())
}
