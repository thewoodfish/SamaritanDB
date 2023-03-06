use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

type DID = String;
type IpfsCid = String;
type HashKey = u64;
type AuthContent = u64;
type DatabaseMetadata = String;
type AccountId = String;

#[derive(Default, Debug)]
#[allow(dead_code)]
struct FileMeta {
    access_list: Vec<AccountId>,
    cid: IpfsCid,
    modified: u64,
    db_meta: DatabaseMetadata,
}

#[derive(Default, Debug)]
#[allow(dead_code)]
struct UserInfo {
    auth_content: AuthContent,
    did_doc_cid: IpfsCid,
}

type Mapping<K, V> = HashMap<K, V>;

#[derive(Default, Debug)]
pub struct SamOs {
    /// Storage for DIDs and their documents and auth material
    auth_list: Mapping<DID, UserInfo>,
    /// Storage for user documents metadata
    files_meta: Mapping<HashKey, FileMeta>,
    /// Stores the access list of a file for easy retreival
    access_list: Mapping<DID, Mapping<HashKey, i64>>,
}

impl SamOs {
    /// Constructor that initializes all the contract storage to default
    #[allow(dead_code)]
    pub fn new() -> Self {
        SamOs {
            auth_list: Mapping::new(),
            files_meta: Mapping::new(),
            access_list: Mapping::new(),
        }
    }

    pub fn create_new_account(
        &mut self,
        did: DID,
        auth_content: AuthContent,
        did_doc_cid: IpfsCid,
    ) {
        let user = UserInfo {
            auth_content,
            did_doc_cid,
        };
        self.auth_list.insert(did, user);
    }

    /// get the latest timestamp and the latest CID
    pub fn get_file_sync_info(&self, hk: HashKey) -> (u64, String) {
        // get entry, if any
        match self.files_meta.get(&hk) {
            Some(meta) => (meta.modified, meta.cid.clone()),
            None => (0, String::new()),
        }
    }

    /// update file metadata to reflect latest changes in file accross the network
    pub fn update_file_meta(
        &mut self,
        cid: &str,
        hk: HashKey,
        metadata: &str,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let old_dids: Vec<String> = if dids[0] == "".to_string() {
            // get the previous metadata to retreive the DID(s)
            let new_meta = FileMeta::new();
            let tmp = self.files_meta.get(&hk).unwrap_or(&new_meta);
            tmp.access_list.clone()
        } else {
            dids.to_vec()
        };

        // created access list
        let meta = FileMeta {
            access_list: old_dids,
            cid: cid.to_owned(),
            modified: now,
            db_meta: metadata.to_string(),
        };
        self.files_meta.insert(hk, meta);

        let mut index = 0;
        for did in dids {
            // get the DIDs files
            let mut files = self.access_list.get(did).unwrap_or(&HashMap::new()).clone();
            files.insert(
                hk,
                if !access_bits[index] {
                    -1
                } else {
                    // keep it as is or set to 0
                    match self.access_list.get(did) {
                        Some(entry) => match entry.get(&hk) {
                            Some(e) => e.clone(),
                            None => 0,
                        },
                        None => 0,
                    }
                },
            );
            // 0 means infinity and there's no cap on time for now
            index += 0;

            // save
            if files.len() > 0 {
                self.access_list.insert(did.to_string(), files);
            };
        }
    }

    /// Return random files that a did has access to.
    /// This helps to populate the database initially
    pub fn get_random_files(&self, did: &str) -> Option<Vec<(HashKey, String)>> {
        if let Some(files) = self.access_list.get(did) {
            let collator: Vec<(HashKey, String)> = files
                .iter()
                .map(|(k, v)| {
                    // get the file latest CID if allowed
                    if *v != -1 {
                        let cid = match self.files_meta.get(k) {
                            Some(meta) => meta.cid.clone(),
                            None => Default::default(),
                        };
                        (k.clone(), cid)
                    } else {
                        (k.clone(), Default::default())
                    }
                })
                .filter(|(_, s)| s.len() > 0)
                .take(50)
                .collect();

            Some(collator)
        } else {
            None
        }
    }
}

impl FileMeta {
    /// Constructor that gives a new metadata
    pub fn new() -> Self {
        Self {
            access_list: Default::default(),
            cid: Default::default(),
            modified: Default::default(),
            db_meta: Default::default(),
        }
    }
}

pub mod interface {
    use super::super::*;

    pub fn create_new_account(did: &str, passw: &str, config: &Arc<Config>) -> bool {
        let password = util::gen_hash(passw);
        let did_doc_cid = "".to_string(); // we'll deal with this much later

        let mut guard = config.contract_storage.lock().unwrap();
        guard.create_new_account(did.to_owned(), password, did_doc_cid);

        true
    }

    pub fn account_is_auth(did: &str, config: &Arc<Config>, passw: &str) -> bool {
        let guard = config.contract_storage.lock().unwrap();
        let password = util::gen_hash(passw);

        if let Some(user_info) = guard.auth_list.get(did) {
            if password == (*user_info).auth_content {
                return true;
            }
            return false;
        } else {
            return false;
        }
    }

    pub fn get_file_info(cfg: &Arc<Config>, hk: HashKey) -> (u64, String) {
        let guard = cfg.contract_storage.lock().unwrap();
        guard.get_file_sync_info(hk)
    }

    /// update the file cid for the smart contract, for others to read latest image
    pub fn update_file_meta(
        cfg: &Arc<Config>,
        cid: &str,
        hashkey: HashKey,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        let mut guard = cfg.contract_storage.lock().unwrap();
        guard.update_file_meta(cid, hashkey, &cfg.metadata, dids, access_bits);
    }

    /// get initial random files to populate the database quickly
    pub fn get_init_files(cfg: &Arc<Config>, did: &str) -> Vec<(HashKey, String)> {
        let guard = cfg.contract_storage.lock().unwrap();
        guard.get_random_files(did).unwrap_or_default()
    }
}

// mod contract {
//     use super::util;
//     use std::process::Command;

//     pub async fn create_new_account(did: &str, passw: &str) -> bool {
//         let pw = util::blake2_hash(passw);
//         let password = String::from_utf8(pw).unwrap_or_default();

//         let output = Command::new("cargo")
//             .args([
//                 "contract",
//                 "call",
//                 "--contract",
//                 "5ErTHUWGoxPps2CSZmTEhtpErM7SkKnf5mzeG5cb3UDCe4zQ",
//                 "--message",
//                 "create_new_account",
//                 "--suri",
//                 "//Alice",
//                 "--args",
//                 did,
//                 password.as_str(),
//                 "emptyDidDocument", /* DID Document is not handled yet */
//             ])
//             .current_dir("../sam_os")
//             .output()
//             .expect("failed to execute process");

//         println!("{:?}", output);
//         true
//     }
// }
