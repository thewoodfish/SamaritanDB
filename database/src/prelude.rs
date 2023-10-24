// Copyright (c) 2023 Algorealm, Inc.

use async_std::sync::RwLock;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::SinkExt;
use serde_json::Value;
use std::collections::HashMap;
use std::error;
use std::fmt;
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::cli;
use crate::contract;
use crate::network;
use crate::network::ChannelMsg;
use crate::network::WriteDataType;
use crate::util;
use crate::util::current_unix_epoch;
use serde::{Deserialize, Serialize};

/// Generic result type
pub type DBResult<T> = Result<T, DBError>;
/// Hash table type
pub type HashTable = HashMap<String, HashTableData>;
/// Simple Hash table type
pub type SimpleHashTable = HashMap<String, SimpleHashTableData>;

/// config file
pub const CONFIG_FILE: &str = ".resource/conf.json";
/// protocol version for identify
pub const PROTOCOL_VERSION: &str = "/samaritan-os/0.1.0";
/// Symmetric encryption key
pub const CRYPTO_KEY: &[u8; 32] = b"Do not be afraid,for i am with u"; // 32-bytes
/// Crypto Nonce
pub const CRYPTO_NONCE: &[u8; 12] = b"GUD&*DY*&TC(";
/// Our salt for generating identifiers
pub const CRYPTOPGRAPHIC_SALT: &str = "Adedeji&Woodfish";
/// Cryptographic secret key
pub const SECRET_KEY: &[u8] =
    b"I knew you before I formed you in the womb; I set you apart for me before you were born";
/// This atomic flag is to prevent race conditions and possible loss of data when we're still intializing the database and SET is run
static INITIALIZATION_COMPLETE: AtomicBool = AtomicBool::new(false);
/// Duration to wait before retrying on recieving a CLI error
pub const CLI_RETRY_DURATION: Duration = Duration::from_secs(5);

/// Generic error type
pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
/// Database server result type
pub type DBServerResult = Result<Value, Value>;

/// Custom error types
#[derive(Debug)]
#[allow(dead_code)]
pub enum DBError {
    AuthenticationError,
    AccessDenied,
    EntryNotFound,
    InvalidCommand,
    CommandParseError,
    DecryptionError,
    EncryptionError,
    IPFSReadError,
    InvalidPasswordError,
    InitializationError,
    ParseError,
}

// Implement the `Error` trait for `DBError`
impl error::Error for DBError {}

// Implement the `Display` trait for `DBError`
impl fmt::Display for DBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DBError::AuthenticationError => write!(f, "Client authenticated failed."),
            DBError::AccessDenied => write!(f, "Access to data entry denied."),
            DBError::EntryNotFound => write!(f, "Entry not found in database."),
            DBError::InvalidCommand => write!(f, "Unrecognized command specified."),
            DBError::CommandParseError => write!(f, "You have an error in your command syntax."),
            DBError::DecryptionError => write!(f, "Failed to decrypt file."),
            DBError::EncryptionError => write!(f, "Failed to encrypt file."),
            DBError::IPFSReadError => write!(f, "Failed to read meaningful data from IPFS."),
            DBError::InvalidPasswordError => write!(f, "Incorrect mnemonic composition."),
            DBError::InitializationError => write!(f, "Failed to initialize application"),
            DBError::ParseError => write!(f, "Parse operation failed"),
        }
    }
}

/// A single unit of data
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DataUnit {
    data: String,
    access: bool,
    pub write_time: u64,
}

/// This is the representation of the internal data storage structure: key -> { value, access? }
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DataMap {
    #[serde(flatten)]
    pub data_map: HashMap<String, DataUnit>,
}

impl DataMap {
    // insert key -> value pair
    pub fn insert(&mut self, key: String, value: String) {
        let mut data_unit = if let Some(data_unit) = self.data_map.get(&key) {
            data_unit.to_owned()
        } else {
            Default::default()
        };

        // set value
        data_unit.data = value;
        data_unit.access = true;
        data_unit.write_time = util::current_unix_epoch();
        // save to memory
        self.data_map.insert(key, data_unit);
    }

    // get a reference to the whole data for an hashtable entry
    pub fn get_entries(&self) -> &HashMap<String, DataUnit> {
        &self.data_map
    }

    // delete a key-value entry
    pub fn delete_data_entry(&mut self, key: &str) {
        self.data_map.remove(key);
    }

    // get data value
    pub fn get_data(&self, key: &str) -> Option<&String> {
        if let Some(data_unit) = self.data_map.get(key) {
            Some(&data_unit.data)
        } else {
            None
        }
    }

    // get data access state
    pub fn get_access_state(&self, key: &str) -> Option<bool> {
        if let Some(data_unit) = self.data_map.get(key) {
            Some(data_unit.access)
        } else {
            None
        }
    }

    // return the keys from some offset
    pub fn get_keys(&self, offset: usize, length: usize) -> Vec<String> {
        self.data_map
            .iter()
            .skip(offset)
            .take(length)
            .map(|(key, _)| key.to_owned())
            .collect()
    }

    // clear all data entries in map
    pub fn clear(&mut self) {
        self.data_map.clear()
    }
}

/// Enum representing database server responses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum DBServerResponse {
    Empty,
    Exists(bool),
    Get { key: String, value: String },
    Keys(String),
}

/// Cache Data
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CacheData {
    // DID we are storing data about (Optional)
    pub sam_did: Option<String>,
    pub value: String,
    pub write_time: u64,
}

/// Cache to store temporary data
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TempCache {
    // key -> Data
    entries: HashMap<String, CacheData>,
}

impl TempCache {
    pub fn insert_data(
        &mut self,
        sam_did: Option<String>,
        key: String,
        value: String,
        write_time: u64,
    ) {
        let data = CacheData {
            sam_did,
            value,
            write_time,
        };
        self.entries.insert(key, data);
    }
}

/// Hashtable data
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HashTableData {
    // The DID of user
    pub did: String,
    // The location of the data on IPFS
    pub cid: String,
    // Data access bit
    pub access: bool,
    // Data the hashtable points to
    data: DataMap,
    // Latest time the access setting was modified
    access_modification_time: u64,
}

