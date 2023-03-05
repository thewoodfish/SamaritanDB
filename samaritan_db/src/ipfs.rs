use crate::{
    contract::interface,
    sam_prelude::{Config, HashKey},
    util,
};
use serde_json::{self, json};
use std::{collections::HashMap, fs, process::Command, sync::Arc};

/// This file interfaces with IPFS and handles all operations involving IPFS
pub fn sync_data(
    cfg: &Arc<Config>,
    data: &HashMap<u64, String>,
    new: bool,
    ipfs_uri: &str,
    hashkey: HashKey,
    dids: &[String; 2],
    access_bits: &[bool; 2],
) -> Box<HashMap<u64, String>> {
    if new {
        // format the file to json format
        let json_data = json!(data);
        // generate random file and write to it
        let file = format!("./ipfs/{}.json", util::gen_random_str(10));
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
    }

    Default::default()
}

/// Initialize the IPFS deamon on the machine
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
