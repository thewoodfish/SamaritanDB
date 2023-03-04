use std::collections::HashMap;

type DID = String;
type IpfsCid = String;
type HashKey = String;
type AuthContent = u64;
type DatabaseMetadata = String;
type AccountId = String;

#[derive(Default, Debug)]

struct FileMeta {
    access_list: Vec<AccountId>,
    cid: IpfsCid,
    timestamp: u64,
    db_meta: DatabaseMetadata,
}

#[derive(Default, Debug)]

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
    access_list: Mapping<DID, (HashKey, u64)>,
}

impl SamOs {
    /// Constructor that initializes all the contract storage to default
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