/// Hashtable data we need across nodes
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SimpleHashTableData {
    // The location of the data on IPFS
    pub cid: String,
    // Data access bit
    pub access: bool,
}

impl From<HashTableData> for SimpleHashTableData {
    fn from(data: HashTableData) -> Self {
        SimpleHashTableData {
            cid: data.cid,
            access: data.access,
        }
    }
}

impl From<SimpleHashTableData> for HashTableData {
    fn from(simple_data: SimpleHashTableData) -> Self {
        HashTableData {
            did: String::default(),
            cid: simple_data.cid,
            access: simple_data.access,
            data: DataMap::default(),
            access_modification_time: 0,
        }
    }
}

impl HashTableData {
    // set data the hashtable entry points to
    pub fn set_data(&mut self, data: DataMap) {
        self.data = data;
    }

    // check if data exists in memory already
    pub fn is_empty(&self) -> bool {
        self.data.data_map.is_empty()
    }

    pub fn get_data(&self) -> &DataMap {
        &self.data
    }
}

/// Application state of the database
#[derive(Clone, Debug, Default)]
pub struct DBState {
    // DID: { Hash (DID(data owner)): Main Data }
    applications: HashMap<String, HashMap<String, HashTableData>>,
    cache: HashMap<String, TempCache>,
    // every authenticated application hashed mnemonic is saved here to compare with the ones from the server request
    pub authenticated: HashMap<String, String>,
}

impl DBState {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_application(&mut self, did: String, hashtable: HashTable) {
        self.applications.insert(did, hashtable);
    }

    pub fn remove_application(&mut self, did: &String) {
        self.applications.remove(did);
    }

    pub fn is_application_init(&self, did: &str) -> bool {
        self.applications.get(did).is_some()
    }

    // return reference to application data
    pub fn get_application_data(&self, did: &str) -> Option<&HashMap<String, HashTableData>> {
        self.applications.get(did)
    }
}

// Trait to log error and stop database instead of panicking, if error
pub trait ExpectVariant<T, E> {
    fn expect_variant(self, message: &str) -> T;
}

impl<T, E: std::fmt::Debug> ExpectVariant<T, E> for Result<T, E> {
    fn expect_variant(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(_) => {
                util::log_error(message);
                process::exit(1);
            }
        }
    }
}

impl<T> ExpectVariant<T, &'static str> for Option<T> {
    fn expect_variant(self, message: &str) -> T {
        match self {
            Some(value) => value,
            None => {
                util::log_error(message);
                process::exit(1);
            }
        }
    }
}

/// Editable configuration for database
#[derive(Default, Serialize, Deserialize, Clone)]
pub struct DBConfig {
    protobuf_keypair: Vec<u8>,
    tcp_port: u16,
    chain_keys: String,
    contract_address: String,
    server_address_port: u16,
}

impl DBConfig {
    pub fn new() -> Self {
        DBConfig {
            protobuf_keypair: Default::default(),
            tcp_port: 1509,
            chain_keys: Default::default(),
            contract_address: Default::default(),
            server_address_port: Default::default(),
        }
    }

    pub fn get_proto_keypair(&self) -> &[u8] {
        &self.protobuf_keypair[..]
    }

    pub fn get_tcp_port(&self) -> u16 {
        self.tcp_port
    }

    pub fn get_chain_keys(&self) -> &str {
        &self.chain_keys
    }

    pub fn get_contract_addr(&self) -> &str {
        &self.contract_address
    }

    pub fn get_server_port(&self) -> u16 {
        self.server_address_port
    }
}

/// This enumeration contains the parsed commands gotten from the CLI
#[derive(Debug)]
pub enum Command<'a> {
    Info,
    Quit,
    Help,
    Invalid,
    Init {
        did: &'a str,
        mnemonic: &'a str,
    },
    Set {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
        key: &'a str,
        value: &'a str,
    },
    Get {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
        key: &'a str,
    },
    Del {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
        key: &'a str,
    },
    Exists {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
        key: &'a str,
    },
    Keys {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
        offset: Option<usize>,
        length: Option<usize>,
    },
    Truncate {
        did: &'a str,
        // samaritan DID. For user, we're storing data about (Optional)
        sam_did: Option<&'a str>,
    },
    Leave {
        did: &'a str,
        mnemonic: &'a str,
    },
    Config {
        did: &'a str,
        flag: &'a str,
        param: &'a str,
    },
}

