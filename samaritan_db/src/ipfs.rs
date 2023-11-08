// Copyright (c) 2023 Algorealm
// This file is a part of SamaritanDb

use crate::{
    contract::interface,
    sam_prelude::{Config, HashKey, Metadata, StringHashMap},
    util,
};
use serde_json::{self, json, Value};
use std::{collections::HashMap, fs, process::Command, sync::Arc};

/// This function interfaces with IPFS and handles all operations involving IPFS
pub fn sync_data(
    cfg: &Arc<Config>,
    meta: &mut Metadata,
    db_data: &StringHashMap,
    push: bool,
    cid: &str,
    hashkey: HashKey,
) -> (String, Box<StringHashMap>) {
    // generate random file and write to it
    let file = format!("./{}.json", util::gen_random_str(10));
    let mut _file_cid = String::new();
    // first pull from ipfs, if its not the first time
    let (mut success, mut ipfs_data) = (true, String::from("{}"));
    if !push {
        (success, ipfs_data) = pull_from_ipfs(cid);
    }

    if success {
        // format data
        let formatted_data: Value = serde_json::from_str(&ipfs_data).unwrap_or(Value::Null);
        if formatted_data != Value::Null {
            // get the data currently true on the network
            let data_result: Result<HashMap<String, Value>, serde_json::Error> =
                serde_json::from_value(formatted_data);
            let result = data_result.unwrap_or_default();
            // extract what we need
            let data_value = result.get("data").unwrap_or(&Value::Null);
            let meta_value = result.get("metadata").unwrap_or(&Value::Null).clone();

            // extract data
            let mut data: StringHashMap =
                serde_json::from_value(data_value.clone()).unwrap_or_default();

            // push or pull will decide our actions
            if push {
                // our data will take precedence
                let _ = (*db_data)
                    .iter()
                    .map(|(k, v)| {
                        data.insert(k.clone(), v.clone());
                    })
                    .collect::<()>();

                // enforce the deleted keys
                let _ = meta
                    .deleted_keys
                    .iter()
                    .map(|k| {
                        data.remove(k);
                    })
                    .collect::<()>();

                // update meta
                meta.deleted_keys = Vec::new();

                // data we want to upload, the metadata goes with it
                let json_data = json!({
                    "metadata": meta,
                    "data": data
                });

                if data.len() > 0 {
                    // write to file
                    fs::write(&file, serde_json::to_string(&json_data).unwrap_or_default())
                        .expect("Failed to create file");

                    // initiate IPFS upload
                    let (success, cid) = upload_to_ipfs(&file);
                    if success && cid.len() > 2 {
                        // delete tmp file
                        fs::remove_file(&file).unwrap_or_default();
                        // update the state of the smart contract
                        interface::update_file_meta(
                            cfg,
                            &cid,
                            hashkey,
                            &meta.dids,
                            &meta.access_bits,
                        );

                        return (cid.to_string(), Box::new(data));
                    }
                }
            } else {
                // extract metadata
                let mdata: Metadata = serde_json::from_value(meta_value.clone()).unwrap_or_default();
                // update access data
                meta.access_bits = mdata.access_bits;
            }

            return (cid.to_string(), Box::new(data));
        }
    }
    (_file_cid, Default::default())
}

/// fetch data and metadata of file from IPFS
pub fn fetch_data(data: &mut StringHashMap, meta: &mut Metadata, cid: &str) {
    let (success, ipfs_data) = pull_from_ipfs(cid);
    if success && ipfs_data.len() > 2 {
        // fetch from IPFS
        let formatted_data: Value = serde_json::from_str(&ipfs_data).unwrap_or(Value::Null);
        if formatted_data != Value::Null {
            // add the current data in the database
            let data_result: Result<HashMap<String, Value>, serde_json::Error> =
                serde_json::from_value(formatted_data);

            let result = data_result.unwrap_or_default();

            // extract what we need
            let data_value = result.get("data").unwrap_or(&Value::Null);
            let meta_value = result.get("metadata").unwrap_or(&Value::Null).clone();

            // extract data
            *data = serde_json::from_value(data_value.clone()).unwrap_or_default();

            // extract metadata
            *meta = serde_json::from_value(meta_value.clone()).unwrap_or_default();
        }
    }
}

/// accepts a files cid and pull data from IPFS into it
fn pull_from_ipfs(cid: &str) -> (bool, String) {
    // remove the rouge null byte
    let ncid = if &cid[cid.len().saturating_sub(1)..] == "\0" {
        &cid[..cid.len() - 1]
    } else {
        cid
    };

    let output = Command::new("ipfs")
        .args(["cat", ncid])
        .output()
        .expect("failed to read data from node");

    let result = String::from_utf8_lossy(&output.stdout);
    println!("ipfs file download, cid: {}", cid);

    (output.status.success(), result.into())
}

/// Initialize the IPFS deamon on the machine
#[allow(dead_code)]
pub fn init_ipfs() -> bool {
    let output = Command::new("ipfs")
        .arg("daemon")
        .current_dir(".")
        .output()
        .expect("failed to start IPFS daemon");

    output.status.success()
}

/// Creates hash table for user at signup
pub fn set_up_ht() -> (bool, String) {
    // upload empty file first
    let file = format!("./{}.json", util::gen_random_str(10));
    let data = json!({});

    // we'll upload this to IPFS and get its CID
    fs::write(&file, serde_json::to_string(&data).unwrap_or_default())
        .expect("Failed to create file");

    // initiate IPFS upload
    let (success, cid) = upload_to_ipfs(&file);
    if success {
        // delete tmp file
        fs::remove_file(&file).unwrap_or_default();
        (success, cid)
    } else {
        (false, Default::default())
    }
}

/// read file and upload to IPFS
pub fn upload_to_ipfs(path: &str) -> (bool, String) {
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

/// fetch app hash table
pub fn fetch_app_ht(htable: &mut HashMap<u64, String>, app_ht_cid: &str) {
    let (success, ipfs_data) = pull_from_ipfs(app_ht_cid);
    if success && ipfs_data.len() > 2 {
        // fetch from IPFS
        let formatted_data: Value = serde_json::from_str(&ipfs_data).unwrap_or(Value::Null);
        if formatted_data != Value::Null {
            // add the current data in the database
            let ht_result: Result<StringHashMap, serde_json::Error> =
                serde_json::from_value(formatted_data);

            let parsed_data = ht_result.unwrap_or_default();
            let _ = parsed_data
                .iter()
                .map(|(k, v)| {
                    let u64_key = k.parse::<u64>().unwrap_or_default();
                    (*htable).insert(u64_key, v.clone());
                })
                .collect::<()>();
        }
    }
}

/// sync the users hsahtable
pub fn sync_hashtable(cfg: &Arc<Config>, did: &str, table: &mut HashMap<u64, String>) {
    // format the file to json format
    let json_data = json!(table);
    // generate random file and write to it
    let file = format!("./{}.json", util::gen_random_str(10));
    // write to file
    fs::write(&file, serde_json::to_string(&json_data).unwrap_or_default())
        .expect("Failed to create file");

    // initiate IPFS upload
    let (success, cid) = upload_to_ipfs(&file);
    if success && cid.len() > 2 {
        // delete tmp file
        fs::remove_file(&file).unwrap_or_default();
        // update the state of the smart contract
        interface::update_hashtable(cfg, &cid, &did);
    }
}