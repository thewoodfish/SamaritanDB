use std::{sync::Mutex, collections::HashMap};
use crate::{contract::SamOs, util};
use crate::util::*;

// type of our database key
pub type HashKey = u64;
pub type DIDKey = u64;
pub type FileKey = u64;

/// The in-memory database shared amongst all clients.
///
/// This database will be shared via `Arc`, so to mutate the internal map we're
/// going to use a `Mutex` for interior mutability.
#[derive(Default)]
pub struct Database {
    /// list of authenticated DIDs
    auth_list: Mutex<Vec<DIDKey>>,
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
    pub ipfs_sync_interval: u64
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

    /// checks if an account has been intitialized already
    pub fn account_is_auth(&self, did: &str) -> bool {
        let guard = self.auth_list.lock().unwrap();
        guard.contains(&util::gen_hash(did))
    }

    /// adds an account to the auth list
    pub fn add_auth_account(&self, did: &str) {
        let mut guard = self.auth_list.lock().unwrap();
        guard.push(util::gen_hash(did));
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
    pub fn get(&self, hashkey: HashKey, data_key: HashKey, subject_key: &str, object_key: &str) -> Option<String> {
        // we have to process the hashkey to check access permissions
        let key_str = format!("{}", hashkey);

        // if its an app
        let file_key = if is_did(subject_key, "app") {
            if object_key == "" {
                // if the app owns the data
                let solo_app_key_str = format_hk(&key_str, 0, 0);
                let solo_app_key = u64::from_str_radix(&solo_app_key_str, 10).unwrap_or_default();
                
                // get file key
                solo_app_key
                
            } else {
                // check that the app has access to a Samaritans data
                let combined_key_str = format_hk(&key_str, 1, 0);
                let combined_key = u64::from_str_radix(&combined_key_str, 10).unwrap_or_default();
                
                // get file key
                combined_key
            }
        } else {
            // get solo data of a Samaritan
            let solo_sam_key_str = format_hk(&key_str, 1, 0);
            let solo_sam_key = u64::from_str_radix(&solo_sam_key_str, 10).unwrap_or_default();
    
            // get file key
            solo_sam_key
        };
        
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
        let guard = self.auth_list.lock().unwrap();
        println!("auth_list -> {:#?}", guard);

        let guard = self.did_file_table.lock().unwrap();
        println!("did_file_table -> {:#?}", guard);

        let guard = self.file_lookup_table.lock().unwrap();
        println!("file_lookup_table -> {:#?}", guard);
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            contract_storage: Default::default(),
            ipfs_sync_interval: 30  // sync every 30 seconds
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
            Response::Single(ref one) => format!("ok, {}", one),
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