impl<'a> Command<'a> {
    pub fn parse(&self) -> Command {
        let result: DBResult<Command> = match self {
            Command::Info => Ok(Command::Info),
            Command::Help => Ok(Command::Help),
            Command::Quit => Ok(Command::Quit),
            Command::Init { did, mnemonic } => {
                // first check if the did is correct
                if Self::is_did(did) {
                    if Self::is_valid_mnemonic(mnemonic) {
                        Ok(Command::Init {
                            did: Command::remove_cli_prefix(did),
                            mnemonic,
                        })
                    } else {
                        Err(DBError::InvalidPasswordError)
                    }
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Set {
                did,
                sam_did,
                key,
                value,
            } => {
                if ((Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default())))
                    && Command::is_not_empty(key)
                    && Command::is_not_empty(value)
                {
                    Ok(Command::Set {
                        did,
                        sam_did: *sam_did,
                        key,
                        value,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Get { did, sam_did, key } => {
                if ((Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default())))
                    && Command::is_not_empty(key)
                {
                    Ok(Command::Get {
                        did,
                        sam_did: *sam_did,
                        key,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Exists { did, sam_did, key } => {
                if ((Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default())))
                    && Command::is_not_empty(key)
                {
                    Ok(Command::Exists {
                        did,
                        sam_did: *sam_did,
                        key,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Del { did, sam_did, key } => {
                if ((Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default())))
                    && Command::is_not_empty(key)
                {
                    Ok(Command::Del {
                        did,
                        sam_did: *sam_did,
                        key,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Keys {
                did,
                sam_did,
                offset,
                length,
            } => {
                if (Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default()))
                {
                    Ok(Command::Keys {
                        did,
                        sam_did: *sam_did,
                        offset: *offset,
                        length: *length,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Truncate { did, sam_did } => {
                if (Self::is_did(did) && (*sam_did).is_none())
                    || (Self::is_did(&sam_did.map(|s| s.to_string()).unwrap_or_default()))
                {
                    Ok(Command::Truncate {
                        did,
                        sam_did: *sam_did,
                    })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Leave { did, mnemonic } => {
                // first check if the did is correct
                if Self::is_did(did) {
                    if Self::is_valid_mnemonic(mnemonic) {
                        Ok(Command::Leave { did, mnemonic })
                    } else {
                        Err(DBError::InvalidPasswordError)
                    }
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            Command::Config { did, flag, param } => {
                if Self::is_did(did) && flag.starts_with("-") && !param.is_empty() {
                    Ok(Command::Config { did, flag, param })
                } else {
                    Err(DBError::CommandParseError)
                }
            }
            _ => Err(DBError::CommandParseError),
        };

        match result {
            Ok(cmd) => cmd,
            Err(e) => {
                println!("~ {}", e);
                Command::Invalid
            }
        }
    }

    #[inline]
    fn is_did(did: &str) -> bool {
        Self::remove_cli_prefix(did).starts_with("did:sam:root:")
            || Self::remove_cli_prefix(did).starts_with("did:sam:apps:")
    }

    #[inline]
    fn remove_cli_prefix(arg: &str) -> &str {
        arg.split("=").last().unwrap_or_default()
    }

    #[inline]
    fn is_not_empty(str: &str) -> bool {
        str.len() > 0
    }

    // Function to check if a mnemonic is valid
    #[inline]
    fn is_valid_mnemonic(mnemonic: &str) -> bool {
        // Check if the mnemonic is at least 8 characters long
        if mnemonic.split("_").count() != 12 {
            return false;
        }

        return true;
    }

    /// function to fire various executors | functions
    pub async fn execute(
        self,
        cfg: Arc<DBConfig>,
        net_client: mpsc::Sender<ChannelMsg>,
        state: Arc<RwLock<DBState>>,
    ) -> serde_json::Value {
        match self {
            Command::Info => {
                async_std::task::block_on(get_db_info(net_client));
                serde_json::Value::Null
            }
            Command::Help => {
                async_std::task::block_on(display_db_cmds());
                serde_json::Value::Null
            }
            Command::Quit => {
                async_std::task::block_on(shutdown_db(cfg, net_client));
                serde_json::Value::Null
            }
            Command::Init { did, mnemonic } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let mnemonic = mnemonic.to_owned();
                async_std::task::block_on(init_application(cfg, did, mnemonic, state, net_client));
                serde_json::Value::Null
            }
            Command::Set {
                did,
                sam_did,
                key,
                value,
            } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());
                let key = key.to_owned();
                let value = value.to_owned();

                // spin up task and wait for it
                match async_std::task::block_on(set_db_value(
                    cfg, net_client, state, did, sam_did, key, value,
                )) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Get { did, key, sam_did } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());
                let key = key.to_owned();

                // spin up task and wait for it
                match async_std::task::block_on(get_db_value(state, did, sam_did, key)) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Del { did, key, sam_did } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());
                let key = key.to_owned();

                // spin up task and wait for it
                match async_std::task::block_on(del_db_value(
                    net_client, cfg, state, did, sam_did, key,
                )) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Exists { did, key, sam_did } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());
                let key = key.to_owned();

                // spin up task and wait for it
                match async_std::task::block_on(check_db_key_existence(state, did, sam_did, key)) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Keys {
                did,
                sam_did,
                offset,
                length,
            } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());
                let offset = offset.unwrap_or(0);
                let length = length.unwrap_or(50);

                // spin up task and wait for it
                match async_std::task::block_on(get_db_keys(state, did, sam_did, offset, length)) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Truncate { did, sam_did } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let sam_did = sam_did.map(|s| s.to_string());

                // spin up task and wait for it
                match async_std::task::block_on(truncate_db_keys(
                    net_client, cfg, state, did, sam_did,
                )) {
                    Ok(json_val) => json_val,
                    Err(err) => err,
                }
            }
            Command::Leave { did, mnemonic } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let mnemonic = mnemonic.to_owned();
                async_std::task::block_on(forget_application(
                    cfg, did, mnemonic, state, net_client,
                ));
                Value::Null
            }
            Command::Config { did, flag, param } => {
                // compiler thinks self may expire while the task is still running
                let did = did.to_owned();
                let flag = flag.to_owned();
                let param = param.to_owned();

                // spin up task and wait for it
                async_std::task::block_on(change_db_cfg(net_client, did, flag, param));
                Value::Null
            }
            Command::Invalid => Value::Null,
        }
    }
}

/// Fetch all the data from IPFS, make them available and read some into memory
async fn localize_application_data(
    net_channel: mpsc::Sender<ChannelMsg>,
    did: String,
    state: Arc<RwLock<DBState>>,
) {
    // Mark initialization phase
    INITIALIZATION_COMPLETE.store(false, Ordering::SeqCst);

    // first we fetch all data for hashtable entries in memory
    let mut entries = {
        let mut state_lock = state.write().await;
        state_lock
            .applications
            .get_mut(&did)
            .unwrap_or(&mut Default::default())
            .clone()
    };

    for (_, ht_entry) in &mut entries {
        // fetch from IPFS to memory and disk outside the lock
        let data = match cli::fetch_from_ipfs(&ht_entry.cid) {
            Ok(data) => data,
            Err(_) => continue, // Handle error as needed
        };

        // convert string into useful format
        let data_map: DataMap = serde_json::from_str(&data).unwrap_or_default();
        ht_entry.set_data(data_map.clone());
    }

    {
        let mut state_lock = state.write().await;
        // Update the entries in the state after modifications
        state_lock.applications.insert(did.clone(), entries);
    }

    // update the network state for IPFS synching
    let _ = net_channel
        .clone()
        .send(ChannelMsg::SetNetworkSyncFlag(did.clone()))
        .await;

    util::log_info(&format!(
        "data for application: {did} successfully written to memory"
    ));
    // Now, update the atomic flag to enable writing of data to the database and not the temporary cache
    INITIALIZATION_COMPLETE.store(true, Ordering::SeqCst);
    // Also, quickly read all the data stored in the cache into the database, if any
    flush_cache(net_channel, &did, state.clone()).await;
}

/// In case data was written when initialization was taking place, flush them all into memory
pub async fn flush_cache(
    net_channel: mpsc::Sender<ChannelMsg>,
    did: &str,
    db_state: Arc<RwLock<DBState>>,
) {
    // select entry in cache
    let default = TempCache::default();
    let cache = db_state
        .read()
        .await
        .cache
        .get(did)
        .unwrap_or(&default)
        .to_owned();

    if !cache.entries.is_empty() {
        let _ = cache
            .entries
            .iter()
            .map(|(key, cache_data)| async {
                // store data relating to a particular samaritan or the app itself
                let cache_data = cache_data.to_owned();
                let owner_did = cache_data.sam_did.unwrap_or(did.to_string());
                let mut state_lock = db_state.write().await;
                if let Some(application_data) = state_lock.applications.get(did) {
                    let mut app_data = application_data.to_owned();
                    // if entry exists already, just update it
                    if let Some(ref_data) = app_data.get(&owner_did) {
                        let mut data = ref_data.to_owned();
                        data.data.insert(key.clone(), cache_data.value.clone());
                        // save the data
                        app_data.insert(owner_did.clone(), data);
                        state_lock.applications.insert(did.to_owned(), app_data);
                    } else {
                        // create new entry
                        let mut data_entry: HashMap<String, DataUnit> = HashMap::new();
                        let data = DataUnit {
                            data: cache_data.value.clone(),
                            access: true,
                            write_time: current_unix_epoch(),
                        };
                        data_entry.insert(key.clone(), data);

                        let ht_data = HashTableData {
                            did: owner_did.clone(),
                            cid: String::new(), // no cid yet
                            access: true,
                            data: DataMap {
                                data_map: data_entry,
                            },
                            access_modification_time: 0,
                        };

                        // save data
                        let mut app_data: HashMap<String, HashTableData> = HashMap::new();
                        app_data.insert(owner_did.clone(), ht_data);
                        state_lock.applications.insert(did.to_owned(), app_data);
                    }

                    // explicitly drop lock
                    drop(state_lock);
                    // update the network state for IPFS synching
                    let _ = net_channel
                        .clone()
                        .send(ChannelMsg::SetNetworkSyncFlag(did.to_owned()))
                        .await;
                }
            })
            .collect::<Vec<_>>();

        // now delete cache
        db_state.write().await.cache.clear();
    }
}

/// This function initializes an application, fetches its data from IPFS and caches it locally on this machine, for it to be read into memory.
/// It then subscribes to the 'DID Topic' and begins to participate in the network
pub async fn init_application(
    cfg: Arc<DBConfig>,
    did: String,
    mnemonic: String,
    state: Arc<RwLock<DBState>>,
    net_client: mpsc::Sender<ChannelMsg>,
) {
    println!("~ adding application to database: {did}...");
    util::log_info(&format!("adding application to database: {did}..."));

    // try to authenticate details provided
    let auth_material = util::hash_string(&mnemonic);
    // we want to fetch the hashtable CID from the contract, this contains all links to the applications data
    // authentication also takes place automatically
    let hashtable_cid = contract::get_app_ht_cid(&cfg, &did, &auth_material).await;
    if hashtable_cid.len() > 9 {
        // fetch it from IPFS
        if let Ok(simple_hashtable) = util::fetch_hashtable(&hashtable_cid) {
            // convert simple hashtable to hashtable proper
            let hashtable = simple_hashtable
                .iter()
                .map(|(did, data)| {
                    let mut data: HashTableData = data.to_owned().into();
                    data.did = did.to_owned();
                    (did.to_owned(), data)
                })
                .collect::<HashTable>();

            // Next, add the did to the database state, so the network manager would pick it up
            let mut guard = state.write().await;
            guard.add_application(did.clone(), hashtable.into());

            // spin up task to retrieve data and localize it
            let net_channel = net_client.clone();
            async_std::task::spawn(localize_application_data(
                net_channel,
                did.clone(),
                state.clone(),
            ));
        } else {
            util::log_error("failed to get IPFS initialization CID");
            return println!("~ Error: could not add application to database.");
        }
    } else {
        // let's upload some data, so we can initialize the applications hashtable properly
        let mut data = DataMap::default();
        data.insert("protocol".to_string(), PROTOCOL_VERSION.to_string());

        // Upload it to IPFS to get its CID
        if let Ok(cid) = util::write_to_ipfs(&data) {
            // Write the account's first hashtable entry
            if let Ok(simple_hashtable) = util::init_hashtable(&did, &cid) {
                let hashtable = simple_hashtable
                    .iter()
                    .map(|(did, data)| {
                        let mut data: HashTableData = data.to_owned().into();
                        data.did = did.to_owned();
                        (did.to_owned(), data)
                    })
                    .collect::<HashTable>();

                // initialize hashtable into memory
                state.write().await.add_application(did.clone(), hashtable);
            }
        } else {
            util::log_error("failed to get IPFS initialization CID");
            return println!("~ Error: could not add application to database.");
        }

        // Now, update the atomic flag to enable writing of data to the database and not the temporary cache
        INITIALIZATION_COMPLETE.store(true, Ordering::SeqCst);
    }

    if state.read().await.is_application_init(&did) {
        // Now we move to the real deal: Networking
        // We want to subscribe the node to the {did} topic and begin recieving and sending updates of data concerning the application
        // We will create a one way channel and wait for the reply from the channel, then we exit
        let (sender, receiver) = oneshot::channel();
        let _ = net_client
            .clone()
            .send(network::ChannelMsg::SubscribeNodeToApplication {
                did: did.clone(),
                state: state.clone(),
                cfg: cfg.clone(),
                hashtable_cid,
                sender,
                ipfs_helper_channel: net_client,
            })
            .await;

        // now we block on it
        if let Ok(chanel_data) = receiver.await {
            if let Ok(msg) = chanel_data {
                // save the auth details
                state.write().await.authenticated.insert(did, auth_material);
                println!("~ {msg}");
            }
        }
    } else {
        // Oops! We got here
        util::log_error("authentication details provided failed to match.");
        println!("~ Error: could not add application to database.");
    }
}

/// This function handles the gossiped data received from peers and tries to add it to our local state
pub async fn handle_gossiped_data(
    db_state: Arc<RwLock<DBState>>,
    did: String,
    key: String,
    value: String,
    owner_did: String,
    write_time: String,
    write_type: String,
) {
    // lets get the write type, this would inform our actions
    let write_type = write_type.parse::<u8>().unwrap_or_default();
    // we're definitely subscribed to this application
    // lets find the entry the message is about
    let mut app_data = db_state
        .write()
        .await
        .applications
        .entry(did.clone())
        .or_default()
        .to_owned();

    let msg_type = match write_type {
        0 => WriteDataType::Access,
        1 => WriteDataType::Delete,
        2 => WriteDataType::Truncate,
        3 => WriteDataType::Write,
        _ => WriteDataType::Unknown,
    };

    match msg_type {
        WriteDataType::Access => {
            // change the access conditions based on the data recieved
            // key => permission
            let perm = key;
            if let Some(hashtable) = app_data.get_mut(&owner_did) {
                // check the time
                if write_time.parse::<u64>().unwrap_or_default()
                    > hashtable.access_modification_time
                {
                    hashtable.access = perm.parse::<bool>().unwrap_or_default();
                    hashtable.access_modification_time = util::current_unix_epoch();

                    db_state.write().await.applications.insert(did, app_data);
                }
            }
        }
        WriteDataType::Write => {
            if let Some(hashtable) = app_data.get(&owner_did) {
                // get the data the hashtable points to
                let data_unit = hashtable
                    .get_data()
                    .data_map
                    .get(&key)
                    .unwrap_or(&Default::default())
                    .to_owned();

                // quickly compare their modified timestamps
                if write_time.parse::<u64>().unwrap_or_default() > data_unit.write_time {
                    // update/create the data
                    let mut data_map = hashtable.get_data().to_owned();
                    data_map.insert(key, value);
                    // update hashtable
                    let mut htable = hashtable.to_owned();
                    htable.data = data_map;
                    // save the application data
                    app_data.insert(owner_did, htable);
                    // save to database state
                    db_state.write().await.applications.insert(did, app_data);
                }
            } else {
                let mut data_entry: HashMap<String, DataUnit> = HashMap::new();
                let data = DataUnit {
                    data: value.clone(),
                    access: true,
                    write_time: util::current_unix_epoch(),
                };
                data_entry.insert(key.clone(), data);

                let ht_data = HashTableData {
                    did: owner_did.clone(),
                    cid: String::new(), // no cid yet
                    access: true,
                    data: DataMap {
                        data_map: data_entry,
                    },
                    access_modification_time: 0,
                };

                // save data
                let mut app_data: HashMap<String, HashTableData> = HashMap::new();
                app_data.insert(owner_did.clone(), ht_data);
                db_state
                    .write()
                    .await
                    .applications
                    .insert(did.clone(), app_data);
            }
        }
        WriteDataType::Delete => {
            if let Some(hashtable) = app_data.get_mut(&owner_did) {
                // Get the data_unit from the data_map
                if let Some(data_unit) = hashtable.data.data_map.get(&key) {
                    // Quickly compare their modified timestamps
                    if write_time.parse::<u64>().unwrap_or_default() > data_unit.write_time {
                        // Delete the data entry from data_map
                        hashtable.data.data_map.remove(&key);

                        // Save the application data
                        db_state.write().await.applications.insert(did, app_data);
                    }
                }
            }
        }
        WriteDataType::Truncate => {
            if let Some(hashtable) = app_data.get_mut(&owner_did) {
                // Filter out entries that are earlier than the timestamp
                hashtable.data.data_map.retain(|_, data_unit| {
                    data_unit.write_time >= write_time.parse::<u64>().unwrap_or_default()
                });

                // Update hashtable in place
                hashtable.data.data_map.shrink_to_fit();

                // Save to database state
                db_state.write().await.applications.insert(did, app_data);
            }
        }
        WriteDataType::Unknown => todo!(),
    }

    util::log_info("local data updated to sync with network state");
}

/// Display most important information about the database
pub async fn get_db_info(net_client: mpsc::Sender<ChannelMsg>) {
    if let Ok(_) = net_client.clone().send(network::ChannelMsg::DBInfo).await {
        return;
    }

    // we shoouldn't reach here
    println!("~ Failed to return database information");
}

/// Perform the necessary operations and then shut down
pub async fn shutdown_db(cfg: Arc<DBConfig>, net_client: mpsc::Sender<ChannelMsg>) {
    // We just need to notify all other peers running the applications of our nodes descision to quit
    // All data would be removed from memory when we kill the database process

    // We will create a one way channel and wait for the reply from the channel, then we exit
    let (sender, receiver) = oneshot::channel();
    let _ = net_client
        .clone()
        .send(network::ChannelMsg::Shutdown { cfg, sender })
        .await;

    // now we block on it
    if let Ok(chanel_data) = receiver.await {
        if let Ok(msg) = chanel_data {
            println!("~ {msg}");
            println!("~ Database is offline");

            // shutdown
            process::exit(0);
        }
    }

    // we shoouldn't reach here
    println!("~ Failed to complete database shutdown");
}

/// Insert a value into the database and communicate this change to sister nodes that care about the application
pub async fn set_db_value(
    cfg: Arc<DBConfig>,
    mut net_client: mpsc::Sender<ChannelMsg>,
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
    key: String,
    value: String,
) -> DBServerResult {
    // First, make sure the application that owns the DID has been initializd on the machine
    if let None = db_state.read().await.applications.get(&did) {
        util::log_error(&format!(
            "attempting to write data for uninitialized application: {did}"
        ));

        println!("~ Initialize application: `{did}` before writing data");
        return Err(util::bake_error(DBError::InitializationError));
    }

    let now = util::current_unix_epoch();
    // get owner DID
    let owner_did = sam_did.clone().unwrap_or(did.clone());
    // Then check to see if initialization is going on, so we can write to the cache and continue
    if !INITIALIZATION_COMPLETE.load(Ordering::SeqCst) {
        // write to cache
        let new_cache = TempCache::default();
        let mut app_cache_entry = db_state
            .read()
            .await
            .cache
            .get(&did)
            .unwrap_or(&new_cache)
            .to_owned();

        app_cache_entry.insert_data(sam_did, key.clone(), value.clone(), now);
        // save cache
        db_state
            .write()
            .await
            .cache
            .insert(did.clone(), app_cache_entry);
    } else {
        // write directly to memory, if permitted
        let mut app_data = db_state
            .read()
            .await
            .applications
            .get(&did)
            .unwrap_or(&Default::default())
            .to_owned();

        // if entry exists already, just update it
        if let Some(ref_data) = app_data.get(&owner_did) {
            let mut data = ref_data.to_owned();
            data.data.insert(key.clone(), value.clone());
            // save the data
            app_data.insert(owner_did.clone(), data);
            db_state
                .write()
                .await
                .applications
                .insert(did.clone(), app_data.clone());
        } else {
            // create new entry
            let mut data_entry: HashMap<String, DataUnit> = HashMap::new();
            let data = DataUnit {
                data: value.clone(),
                access: true,
                write_time: util::current_unix_epoch(),
            };
            data_entry.insert(key.clone(), data);

            let ht_data = HashTableData {
                did: owner_did.clone(),
                cid: String::new(), // no cid yet
                access: true,
                data: DataMap {
                    data_map: data_entry,
                },
                access_modification_time: 0,
            };

            // save data
            app_data.insert(owner_did.clone(), ht_data);
            db_state
                .write()
                .await
                .applications
                .insert(did.clone(), app_data.clone());
        }

        // update the network state for IPFS synching
        let _ = net_client
            .clone()
            .send(ChannelMsg::SetNetworkSyncFlag(did.to_owned()))
            .await;
    }

    // gossip to peers
    let _ = net_client
        .send(network::ChannelMsg::GossipWriteData {
            did,
            key, 
            value,
            owner_did,
            write_time: now,
            cfg,
        })
        .await;
    println!("~ Write operation successful.");

    // send to db-client, if any
    Ok(util::bake_response(DBServerResponse::Empty))
}

/// Retrieve value from database
pub async fn get_db_value(
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
    key: String,
) -> DBServerResult {
    // First, check the database cache
    if let Some(app_cache) = db_state.read().await.cache.get(&did) {
        // check for the key
        if let Some(cache_entry) = app_cache.entries.get(&key) {
            // check for owner
            if cache_entry.sam_did == sam_did {
                // we've found what we're looking for
                println!("~ {}: {}", key, cache_entry.value);

                // send to client, if any
                return Ok(util::bake_response(DBServerResponse::Get {
                    key,
                    value: cache_entry.value.clone(),
                }));
            }
        }
    }

    // If not in cache, check the real memory store for it then
    let hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        let owner_did = sam_did.unwrap_or(did.clone());
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get(&owner_did) {
            // check for permissions
            if ht_data.access {
                let data_map = ht_data.get_data();
                if let Some(value) = data_map.get_data(&key) {
                    if let Some(access_flag) = data_map.get_access_state(&key) {
                        if access_flag {
                            // data found!
                            println!("~ {}: {}", key, value);
                            // return data to client
                            return Ok(util::bake_response(DBServerResponse::Get {
                                key,
                                value: value.to_owned(),
                            }));
                        } else {
                            // Access denied. Not personal!
                            util::log_error(&format!(
                                "Permission to access data unit denied: {}",
                                owner_did
                            ));
                            println!("~ {}: Access denied", key);
                            return Err(util::bake_error(DBError::AccessDenied));
                        }
                    } else {
                        return Err(util::bake_error(DBError::EntryNotFound));
                    }
                } else {
                    // not found
                    println!("~ {}: Not found.", key);
                    return Err(util::bake_error(DBError::EntryNotFound));
                }
            } else {
                // Access denied. Not personal!
                util::log_error(&format!(
                    "Permission to access data unit denied: {}",
                    owner_did
                ));
                println!("~ {}: Access denied", key);
                return Err(util::bake_error(DBError::AccessDenied));
            }
        } else {
            // not found
            println!("~ {}: Not found.", key);
            return Err(util::bake_error(DBError::EntryNotFound));
        }
    } else {
        util::log_error(&format!(
            "attempting to read data from uninitialized application: {did}"
        ));

        println!("~ Initialize application: `{did}` to read data");
        return Err(util::bake_error(DBError::InitializationError));
    }
}

/// Delete entry from database
pub async fn del_db_value(
    mut net_client: mpsc::Sender<ChannelMsg>,
    cfg: Arc<DBConfig>,
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
    key: String,
) -> DBServerResult {
    // First, check the database cache
    let mut app_cache = db_state
        .read()
        .await
        .cache
        .get(&did)
        .unwrap_or(&Default::default())
        .to_owned();

    if app_cache.entries.len() > 0 {
        // check for the key
        if let Some(cache_entry) = app_cache.entries.get(&key) {
            if cache_entry.sam_did == sam_did {
                // We've found what we're looking for, delete it
                app_cache.entries.remove(&key);
                db_state.write().await.cache.insert(did.clone(), app_cache);
                println!("~ {}: Entry removed", key);

                // send to db-client, if any
                return Ok(util::bake_response(DBServerResponse::Empty));
            }
        }
    }

    // If not in cache, check the real memory store for it then
    let mut hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        let owner_did = sam_did.unwrap_or(did.clone());
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get(&owner_did) {
            // check for permissions
            if ht_data.access {
                let data_map = ht_data.get_data();
                if let Some(_) = data_map.get_data(&key) {
                    if let Some(access_flag) = data_map.get_access_state(&key) {
                        if access_flag {
                            // data found, delete it
                            let mut data_map = data_map.clone();
                            data_map.delete_data_entry(&key);

                            // update hashtable entry
                            let mut ht_entry = ht_data.clone();
                            ht_entry.set_data(data_map);

                            // save hashtable data
                            hashtable.insert(owner_did.clone(), ht_entry);
                            db_state
                                .write()
                                .await
                                .applications
                                .insert(did.to_owned(), hashtable);

                            println!("~ {}: Entry removed", key);

                            // update the network state for IPFS synching
                            let _ = net_client
                                .clone()
                                .send(ChannelMsg::SetNetworkSyncFlag(did.clone()))
                                .await;

                            // gossip to peers
                            let _ = net_client
                                .send(network::ChannelMsg::GossipDeleteEvent {
                                    did,
                                    key,
                                    owner_did,
                                    write_time: util::current_unix_epoch(),
                                    cfg,
                                })
                                .await;

                            return Ok(util::bake_response(DBServerResponse::Empty));
                        } else {
                            // Access denied. Not personal!
                            util::log_error(&format!(
                                "Permission to delete data entry denied: {}",
                                owner_did
                            ));
                            println!("~ {}: Access denied", key);
                            return Err(util::bake_error(DBError::AccessDenied));
                        }
                    } else {
                        // not found
                        println!("~ {}: Not found.", key);
                        return Err(util::bake_error(DBError::EntryNotFound));
                    }
                } else {
                    // not found
                    println!("~ {}: Not found.", key);
                    return Err(util::bake_error(DBError::EntryNotFound));
                }
            } else {
                // Access denied. Not personal!
                util::log_error(&format!(
                    "Permission to delete data entry denied: {}",
                    owner_did
                ));
                println!("~ {}: Access denied", key);
                return Err(util::bake_error(DBError::AccessDenied));
            }
        } else {
            // not found
            println!("~ {}: Not found.", key);
            return Err(util::bake_error(DBError::EntryNotFound));
        }
    } else {
        util::log_error(&format!(
            "attempting to delete data from uninitialized application: {did}"
        ));

        println!("~ Initialize application: `{did}` to delete data");
        return Err(util::bake_error(DBError::InitializationError));
    }
}

/// Verify if there is an entry for a key in the database
async fn check_db_key_existence(
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
    key: String,
) -> DBServerResult {
    // First, check the database cache
    let app_cache = db_state
        .read()
        .await
        .cache
        .get(&did)
        .unwrap_or(&Default::default())
        .to_owned();

    if app_cache.entries.len() > 0 {
        // check for the key
        if let Some(cache_entry) = app_cache.entries.get(&key) {
            // check for owner
            if cache_entry.sam_did == sam_did {
                // we've found what we're looking for
                println!("~ {}: Record found", key);
                return Ok(util::bake_response(DBServerResponse::Exists(true)));
            }
        }
    }

    // If not in cache, check the real memory store for it then
    let hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        let owner_did = sam_did.unwrap_or(did.clone());
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get(&owner_did) {
            // check for permissions
            if ht_data.access {
                let data_map = ht_data.get_data();
                if let Some(_) = data_map.get_data(&key) {
                    if let Some(access_flag) = data_map.get_access_state(&key) {
                        if access_flag {
                            // data found, delete it
                            println!("~ {}: Record found", key);
                            return Ok(util::bake_response(DBServerResponse::Exists(true)));
                        } else {
                            // Access denied. Not personal!
                            util::log_error(&format!(
                                "Permission to access data entry denied: {}",
                                owner_did
                            ));
                            println!("~ {}: Access denied", key);
                            return Err(util::bake_error(DBError::AccessDenied));
                        }
                    } else {
                        // not found
                        println!("~ {}: No record found", key);
                        return Ok(util::bake_response(DBServerResponse::Exists(false)));
                    }
                } else {
                    // not found
                    println!("~ {}: No record found", key);
                    return Ok(util::bake_response(DBServerResponse::Exists(false)));
                }
            } else {
                // Access denied. Not personal!
                util::log_error(&format!(
                    "Permission to eccess data entry denied: {}",
                    owner_did
                ));
                println!("~ {}: Access denied", key);
                return Err(util::bake_error(DBError::AccessDenied));
            }
        } else {
            // not found
            println!("~ {}: No record found", key);
            return Ok(util::bake_response(DBServerResponse::Exists(false)));
        }
    } else {
        util::log_error(&format!(
            "attempting to access data of uninitialized application: {did}"
        ));
        println!("~ Initialize application: `{did}` to access data");
        return Err(util::bake_error(DBError::InitializationError));
    }
}

/// Return the keys for an application in a database
pub async fn get_db_keys(
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
    offset: usize,
    length: usize,
) -> DBServerResult {
    let hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        let owner_did = sam_did.unwrap_or(did.clone());
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get(&owner_did) {
            // check for permissions
            if ht_data.access {
                let data_map = ht_data.get_data();
                let keys_vec = data_map.get_keys(offset, length);
                let keys = keys_vec.join(", ");

                // return keys
                println!(
                    "~ Keys: {} ({} keys found)",
                    keys.trim_end_matches(", "),
                    keys_vec.len()
                );

                return Ok(util::bake_response(DBServerResponse::Keys(
                    keys.trim_end_matches(", ").to_owned(),
                )));
            } else {
                // Access denied. Not personal!
                util::log_error(&format!(
                    "Permission to eccess data entry denied: {}",
                    owner_did
                ));
                println!("~ : Access denied");
                return Err(util::bake_error(DBError::AccessDenied));
            }
        } else {
            // not found
            println!("~ Keys: (0 keys found)");
            return Err(util::bake_error(DBError::EntryNotFound));
        }
    } else {
        util::log_error(&format!(
            "attempting to access data of uninitialized application: {did}"
        ));
        println!("~ Initialize application: `{did}` to access data");
        return Err(util::bake_error(DBError::InitializationError));
    }
}

/// Delete every data belonging to a DID
pub async fn truncate_db_keys(
    mut net_client: mpsc::Sender<ChannelMsg>,
    cfg: Arc<DBConfig>,
    db_state: Arc<RwLock<DBState>>,
    did: String,
    sam_did: Option<String>,
) -> DBServerResult {
    let mut hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        let owner_did = sam_did.unwrap_or(did.clone());
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get(&owner_did) {
            // check for permissions
            if ht_data.access {
                let mut data_map = ht_data.get_data().to_owned();
                let entries_count = data_map.get_entries().len();

                // delete all the data
                data_map.clear();

                // update hashtable entry
                let mut ht_entry = ht_data.clone();
                ht_entry.set_data(data_map);

                // save hashtable data
                hashtable.insert(owner_did.clone(), ht_entry);
                db_state
                    .write()
                    .await
                    .applications
                    .insert(did.to_owned(), hashtable);

                println!("~ All entries deleted ({} entries)", entries_count);

                // update the network state for IPFS synching
                let _ = net_client
                    .clone()
                    .send(ChannelMsg::SetNetworkSyncFlag(did.clone()))
                    .await;

                // gossip to peers
                let _ = net_client
                    .send(network::ChannelMsg::GossipTruncateEvent {
                        did,
                        owner_did,
                        write_time: util::current_unix_epoch(),
                        cfg,
                    })
                    .await;

                return Ok(util::bake_response(DBServerResponse::Empty));
            } else {
                // Access denied. Not personal!
                util::log_error(&format!(
                    "Permission to delete data entry denied: {}",
                    owner_did
                ));
                println!("~ Access denied");
                return Err(util::bake_error(DBError::AccessDenied));
            }
        } else {
            // not found
            println!("~ Entry not found.");
            return Err(util::bake_error(DBError::EntryNotFound));
        }
    } else {
        util::log_error(&format!(
            "attempting to delete data from uninitialized application: {did}"
        ));
        println!("~ Initialize application: `{did}` to delete data");
        return Err(util::bake_error(DBError::InitializationError));
    }
}

/// Stop the database from supporting an application, locally and network-wide
pub async fn forget_application(
    cfg: Arc<DBConfig>,
    did: String,
    mnemonic: String,
    state: Arc<RwLock<DBState>>,
    net_client: mpsc::Sender<ChannelMsg>,
) {
    println!("~ removing application from database: {did}...");
    util::log_info(&format!("unsubscribing application from node: {did}..."));

    // try to authenticate details provided
    let auth_material = util::generate_auth_material(&mnemonic, SECRET_KEY);
    // if authenticated, we'll get the CID
    let hashtable_cid = contract::get_app_ht_cid(&cfg, &did, &auth_material).await;
    if !hashtable_cid.is_empty() {
        // first remove the application entry from the database
        state.write().await.remove_application(&did);

        // Now we move to the real deal: Networking
        // We want to unsubscribe the node from the (did) topic and stop recieving and sending updates of data concerning the application
        // We will create a one way channel and wait for the reply from the channel, then we exit
        let (sender, receiver) = oneshot::channel();
        let _ = net_client
            .clone()
            .send(network::ChannelMsg::UnsubscribeNodeFromApplication {
                did: did.clone(),
                cfg: cfg.clone(),
                sender,
            })
            .await;

        // now we block on it
        if let Ok(chanel_data) = receiver.await {
            if let Ok(msg) = chanel_data {
                println!("~ {msg}");
                return;
            }
        }
        return;
    }

    // Oops! We got here
    util::log_error("authentication details provided failed to match.");
    println!("~ Error: could not remove application from database.");
}

/// Manage an applications access to user data
pub async fn manage_db_access(
    cfg: Arc<DBConfig>,
    mut net_client: mpsc::Sender<ChannelMsg>,
    db_state: Arc<RwLock<DBState>>,
    did: String,
    owner_did: String,
    perm: bool,
) {
    let mut hashtable = db_state
        .read()
        .await
        .get_application_data(&did)
        .unwrap_or(&HashMap::new())
        .to_owned();

    if hashtable.len() > 0 {
        // check hashtable for owner entry
        if let Some(ht_data) = hashtable.get_mut(&owner_did) {
            // check for permissions
            ht_data.access = perm;
            // update access modification timestamp
            ht_data.access_modification_time = util::current_unix_epoch();

            db_state
                .write()
                .await
                .add_application(did.clone(), hashtable);

            println!("~ Data access settings modified");

            // update the network state for IPFS synching
            let _ = net_client
                .clone()
                .send(ChannelMsg::SetNetworkSyncFlag(did.clone()))
                .await;

            // gossip to peers
            let _ = net_client
                .send(network::ChannelMsg::GossipAccessChange {
                    perm,
                    did,
                    owner_did,
                    access_modification_time: util::current_unix_epoch(),
                    cfg,
                })
                .await;
        } else {
            // not found
            println!("~ Entry Not found.");
        }
    } else {
        util::log_error(&format!(
            "attempting to manage access of uninitialized application: {did}"
        ));
        println!("~ Initialize application: `{did}` to manage data access");
    }
}

/// Change configuration setting of the database
pub async fn change_db_cfg(
    net_client: mpsc::Sender<ChannelMsg>,
    did: String,
    flag: String,
    param: String,
) {
    match flag.as_str() {
        "-url" => {
            // change the pinning server URL of an application
            let (sender, receiver) = oneshot::channel();
            let _ = net_client
                .clone()
                .send(network::ChannelMsg::ChangePinningServer {
                    did: did.clone(),
                    url: param,
                    sender,
                })
                .await;

            // now we block on it
            if let Ok(chanel_data) = receiver.await {
                if let Ok(msg) = chanel_data {
                    println!("~ {msg}");
                    return;
                }
            }
        }
        _ => {}
    }
}

/// Display the database commands
pub async fn display_db_cmds() {
    println!();
    println!("~ SamaritanDB v1.0 (Chevalier)");
    println!("~ Copyright (c) 2023. Algorealm, Inc.");
    let help_text = r#"
    Database Commands:
    
    init <application_DID> <mnemonic>
        Initialize application to be managed by the database.
    
    set <application_DID> [samaritan_DID] <key> <value>
        Store data about an application or Samaritan.
    
    get <application_DID> [samaritan_DID] <key>
        Retrieve data.
    
    exists <application_DID> [samaritan_DID] <key>
        Check if data exists.
    
    info
        Provide information about the database.
    
    quit
        Perform necessary bookkeeping and shut down the database.
    
    truncate <application_DID> [samaritan_DID]
        Delete all data for an application or Samaritan.
    
    leave <application_DID>
        Remove an application from the database.
    
    del <application_DID> [samaritan_DID] <key>
        Delete data.
    
    config -url <link>
        Change the pinning server for IPFS updates.
    "#;

    println!("{}", help_text);
}
