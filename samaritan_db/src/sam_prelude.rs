use std::sync::Arc;
use std::{collections::HashMap, sync::Mutex};

use crate::contract::interface;
use crate::util;
use crate::{
    ipfs::{self},
    util::*,
};

// custom defined types
pub type HashKey = u64;

pub type MetaMap = HashMap<HashKey, Metadata>;

/// The in-memory database shared amongst all clients.
///
/// This database will be shared via `Arc`, so to mutate the internal map we're
/// going to use a `Mutex` for interior mutability.
#[derive(Default)]
pub struct Database {
    /// file metadata storage
    metacache: Mutex<MetaMap>,
    /// list of authenticated DIDs
    auth_list: Mutex<HashMap<HashKey, HashKey>>,
    /// table matching files and its contents
    file_lookup_table: Mutex<HashMap<HashKey, HashMap<HashKey, String>>>,
}

/// The struct that describes behaviour of the database
#[derive(Default)]
pub struct Config {
    /// IPFS synchronization interval
    pub ipfs_sync_interval: u64,
    /// Database Metadata i.e info about database
    pub metadata: String,
    /// Key-value in-memory separator
    separator: String,
}

/// This is a tempopary data structure used to hold data breifly on initialization
#[derive(Default, Debug, Clone)]
pub struct TmpData {
    pub dids: Vec<String>,
    pub cid: String,
    pub nonce: u64,
    pub access_bit: u64,
    pub hash_key: HashKey,
    pub cache: HashMap<String, String>,
}

/// The struct containing important file information
#[derive(Default, Debug, Clone)]
pub struct Metadata {
    /// DIDs that have access to the file
    dids: [String; 2],
    /// access flags
    access_bits: [bool; 2],
    /// modified timestamp
    modified: u64,
    /// ipfs sync timestamp
    ipfs_sync_timestamp: u64,
    /// ipfs syunc nonce
    ipfs_sync_nonce: u64,
    /// deleted keys, for syncing across the network
    pub deleted_keys: Vec<String>,
}

/// Possible requests our clients can send us
pub enum Request<'a> {
    New {
        class: &'a str,
        password: &'a str,
    },
    Init {
        did: &'a str,
        password: &'a str,
    },
    Get {
        subject_did: &'a str,
        key: &'a str,
        object_did: &'a str,
    },
    Del {
        subject_did: &'a str,
        key: &'a str,
        object_did: &'a str,
    },
    Revoke {
        revoker_did: &'a str,
        app_did: &'a str,
        revoke: bool,
    },
    Insert {
        subject_did: &'a str,
        key: &'a str,
        value: String,
        object_did: &'a str,
    },
}

/// Responses to the `Request` commands above
pub enum Response {
    Single(String),
    Double {
        one: String,
        two: String,
    },
    Triple {
        one: String,
        two: String,
        three: Option<String>,
    },
    Error {
        msg: String,
    },
}

impl Database {
    /// intitializes the in-memory database
    pub fn new() -> Self {
        Default::default()
    }

