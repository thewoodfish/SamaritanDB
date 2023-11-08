// Copyright (c) 2023 Algorealm
// This file is a part of SamaritanDB

use serde_derive::{Deserialize, Serialize};
use serde_json::json;

use crate::contract::interface;
use crate::util;
use crate::{
    ipfs::{self},
    util::*,
};
use std::sync::Arc;
use std::{collections::HashMap, sync::Mutex};

// custom defined types
pub type HashKey = u64;

pub type MetaMap = HashMap<HashKey, Metadata>;
pub type StringHashMap = HashMap<String, String>;
// pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The in-memory database shared amongst all clients.
///
/// This database will be shared via `Arc`, so to mutate the internal map we're
/// going to use a `Mutex` for interior mutability.
#[derive(Default)]
pub struct Database {
    /// app hash table storage
    hashtable: Mutex<HashMap<HashKey, HashMap<u64, String>>>,
    /// data metadata storage
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
    /// contract address
    contract_address: String,
    /// contract keys
    contract_keys: String,
    /// number of initialization elements
    init_elements_count: u32,
}

/// The struct containing important file information
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// DIDs that have access to the file
    pub dids: [String; 2],
    /// access flags
    pub access_bits: [bool; 2],
    /// modified timestamp
    pub modified: u64,
    /// ipfs sync timestamp
    pub ipfs_sync_timestamp: u64,
    /// ipfs sync nonce
    pub ipfs_sync_nonce: u64,
    /// deleted keys, for syncing across the network
    pub deleted_keys: Vec<String>,
    /// Hashkey
    pub hashkey: u64,
}

