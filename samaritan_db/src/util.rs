use crate::sam_prelude::*;
use std::{
    hash::Hasher,
    time::{SystemTime, UNIX_EPOCH},
};

use fnv::FnvHasher;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::Serialize;
use serde_json::{json, Value};

/// generate a did for the user
pub fn get_did(class: &str) -> String {
    let mut _did = String::with_capacity(60);
    let random_str = gen_random_str(43);
    if class == "sam" {
        _did = format!("did:sam:root:DS{random_str}");
    } else {
        _did = format!("did:sam:apps:DS{random_str}");
    }
    _did
}

/// generate random number of alphanumerics
pub fn gen_random_str(n: u32) -> String {
    let r = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(n as usize)
        // .map(|mut e| {
        //     let a = 32;
        //     if e.is_ascii_alphabetic() {
        //         e = e & !a;
        //     }
        //     e
        // })
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&r).into()
}

/// generate a blake2 hash of input
pub fn gen_hash(input: &str) -> HashKey {
    let mut hasher = FnvHasher::default();

    hasher.write(input.as_bytes());
    hasher.finish()
}

pub fn is_did(value: &str, class: &str) -> bool {
    if class == "all" {
        value.contains("did:sam:root") || value.contains("did:sam:apps")
    } else {
        if class == "user" {
            value.contains("did:sam:root")
        } else {
            value.contains("did:sam:apps")
        }
    }
}

#[allow(dead_code)]
fn json_parse<T: Sized + Serialize>(value: T) -> Value {
    json!(value)
}

#[allow(dead_code)]
fn json_stringify<T: Sized + Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[allow(dead_code)]
/// Removes the last two numbers of the hashkey and replace it with the respecpective binaries.
/// If bin_one == 0, it means the DID owns the file.
/// If bin_one == 0, it means the DID has access to the file
pub fn format_hk<'a>(hk: &'a str, bin_one: u32, bin_two: u32) -> String {
    let length = hk.len() - 2;
    format!("{}{}{}", &hk[0..length], bin_one, bin_two)
}

/// calculates the hashkey based on its input
pub fn get_hashkey(subject_did: &str, object_did: &str) -> HashKey {
    let hash_key: HashKey;
    if object_did == "" {
        hash_key = gen_hash(subject_did);
        // let hashkey_buf = format_hk(&format!("{hk}"), 0, 0);
        // hash_key = u64::from_str_radix(&hk, 10).unwrap_or_default();
    } else {
        // combine the object and subject to form the hashkey
        hash_key = gen_hash(&format!("{}{}", subject_did, object_did));
        // let hashkey_buf = format_hk(&format!("{hk}"), 1, 0); // app has access but did owns the data
        // hash_key = u64::from_str_radix(&hk, 10).unwrap_or_default();
    }
    hash_key
}

/// return now, as in this instant as a Unix Timestamp
pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// parse initialation data
pub fn parse_init_data(data: &(String, String), collator: &mut Vec<TmpData>) {
    let (basic_info, extra_info) = data;
    let mut index = 1; // skip first guy
    let _ = basic_info
        .split("####")
        .map(|chunk| {
            if !chunk.is_empty() {
                let num_str = extra_info
                    .split(",,")
                    .skip(index)
                    .next()
                    .unwrap_or_default();

                if !num_str.is_empty() {
                    let mut num = num_str.split(",");
                    let mut s = chunk.split("##");
                    let dids = s.next().unwrap_or("--");
                    let nonce = num.next().unwrap_or("0");
                    let access_bit = num.next().unwrap_or("0");
                    let cid = s.next().unwrap_or_default().to_string();
                    let hk = num.next().unwrap_or_default();

                    let dids = dids
                        .split("--")
                        .map(|d| d.to_owned())
                        .filter(|did| did != "did:sam:root:apps:xxxxxxxxxxxx")
                        .collect();

                    // populate
                    let tmp = TmpData {
                        dids,
                        cid,
                        nonce: nonce.parse::<u64>().unwrap_or(0),
                        access_bit: access_bit.parse::<u64>().unwrap_or(0),
                        hash_key: hk.parse::<HashKey>().unwrap_or(0),
                        cache: Default::default(),
                    };
                    collator.push(tmp);
                }
            }
            index += 1;
        })
        .collect::<()>();

    //     did:sam:root:jhjbsfgiusif78uwn9s980h9--did:sam:root:apps:xxxxxxxxxxxx##QmsiunSlonNdhuIjNHuUtfkhjOijoho####did:sam:root:jhjbsfgiusif78uwn9s980h9
    // --did:sam:root:apps:xxxxxxxxxxxx##QmsiunSlonNdhuIjNHuUtfkhjOijoho####
    // ,,1,0,1,,1,0,1
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

// accepts a string and returns its hexadecimal format
pub fn str_to_hex(data: &str) -> String {
    format!("{}{}", "0x", hex::encode(data))
}

/// parse contract boolean data
pub fn parse_contract_boolean(data: &str) -> bool {
    let io = data.split("Bool(").skip(1).next().unwrap_or_default();
    io.chars().nth(0).unwrap_or('f') == 't'
}

/// Parses the first integreg(u64) in a tuple
pub fn parse_first_tuple_u64(binding: &str) -> u64 {
    let output = binding.split("elems: ").next().unwrap_or_default();
    // lets the number out
    let parsed = output
        .as_bytes()
        .to_vec()
        .iter()
        .filter(|&&e| e.is_ascii_digit())
        .map(|e| e.clone())
        .collect::<Vec<u8>>();

    let str = String::from_utf8(parsed).unwrap_or_default();
    str.parse::<u64>().unwrap_or_default()
}

/// Parses the tuple vector gotten from the contract
pub fn parse_contract_tuple_vector(binding: &str) -> String {
    let parsed = binding
        .as_bytes()
        .to_vec()
        .iter()
        .filter(|&&e| e == b',' || e.is_ascii_digit())
        .map(|e| e.clone())
        .collect::<Vec<u8>>();

    String::from_utf8(parsed).unwrap_or_default()
}