    /// checks if an account exists
    pub fn account_is_alive(&self, did: &str, password: &str) -> bool {
        let guard = self.auth_list.lock().unwrap();
        match guard.get(&util::gen_hash(did)) {
            Some(pass) => {
                if util::gen_hash(password) == *pass {
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// checks if an account has been intitialized already
    pub fn account_is_auth(&self, did: &str) -> bool {
        let guard = self.auth_list.lock().unwrap();
        match guard.get(&util::gen_hash(did)) {
            Some(_) => true,
            None => {
                // check the contract
                if interface::account_exists(did) {
                    true
                } else {
                    false
                }
            }
        }
    }

    /// adds an account to the auth list
    pub fn add_auth_account(&self, did: &str, password: &str) {
        let mut guard = self.auth_list.lock().unwrap();
        guard.insert(util::gen_hash(did), util::gen_hash(password));
    }

    /// insert custom DID data into the database
    pub fn insert(
        &self,
        cfg: &Arc<Config>,
        data_key: &str,
        hashkey: HashKey,
        key: HashKey,
        value: String,
    ) -> String {
        // previous data
        let previous_data: Option<String>;
        // the unhashed key and the value are stored together
        let value = format!("{}{}{}", data_key, cfg.get_separator(), value);

        // update the file lookup table
        let mut guard = self.file_lookup_table.lock().unwrap();
        if let Some(kv_entry) = guard.get(&hashkey) {
            let mut entry = kv_entry.clone();
            previous_data = (entry).insert(key, value);

            // commit to storage
            guard.insert(hashkey, entry);
        } else {
            // create new and insert entry
            let mut kv_pair: HashMap<HashKey, String> = HashMap::new();
            previous_data = kv_pair.insert(key, value);

            // save data
            guard.insert(hashkey, kv_pair);
        }

        match previous_data {
            Some(data) => data
                .split(&cfg.get_separator())
                .skip(1)
                .next()
                .unwrap_or_default()
                .into(),
            None => "".into(),
        }
    }

    /// retreive a database entry
    pub fn get(
        &self,
        cfg: &Arc<Config>,
        file_key: HashKey,
        data_key: HashKey,
        subject_did: &str,
    ) -> Option<String> {
        // check for access permissions from the metadata
        let mut index = 0;
        let guard = self.metacache.lock().unwrap();
        if let Some(meta) = guard.get(&file_key) {
            if &meta.dids[index] == subject_did {
            } else if &meta.dids[index] == subject_did {
                index += 1;
            } else {
                return None;
            }

            // then use that index to check the access permissions
            if !meta.access_bits[index] {
                return None;
            }
        }

        // now try to read the entry if it exists
        // get the data
        let guard = self.file_lookup_table.lock().unwrap();
        if let Some(kv_store) = guard.get(&file_key) {
            if let Some(val) = (*kv_store).get(&data_key) {
                // after getting the value, split the key away from it to isolate the real value
                let value = val
                    .split(&cfg.get_separator())
                    .skip(1)
                    .next()
                    .unwrap_or("not found");
                Some(value.into())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// revoke app access to user data
    pub fn revoke(&self, file_key: HashKey, app_did: &str, revoke: bool) -> bool {
        // check for access permissions from the metadata
        let index = 0; // apps always take the first index
        let mut guard = self.metacache.lock().unwrap();
        // check if its in current memory
        if let Some(meta) = guard.get(&file_key) {
            // revoke the apps access
            let mut mdata = meta.clone();
            mdata.access_bits[index] = if revoke { false } else { true };
            guard.insert(file_key, mdata);
        }

        // revoke it as smart contract level
        interface::revoke_app_access(file_key, app_did, revoke)
    }

    /// delete an entry from the database
    pub fn del(
        &self,
        cfg: &Arc<Config>,
        string_key: &str,
        file_key: HashKey,
        data_key: HashKey,
        subject_did: &str,
    ) -> Option<String> {
        // check for access permissions from the metadata
        let mut index = 0;
        let mut mguard = self.metacache.lock().unwrap();
        if let Some(meta) = mguard.get(&file_key) {
            if &meta.dids[index] == subject_did {
            } else if &meta.dids[index] == subject_did {
                index += 1;
            } else {
                return None;
            }

            // then use that index to check the access permissions
            if !meta.access_bits[index] {
                return None;
            }
        }

        // now try to read the entry if it exists
        // get the data
        let mut guard = self.file_lookup_table.lock().unwrap();
        if let Some(kv_store_ref) = guard.get(&file_key) {
            let mut kv_store = kv_store_ref.clone();
            match kv_store.remove(&data_key) {
                Some(val) => {
                    guard.insert(file_key, kv_store);

                    // update metadata
                    if let Some(meta_ref) = mguard.get(&file_key) {
                        let mut meta = meta_ref.clone();
                        meta.deleted_keys.push(string_key.into());
                        meta.modified = get_timestamp();
                        mguard.insert(file_key, meta);
                    }

                    let value = val
                        .split(&cfg.get_separator())
                        .skip(1)
                        .next()
                        .unwrap_or("not found");
                    Some(value.into())
                }
                None => None,
            }
        } else {
            None
        }
    }

    /// get a snapshot of the database
    pub fn snapshot(&self) {
        println!("-----------------------------");
        let guard = self.metacache.lock().unwrap();
        println!("metacache -> {:#?}", guard);

        let guard = self.auth_list.lock().unwrap();
        println!("auth_list -> {:#?}", guard);

        let guard = self.file_lookup_table.lock().unwrap();
        println!("file_lookup_table -> {:#?}", guard);
    }

    /// write important file details to metadata
    pub fn write_metadata(&self, file_hk: HashKey, subject_did: &str, object_did: &str) {
        let now = get_timestamp();
        let mut guard = self.metacache.lock().unwrap();
        // set up metadata
        // first check if metadata exists {

        if let Some(m) = guard.get(&file_hk) {
            let mut meta = m.clone();
            meta.modified = now;
            // save file metadata
            guard.insert(file_hk, meta);
        } else {
            let meta = Metadata {
                dids: [subject_did.to_owned(), object_did.to_owned()],
                access_bits: [true, true],
                modified: now,
                ipfs_sync_timestamp: 0,
                ipfs_sync_nonce: 0,
                deleted_keys: Default::default(),
            };
            // save file metadata
            guard.insert(file_hk, meta);
        }
    }

    /// This will be dealing majorly with the matadata cache of the files.
    /// The smart contract would also be queried.
    pub fn sync_files(&self, cfg: &Arc<Config>) {
        let mut metadata = self.metacache.lock().unwrap().clone();
        let mut value_hashmap: HashMap<String, String> = Default::default();

        let _ = metadata
            .iter_mut()
            .map(|(hk, m)| {
                // first check the smart contract for the latest version of the file
                let file_info = interface::get_file_info(*hk);
                let ipfs_cid = file_info.1;
                let default = HashMap::<HashKey, String>::new();
                let default_meta = Metadata::new();

                let mut mguard = self.metacache.lock().unwrap();
                let metadata = mguard.get(hk).unwrap_or(&default_meta);

                // get data we want to sync
                let guard = self.file_lookup_table.lock().unwrap().clone();
                let data = guard.get(hk).unwrap_or(&default).clone();
                // Get all the string part of the data
                // They contain all the unhashed key and the value.
                let _ = data
                    .iter()
                    .map(|(_, v)| {
                        let split = cfg.get_separator();
                        let mut value = v.split(&split);

                        // get key and value of string
                        let inner_key = value.next().unwrap_or_default().into();
                        let inner_value = value.next().unwrap_or_default().into();
                        value_hashmap.insert(inner_key, inner_value);
                    })
                    .collect::<()>();
                let fresh_data: Box<HashMap<String, String>>;

                // check timestamp
                if file_info.0 != 0 {
                    // check if we have an outdated copy
                    if file_info.0 > metadata.ipfs_sync_nonce
                        || m.modified > metadata.ipfs_sync_timestamp
                    {
                        // sync data
                        fresh_data = ipfs::sync_data(
                            metadata,
                            &value_hashmap,
                            false,
                            &ipfs_cid,
                            *hk,
                            &m.dids,
                            &m.access_bits,
                        );

                        // update memory data
                        let data_collator = Self::restructure_kvstore(cfg, &fresh_data);
                        // now input the data
                        let mut guard = self.file_lookup_table.lock().unwrap();
                        guard.insert(*hk, data_collator);

                        // update metadata ipfs timestamp
                        let now = get_timestamp();

                        // save file metadata
                        match mguard.get(hk) {
                            Some(entry) => {
                                let mut meta = (*entry).clone();
                                meta.ipfs_sync_timestamp = now;
                                meta.ipfs_sync_nonce = file_info.0 + 1;
                                // clear all deleted items
                                meta.deleted_keys = Vec::new();

                                // save
                                mguard.insert(*hk, meta);
                            }
                            None => {}
                        }
                    } else {
                        // do nothing, we're up to date
                        println!("reach here !!");
                    }
                } else {
                    // push the first copy to IPFS
                    ipfs::sync_data(
                        metadata,
                        &value_hashmap,
                        true,
                        "",
                        *hk,
                        &m.dids,
                        &m.access_bits,
                    );
                    let now = get_timestamp();

                    // update ipfs sync timestamp
                    match mguard.get(hk) {
                        Some(entry) => {
                            let mut meta = (*entry).clone();
                            meta.ipfs_sync_timestamp = now;
                            meta.ipfs_sync_nonce = file_info.0 + 1; // increase nonce
                            mguard.insert(*hk, meta);
                        }
                        None => {}
                    }
                }
            })
            .collect::<()>();
    }

    /// takes a HashMap<String, String>, hashes the first as the key and combines the two as the value
    pub fn restructure_kvstore(
        cfg: &Arc<Config>,
        fresh_data: &HashMap<String, String>,
    ) -> HashMap<HashKey, String> {
        let mut data_collator = HashMap::<u64, String>::new();
        let _ = fresh_data
            .iter()
            .map(|(k, v)| {
                // hash the key
                let hashk = gen_hash(k);
                // combine them
                let combined = format!("{}{}{}", k, cfg.get_separator(), v);
                // input the datax
                data_collator.insert(hashk, combined);
            })
            .collect::<()>();

        data_collator.clone()
    }

    /// get data from IPFS and populate database
    pub fn populate_db(&self, cfg: &Arc<Config>, did: &str) {
        // get random 10 files related to the app from the smart contract
        // this enables fast initial lookup
        let sc_data = interface::get_init_files(did);
        let mut collator: Vec<TmpData> = Vec::new();
        
        // populate the vector with the necessary parsed data
        util::parse_init_data(&sc_data, &mut collator);
        // we have the CID now, so begin fetching(updating the collator internally)
        ipfs::fetch_fresh_data(&mut collator);

        // then begin writing to data-base
        let _ = collator
            .iter()
            .map(|tmp| {
                let hk = tmp.hash_key;
                let mut guard = self.file_lookup_table.lock().unwrap();
                let refined_data = Self::restructure_kvstore(cfg, &tmp.cache);
                // now insert the data
                guard.insert(hk, refined_data);
                let now = get_timestamp();

                // update metadata also
                let meta = Metadata {
                    dids: [tmp.dids[0].clone(), tmp.dids[1].clone()],
                    access_bits: [if tmp.access_bit == 0 { false } else { true }, true],
                    modified: now,
                    ipfs_sync_nonce: tmp.nonce,
                    ipfs_sync_timestamp: now,
                    deleted_keys: Default::default(),
                };

                let mut mguard = self.metacache.lock().unwrap();
                mguard.insert(hk, meta);
            })
            .collect::<()>();
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            ipfs_sync_interval: 30,                 // sync every 30 seconds
            metadata: String::new(),                // will populate this later
            separator: "*%~(@^&*$)~%*".to_string(), // complex separator
        }
    }

    pub fn get_separator(&self) -> String {
        self.separator.clone()
    }
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            dids: Default::default(),
            access_bits: Default::default(),
            modified: Default::default(),
            ipfs_sync_timestamp: Default::default(),
            ipfs_sync_nonce: Default::default(),
            deleted_keys: Default::default(),
        }
    }
}

impl<'a> Request<'a> {
    pub fn parse(input: &'a str) -> Result<Request<'a>, String> {
        let mut parts = input.split("~~");
        match parts.next() {
            Some("GET") => {
                let subject_did = match parts.next() {
                    Some(did) 
                        // the first parameter must be a DID
                        if is_did(did, "all") => {
                            did
                        },
                    _ => return Err("GET must be followed by a SamOS DID".into()),
                };

                // retreive the key
                let key = match parts.next() {
                    Some(key) => key,
                    None => return Err("a key must be specified for insertion".into()),
                };
                // check if theres a value after - Must be a user DID
                let object_did = match parts.next() {
                    Some(did) => {
                        if is_did(did, "user") && is_did(subject_did, "app") {
                            did
                        } else {
                            return Err(
                                "the last value, if present, must be a Samaritans DID".into()
                            );
                        }
                    }
                    None => "",
                };

                Ok(Request::Get {
                    subject_did,
                    key,
                    object_did,
                })
            }
            Some("DEL") => {
                let subject_did = match parts.next() {
                    Some(did) 
                        // the first parameter must be a DID
                        if is_did(did, "all") => {
                            did
                        },
                    _ => return Err("DEL must be followed by a SamOS DID".into()),
                };

                // retreive the key
                let key = match parts.next() {
                    Some(key) => key,
                    None => return Err("a key must be specified for insertion".into()),
                };
                // check if theres a value after - Must be a user DID
                let object_did = match parts.next() {
                    Some(did) => {
                        if is_did(did, "user") && is_did(subject_did, "app") {
                            did
                        } else {
                            return Err(
                                "the last value, if present, must be a Samaritans DID".into()
                            );
                        }
                    }
                    None => "",
                };

                Ok(Request::Del {
                    subject_did,
                    key,
                    object_did,
                })
            }
            Some("INSERT") => {
                let subject_did = match parts.next() {
                    Some(did) 
                        // the first parameter must be a DID
                        if is_did(did, "all") => {
                            did
                        },
                    _ => return Err("INSERT must be followed by a SamOs DID".into()),
                };
                // retreive the key
                let key = match parts.next() {
                    Some(key) => key,
                    None => return Err("a key must be specified for insertion".into()),
                };
                let value = match parts.next() {
                    Some(value) => value,
                    None => return Err("a value must be specified for insertion".into()),
                };
                // check if theres a value after - Must be a user DID
                let object_did = match parts.next() {
                    Some(did) => {
                        if is_did(did, "user") && is_did(subject_did, "app") {
                            did
                        } else {
                            return Err(
                                "the last element, if present, must be a Samaritans DID".into()
                            );
                        }
                    }
                    None => "",
                };

                Ok(Request::Insert {
                    subject_did,
                    key,
                    value: value.to_string(),
                    object_did,
                })
            }
            Some("NEW") => {
                let class = parts.next().ok_or("NEW must be followed by a type")?;
                if class != "sam" && class != "app" {
                    return Err("invalid type of user specified".into());
                }
                let password = parts
                    .next()
                    .ok_or("password must be specified after DID type")?;

                // check password length and contentx
                if password.chars().all(char::is_alphabetic)
                    || password.chars().all(char::is_numeric)
                    || password.len() < 8
                {
                    return Err("password must be aplhanumeric and more than 8 characters".into());
                }
                Ok(Request::New { class, password })
            }
            Some("REVOKE") => {
                let revoker_did = parts.next().ok_or("REVOKE must be followed by a DID")?;
                if !is_did(revoker_did, "user") {
                    return Err("REVOKE must be followed by a Samaritans DID".into());
                }

                let app_did = parts
                    .next()
                    .ok_or("an app DID must be present for revocation")?;
                if !is_did(app_did, "app") {
                    return Err("an app DID must be present for revocation".into());
                }

                Ok(Request::Revoke {
                    revoker_did,
                    app_did,
                    revoke: true,
                })
            }
            Some("UNREVOKE") => {
                let revoker_did = parts.next().ok_or("UNREVOKE must be followed by a DID")?;
                if !is_did(revoker_did, "user") {
                    return Err("UNREVOKE must be followed by a Samaritans DID".into());
                }

                let app_did = parts
                    .next()
                    .ok_or("an app DID must be present for revocation removal")?;
                if !is_did(app_did, "app") {
                    return Err("an app DID must be present for revocation removal".into());
                }

                Ok(Request::Revoke {
                    revoker_did,
                    app_did,
                    revoke: false,
                })
            }
            Some("INIT") => {
                let did = parts.next().ok_or("INIT must be followed by a DID")?;
                let password = parts.next().ok_or("password must be specified after DID")?;

                // check password length and content
                if password.chars().all(char::is_alphabetic)
                    || password.chars().all(char::is_numeric)
                    || password.len() < 8
                {
                    return Err("password must be aplhanumeric and more than 8 characters".into());
                }
                Ok(Request::Init { did, password })
            }
            Some(cmd) => Err(format!("unknown command: {}", cmd)),
            None => Err("empty input".into()),
        }
    }
}

impl Response {
    pub fn serialize(&self) -> String {
        match *self {
            Response::Single(ref one) => format!("[ok, {}]", one),
            Response::Double { ref one, ref two } => format!("[ok, {}, {}]", one, two),
            Response::Triple {
                ref one,
                ref two,
                ref three,
            } => format!(
                "[ok, {}, {}, {}]",
                one,
                two,
                three.to_owned().unwrap_or_default()
            ),
            Response::Error { ref msg } => format!("[error, {}]", msg),
        }
    }
}