/// The struct containing important did info esp. for syncing
#[derive(Default, Debug, Clone)]
pub struct DidMetadata {
    /// did
    pub did: String,
    /// sync timestamp
    pub ipfs_sync_timestamp: u64,
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
    Auth {
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
    GetAll {
        app_did: &'a str,
        sam_did: &'a str,
    }
}

/// Responses to the `Request` commands above
pub enum Response {
    Single(String),
    Double {
        one: String,
        two: String,
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
    pub fn account_is_auth(&self, cfg: &Arc<Config>, did: &str) -> bool {
        let guard = self.auth_list.lock().unwrap();
        match guard.get(&util::gen_hash(did)) {
            Some(_) => true,
            None => {
                // check the contract
                if interface::account_exists(cfg, did).1.len() > 7 {
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
        keys: &str,
        hashkey: HashKey,
        values: String,
    ) -> Vec<String> {
        // the keys and values can be one or more
        // we'll have to iterate over them to see
        let ikeys = keys.split(';');
        let ivalues = values.split(';');

        let str_vector: Vec<String> = ikeys.zip(ivalues).map(|(data_key, value)| {
            let key = gen_hash(data_key);
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
                Some(data) => Some(data
                    .split(&cfg.get_separator())
                    .skip(1)
                    .next()
                    .unwrap_or_default()
                    .to_owned()),
                None => None
            }
        }).flatten().collect();
        str_vector
    }

    /// retreive a database entry
    pub fn get(
        &self,
        cfg: &Arc<Config>,
        file_key: HashKey,
        keys: &str,
        subject_did: &str,
        object_did: &str,
        reloop: bool,
    ) -> Vec<String> {
        // the keys and values can be one or more
        // we'll have to iterate over them to see
        let ikeys = keys.split(';');

        let str_vector: Vec<String> = ikeys.map(|data_key| {
            // check for access permissions from the metadata
            let guard = self.metacache.lock().unwrap().clone();
            if let Some(meta) = guard.get(&file_key) {
                if &meta.dids[0] == subject_did {
                    // then use that index to check the access permissions
                    if !meta.access_bits[0] {
                        return Some("revoked".to_string())
                    }
                } else {
                    return None;
                }
            } else {
                // check the network for the file
                if reloop {
                    // get the latest image of the apps hashtable
                    // read it to get the users hashtable,
                    // then modify the apps hashtable
                    // then write the users data to memory
                    let (_, app_ht_cid) = interface::account_is_auth(&cfg, subject_did, "invalid");
                    if !app_ht_cid.is_empty() {
                        // read it and update the hashtable
                        self.populate_app_ht(subject_did, &app_ht_cid);
                        self.populate_db(cfg, subject_did);
                        // set reloop to false, so we won't create an infinite recursive loop
                        let result = self.get(&cfg, file_key, keys, subject_did, object_did, false);
                        // println!("{:#?} --> {} -> {} -> {}", result, file_key, subject_did, object_did);
                        if result.len() > 0 {
                            let total = String::new();
                            // concatenate the values in 'result' into one
                            return Some(result.iter().fold(total, |total, s| total + "%%%" + s));
                        } else {
                            return None;
                        }
                    }
                }
                return None;
            }

            let data_key = gen_hash(data_key);
            // now try to read the entry if it exists
            // get the data
            let guard = self.file_lookup_table.lock().unwrap().clone();
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
        }).flatten().collect();
        str_vector
    }

    /// retreive all entries for a DID
    pub fn get_all(
        &self,
        cfg: &Arc<Config>,
        file_key: HashKey,
        app_did: &str,
        sam_did: &str
    ) -> Option<String> {
        // check for access permissions from the metadata
        let mut index = 0;
        let guard = self.metacache.lock().unwrap().clone();
        if let Some(meta) = guard.get(&file_key) {
            if &meta.dids[index] == app_did {
            } else if &meta.dids[index] == app_did {
                index += 1;
            } else {
                return None;
            }

            // then use that index to check the access permissions
            if !meta.access_bits[index] {
                return None;
            }
        } else {
            // check the network for the file
            if !sam_did.is_empty() {
                // get the latest image of the apps hashtable
                // read it to get the users hashtable,
                // then modify the apps hashtable
                // then write the users data to memory
                let (_, app_ht_cid) = interface::account_is_auth(&cfg, app_did, "invalid");
                if !app_ht_cid.is_empty() {
                    // read it and update the hashtable
                    self.populate_app_ht(app_did, &app_ht_cid);
                    self.populate_db(cfg, app_did);
                    // set object_did to empty, so we won't create an infinite recursive loop
                    return self.get_all(&cfg, file_key, app_did, "");
                }
            } else {
                return None;
            }
        }

        // now try to read the entry if it exists
        // get the data
        let guard = self.file_lookup_table.lock().unwrap().clone();
        if let Some(kv_store) = guard.get(&file_key) {
            // our data collator
            let mut vstore: StringHashMap = Default::default();
            let separator = &cfg.get_separator();
            // we have a hashmap of u64 -> String(key, separator , value)
            let _ = kv_store.iter().map(|(_, v)| {
                let mut kv = v.split(separator);
                // hilarious 
                vstore.insert(kv.next().unwrap_or_default().to_owned(), kv.next().unwrap_or_default().to_owned());
            }).collect::<()>();

            // JSON stringify it
            Some(json_stringify(&vstore))
        } else {
            None
        }
    } 

    /// fetches and reads app hash table into memory
    pub fn populate_app_ht(&self, did: &str, app_ht_cid: &str) {
        // retreive the app hash table from IPFS
        let mut htable = HashMap::<u64, String>::new();
        let did_hash = &util::gen_hash(did);

        // populate our hashmap
        ipfs::fetch_app_ht(&mut htable, app_ht_cid);

        // insert our hashtable into memory
        let mut guard = self.hashtable.lock().unwrap();
        guard.insert(*did_hash, htable);
    }

    /// revoke app access to user data
    pub fn revoke(
        &self,
        cfg: &Arc<Config>,
        file_key: HashKey,
        app_did: &str,
        revoke: bool,
    ) -> bool {
        // check for access permissions from the metadata
        let index = 0; // apps always take the first index
        let mut guard = self.metacache.lock().unwrap();
        // check if its in current memory
        if let Some(meta) = guard.get(&file_key) {
            // revoke the apps access
            let mut mdata = meta.clone();
            mdata.modified = get_timestamp();
            mdata.access_bits[index] = if revoke { false } else { true };
            guard.insert(file_key, mdata);
        }

        // revoke it as smart contract level
        interface::revoke_app_access(cfg, file_key, app_did, revoke)
    }

    /// delete an entry from the database
    pub fn del(
        &self,
        cfg: &Arc<Config>,
        file_key: HashKey,
        keys: &str,
        subject_did: &str,
        object_did: &str,
    ) -> Vec<String> {
        // we'll have to iterate over them to see
        let ikeys = keys.split(';');

        let str_vector: Vec<String> = ikeys.map(|str_key| {
            // check for access permissions from the metadata
            let mut mguard = self.metacache.lock().unwrap();
            if let Some(meta) = mguard.get(&file_key) {
                if &meta.dids[0] == subject_did {
                    // then use that index to check the access permissions
                    if !meta.access_bits[0] {
                        return Some("revoked".to_string())
                    }
                } else {
                    return None;
                }
            } else {
                // check the network for the file
                if !object_did.is_empty() {
                    // get the latest image of the apps hashtable
                    // read it to get the users hashtable,
                    // then modify the apps hashtable
                    // then write the users data to memory
                    // then perform delete
                    let (_, app_ht_cid) = interface::account_is_auth(&cfg, subject_did, "invalid");
                    if !app_ht_cid.is_empty() {
                        // read it and update the hashtable
                        self.populate_app_ht(subject_did, &app_ht_cid);
                        self.populate_db(cfg, subject_did);
                        // set object_did to empty, so we won't create an infinite recursive loop
                        let result =  self.del(&cfg, file_key, keys, subject_did, "");
                        if result.len() > 0 {
                            let total = String::new();
                            // concatenate the values in 'result' into one
                            return Some(result.iter().fold(total, |total, s| total + "%%%" + s));
                        } else {
                            return None;
                        }
                    }
                } else {
                    return None;
                }
                return None;
            }

            let data_key = gen_hash(str_key);
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
                            meta.deleted_keys.push(str_key.into());
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
        }).flatten().collect();
        str_vector
    }

    /// get a snapshot of the database
    #[allow(dead_code)]
    pub fn snapshot(&self) {
        println!("-----------------------------");
        let guard = self.hashtable.lock().unwrap();
        println!("HashTable -> {:#?}", guard);
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
                hashkey: file_hk,
            };
            // save file metadata
            guard.insert(file_hk, meta);
        }
    }

    /// Fetch data that apps hold about a samaritan.
    /// This would be the the data likely to be read
    pub fn fetch_sam_data(&self, cfg: &Arc<Config>, did: &str) {
        // read data from the hash table
        let did_hash = &util::gen_hash(did);
        let guard = self.hashtable.lock().unwrap();
        let _ = guard
            .iter()
            .map(|(_, hashtable)| {
                match hashtable.get(did_hash) {
                    Some(cid) => {
                        let mut data = HashMap::<String, String>::new();
                        // metadata container
                        let mut meta = Metadata::new();
                        // fetch data and metadata from IPFS
                        ipfs::fetch_data(&mut data, &mut meta, cid);

                        if data.len() != 0 {
                            // write to storage now
                            let mut guard = self.file_lookup_table.lock().unwrap();
                            let refined_data = Self::restructure_kvstore(cfg, &data);
                            guard.insert(meta.hashkey, refined_data);

                            // write metadata
                            let mut mguard = self.metacache.lock().unwrap();
                            mguard.insert(meta.hashkey, meta);
                        }
                    }
                    None => {}
                }
            })
            .collect::<()>();
    }

    /// Sync the hashtable of the app and/or user
    pub fn sync_hashable(&self, cfg: &Arc<Config>, cid: &str, dids: &[String]) {
        // update the hashtable
        let mut index = 0;
        let _ = dids
            .iter()
            .map(|did| {
                let did_hash = util::gen_hash(did);
                let mut hguard = self.hashtable.lock().unwrap();
                match hguard.get(&did_hash) {
                    Some(htable) => {
                        // if its its own
                        let mut nhtable = htable.clone();
                        if dids[1].is_empty() {
                            nhtable.insert(util::gen_hash("root"), cid.to_owned());
                        } else {
                            nhtable.insert(
                                if index == 0 {
                                    util::gen_hash(&dids[1])
                                } else {
                                    util::gen_hash(&dids[0])
                                },
                                cid.to_owned(),
                            );
                        }

                        // save table
                        hguard.insert(did_hash, nhtable.clone());

                        // update hashtable
                        ipfs::sync_hashtable(cfg, did, &mut nhtable);
                    }
                    None => {
                        // create new entry
                        // if its its own
                        let mut nhtable = HashMap::<u64, String>::new();
                        if dids[1].is_empty() {
                            nhtable.insert(util::gen_hash("root"), cid.to_owned());
                        } else {
                            nhtable.insert(
                                if index == 0 {
                                    util::gen_hash(&dids[1])
                                } else {
                                    util::gen_hash(&dids[0])
                                },
                                cid.to_owned(),
                            );
                        }

                        // save table
                        hguard.insert(did_hash, nhtable.clone());

                        // update hashtable
                        ipfs::sync_hashtable(cfg, did, &mut nhtable);
                    }
                }
                index += 1;
            })
            .collect::<()>();
    }

    /// This function queries the contract and synchronizes the database
    pub fn sync_files(&self, cfg: &Arc<Config>) {
        let mut meta = self.metacache.lock().unwrap();
        let _: () = meta
            .iter_mut()
            .map(|(hk, metadata)| {
                let mut value_hashmap: StringHashMap = Default::default();

                // first check the smart contract for the latest version of the file
                let file_info = interface::get_file_info(cfg, *hk);
                let ipfs_cid = file_info.1;
                let default = HashMap::<HashKey, String>::new();

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

                let fresh_data: Box<StringHashMap>;

                // check timestamp
                if file_info.0 != 0 && ipfs_cid.len() > 7 {
                    // check if we have an outdated copy
                    if file_info.0 > metadata.ipfs_sync_nonce
                    // pull
                    {
                        // sync data
                        let returned_data =
                            ipfs::sync_data(cfg, metadata, &value_hashmap, false, &ipfs_cid, *hk);

                        fresh_data = returned_data.1;

                        // update memory data
                        let data_collator = Self::restructure_kvstore(cfg, &fresh_data);
                        // now input the data
                        let mut guard = self.file_lookup_table.lock().unwrap();
                        guard.insert(*hk, data_collator);

                        // update metadata ipfs timestamp
                        let now = get_timestamp();

                        // save file metadata
                        metadata.ipfs_sync_timestamp = now;
                        metadata.ipfs_sync_nonce = file_info.0;

                        // sync hashtable
                        if !returned_data.0.is_empty() {
                            self.sync_hashable(cfg, &returned_data.0, &metadata.dids);
                        }
                    } else if metadata.modified > metadata.ipfs_sync_timestamp {
                        // push
                        let returned_data =
                            ipfs::sync_data(cfg, metadata, &value_hashmap, true, &ipfs_cid, *hk);

                        // save file metadata
                        metadata.ipfs_sync_timestamp = metadata.modified;
                        metadata.ipfs_sync_nonce = file_info.0;

                        // sync since you're pushing
                        self.sync_hashable(cfg, &returned_data.0, &metadata.dids);
                    } else {
                        // println!("no show");
                    }
                } else {
                    // push
                    let returned_data =
                        ipfs::sync_data(cfg, metadata, &value_hashmap, true, "", *hk);

                    // save file metadata
                    metadata.ipfs_sync_timestamp = metadata.modified;
                    metadata.ipfs_sync_nonce = file_info.0;

                    // sync since you're pushing
                    self.sync_hashable(cfg, &returned_data.0, &metadata.dids);
                }
            })
            .collect();
    }

    /// takes a StringHashMap, hashes the first as the key and combines the two as the value
    pub fn restructure_kvstore(
        cfg: &Arc<Config>,
        fresh_data: &StringHashMap,
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
        // get the root file + some random files specified in the hash table
        let hashtable = self.hashtable.lock().unwrap();
        let empty_hm: HashMap<u64, String> = Default::default();
        // read the hashtable
        let htable = hashtable.get(&util::gen_hash(did)).unwrap_or(&empty_hm);
        // counter
        let mut index = 0;
        let limit = cfg.get_init_data_count();
        // select a number of CIDs to fetch their data, which increases chances of a hit
        let _ = htable
            .iter()
            .map(|(_, cid)| {
                if index < limit {
                    // data container
                    let mut data = HashMap::<String, String>::new();
                    // metadata container
                    let mut meta = Metadata::new();
                    // fetch data and metadata from IPFS
                    ipfs::fetch_data(&mut data, &mut meta, cid);
                    if data.len() != 0 {
                        // write to storage now
                        let mut guard = self.file_lookup_table.lock().unwrap();
                        let refined_data = Self::restructure_kvstore(cfg, &data);
                        guard.insert(meta.hashkey, refined_data);

                        // write metadata
                        let mut mguard = self.metacache.lock().unwrap();
                        mguard.insert(meta.hashkey, meta);
                    }
                }

                index += 1;
            })
            .collect::<()>();
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            ipfs_sync_interval: 5,                  // sync every 30 seconds
            metadata: String::new(),                // will populate this later
            separator: "*%~(@^&*$)~%*".to_string(), // complex separator
            contract_address: "5FRBJwwhFbe6gkRTNuEiECDGodUXuPioafZmBgrWHrGfUPTN".to_string(), // contract address
            // contract_keys: "blast they annual column pave umbrella again olympic exotic vibrant lemon visa".to_string(),     // contract key
            contract_keys: "//Alice".to_string(), // for test
            init_elements_count: 15,
        }
    }

