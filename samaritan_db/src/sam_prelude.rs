use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{sync::Mutex, collections::HashMap};

use crate::contract::interface;
use crate::{contract::SamOs, util};
use crate::{
    util::*,
    ipfs::{self}
};

// custom defined types
pub type HashKey = u64;
pub type DIDKey = u64;
pub type FileKey = u64;
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
    /// table that maps did to the files they own
    did_file_table: Mutex<HashMap<DIDKey, FileKey>>,
    /// table matching files and its contents
    file_lookup_table: Mutex<HashMap<FileKey, HashMap<HashKey, String>>>
}

/// The struct that describes behaviour of the database
#[derive(Default)]       
pub struct Config {
    /// contract storage
    pub contract_storage: Mutex<SamOs>,
    /// IPFS synchronization interval
    pub ipfs_sync_interval: u64,
    /// Database Metadata
    pub metadata: String
}

/// The struct containing important file information
#[derive(Default, Debug)]       
pub struct Metadata {
    /// DIDs that have access to the file
    dids: [String; 2],
    /// access flags
    access_bits: [bool; 2],
    /// modified timestamp
    modified: u64,
}

/// Possible requests our clients can send us
pub enum Request<'a> {
    New { class: &'a str, password: &'a str },
    Init { did: &'a str, password: &'a str },
    Get { 
        subject_did: &'a str,
        key: &'a str, 
        object_did: &'a str
    },
    Insert { 
        subject_did: &'a str,
        key: &'a str, 
        value: String,
        object_did: &'a str
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
            },
            None => false
        }
    }

    /// checks if an account has been intitialized already
    pub fn account_is_auth(&self, did: &str) -> bool {
        let guard = self.auth_list.lock().unwrap();
        match guard.get(&util::gen_hash(did)) {
            Some(_) => {
                true
            },
            None => false
        }
    }

    /// adds an account to the auth list
    pub fn add_auth_account(&self, did: &str, password: &str) {
        let mut guard = self.auth_list.lock().unwrap();
        guard.insert(util::gen_hash(did), util::gen_hash(password));
    }

    /// insert custom DID data into the database
    pub fn insert(&self, subject_key: HashKey, object_key: HashKey, hashkey: HashKey, key: HashKey, value: String) -> String {
        let mut guard = self.did_file_table.lock().unwrap();
        // update the did file table entry
        guard.entry(subject_key).or_insert(hashkey);
        // do same for object did, if there is any
        if object_key != 0 {
            guard.entry(object_key).or_insert(hashkey);
        }

        // previous data
        let previous_data: Option<String>;

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
        previous_data.unwrap_or_default()
    }

    /// retreive a database entry
    pub fn get(&self, file_key: HashKey, data_key: HashKey, subject_did: &str) -> Option<String> {
        // check for access permissions from the metadata
        let mut index = 0;
        let guard = self.metacache.lock().unwrap();
        if let Some(meta) = guard.get(&file_key) {
            if &meta.dids[index] == subject_did {}
            else if &meta.dids[index] == subject_did {
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
                Some(val.clone())
            } else {
                None
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

        let guard = self.did_file_table.lock().unwrap();
        println!("did_file_table -> {:#?}", guard);

        let guard = self.file_lookup_table.lock().unwrap();
        println!("file_lookup_table -> {:#?}", guard);
        println!("-----------------------------");
    }

    /// write to important file details to metadata
    pub fn write_metadata(&self, file_hk: HashKey, subject_did: &str, object_did: &str) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        // set up metadata
        let meta = Metadata {
            dids: [subject_did.to_owned(), object_did.to_owned()],
            access_bits: [true, true],
            modified: now
        };

        // save file metadata
        let mut guard = self.metacache.lock().unwrap();
        guard.insert(file_hk, meta);
    }

    // This will be dealing majorly with the matadata cache of the files.
    // The smart contract would also be queried.
    pub fn sync_files(&self, cfg: &Arc<Config>) {
        let mut metadata = self.metacache.lock().unwrap();
        let ret = metadata.iter_mut()
        .map(|m| {
            // first check the smart contract for the latest version of the file
            let file_info = interface::get_file_info(cfg, *m.0);
            let ipfs_cid = file_info.1;
            let default = HashMap::<u64, String>::new();

            // get data we want to sync
            let guard = self.file_lookup_table.lock().unwrap();
            let data = guard.get(m.0).unwrap_or(&default);
            let fresh_data: Box<HashMap::<u64, String>>;

            println!("yes sir");

            // check timestamp
            if file_info.0 != 0 {
                // check if we have an outdated copy
                if file_info.0 > m.1.modified {
                    // sync data
                    fresh_data = ipfs::sync_data(&cfg, data, false, &ipfs_cid, *m.0, &m.1.dids, &m.1.access_bits);
                } else {
                    // do nothing, we're up to date
                }
            } else {
                // push the first copy to IPFS
                ipfs::sync_data(&cfg, data, true, "", *m.0, &m.1.dids, &m.1.access_bits);
            }
            String::new()
        }).collect::<Vec<String>>();
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            contract_storage: Default::default(),
            ipfs_sync_interval: 30,  // sync every 30 seconds
            metadata: String::new()     // will populate this later
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
                    _ => return Err("INSERT must be followed by a SamOs DID".into()),
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
                            return Err("the last value, if present, must be a Samaritans DID".into())
                        }
                    },
                    None => "",
                };

                Ok(Request::Get {
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
                            return Err("the last element, if present, must be a Samaritans DID".into())
                        }
                    },
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

                // check password length and content
                if password.chars().all(char::is_alphabetic)
                    || password.chars().all(char::is_numeric)
                    || password.len() < 8
                {
                    return Err("password must be aplhanumeric and more than 8 characters".into());
                }
                Ok(Request::New { class, password })
            },
            Some("INIT") => {
                let did = parts.next().ok_or("INIT must be followed by a DID")?;
                let password = parts
                    .next()
                    .ok_or("password must be specified after DID")?;

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