    pub fn get_separator(&self) -> String {
        self.separator.clone()
    }

    pub fn get_contract_address(&self) -> String {
        self.contract_address.clone()
    }

    pub fn get_contract_keys(&self) -> String {
        self.contract_keys.clone()
    }

    pub fn get_init_data_count(&self) -> u32 {
        self.init_elements_count
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
            hashkey: Default::default(),
        }
    }
}

impl DidMetadata {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            did: Default::default(),
            ipfs_sync_timestamp: Default::default(),
        }
    }
}

impl<'a> Request<'a> {
    pub fn parse(input: &'a str) -> Result<Request<'a>, String> {
        let mut parts = input.split("::");
        match parts.next() {
            Some("GET" | "get") => {
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
            Some("DEL" | "del") => {
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
            Some("INSERT" | "insert") => {
                let subject_did = match parts.next() {
                    Some(did) 
                        // the first parameter must be a DID
                        if is_did(did, "all") => {
                            did 
                        },
                    _ => return Err("INSERT must be followed by a SamOS DID".into()),
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
                        if is_did(did, "user")/* && is_did(subject_did, "app") */ {
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
            Some("NEW" | "new") => {
                let class = parts.next().ok_or("`new` must be followed by a type")?;
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
            Some("REVOKE" | "revoke") => {
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
            Some("UNREVOKE" | "unrevoke") => {
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
            Some("GETALL" | "getall") => {
                let app_did = match parts.next() {
                    Some(did) 
                        // the first parameter must be a DID
                        if is_did(did, "all") => {
                            did
                        },
                    _ => return Err("GET must be followed by a SamOS DID".into()),
                };

                // check if the a samaritans DID is appended
                let sam_did = match parts.next() {
                    Some(did) => {
                        if is_did(did, "user") {
                            did
                        } else {
                            return Err(
                                "the last value, if present, must be a Samaritans DID".into()
                            );
                        }
                    }
                    None => "",
                };

                Ok(Request::GetAll { app_did, sam_did })
            }
            Some("INIT" | "init") => {
                let did = parts
                    .next()
                    .ok_or("`init` must be followed by an applications DID")?;
                if !is_did(did, "app") {
                    return Err("the DID must be an applications DID".into());
                } // only applications must be initialized

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
            Some("AUTH" | "auth") => {
                let did = parts
                    .next()
                    .ok_or("`init` must be followed by an samaritans DID")?;
                if !is_did(did, "user") {
                    return Err("the DID must be a samaritans DID".into());
                } // only samaritans should be authenticated

                let password = parts.next().ok_or("password must be specified after DID")?;

                Ok(Request::Auth { did, password })
            }
            Some(cmd) => Err(format!("unknown command: {}", cmd)),
            None => Err("empty input".into()),
        }
    }
}

impl Response {
    pub fn serialize(&self) -> String {
        match *self {
            Response::Single(ref one) => serde_json::to_string(&json!({
                "status": "ok",
                "data": {"0": one},
            }))
            .unwrap_or_default(),
            Response::Double { ref one, ref two } => serde_json::to_string(&json!({
                "status": "ok",
                "data": {"0": one, "1": two }
            }))
            .unwrap_or_default(),
            Response::Error { ref msg } => serde_json::to_string(&json!({
                "status": "error",
                "data": { "msg": msg }
            }))
            .unwrap_or_default(),
        }
    }
}