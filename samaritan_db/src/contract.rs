mod net {
    use std::{collections::HashMap, process::Command};

    // type DID = String;
    type HashKey = u64;
    type AuthContent = u64;

    /// send nessage to contract to create account
    pub fn create_new_account(did: &str, password: AuthContent, did_doc_cid: &str) -> bool {
        let output = Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"did": "{did}", "auth_content": "{password}", "did_doc_cid": "{did_doc_cid}"}}"##),
                "http://localhost:4000/create-new-account"
            ])
            .output()
            .expect("failed to create account");

        output.status.success()
    }

    /// send nessage to contract to create account
    pub fn account_is_auth(did: &str, password: AuthContent, is_auth: bool) -> bool {
        let output = Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"did": "{did}", "auth_content": "{password}", "is_auth": {is_auth}}}"##),
                "http://localhost:4000/account-is-auth",
            ])
            .output()
            .expect("failed to authenticate account");

        let result = String::from_utf8_lossy(&output.stdout);
        let bool_str = result
            .split(":")
            .map(|e| e.trim_end_matches('}'))
            .skip(1)
            .next()
            .unwrap_or("false");

        bool_str.parse::<bool>().unwrap()
    }

    /// get specific file info
    pub fn get_file_info(hk: HashKey) -> (u64, String) {
        let output = Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"hashkey": "{hk}"}}"##),
                "http://localhost:4000/get-file-sync-info",
            ])
            .output()
            .expect("failed to get file info");

        let result = String::from_utf8_lossy(&output.stdout);
        let response: HashMap<String, String> = serde_json::from_str(&result).unwrap_or_default();
        let res = response.get("nonce").unwrap_or(&String::from("0")).clone();
        let cid = response.get("cid").unwrap();

        (res.parse::<u64>().unwrap(), cid.into())
    }

    /// update file details
    pub fn update_file_meta(
        cid: &str,
        hashkey: HashKey,
        metadata: &str,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"cid": "{cid}", "hashkey": "{hashkey}", "metadata": "{metadata}", "dids": ["{}", "{}"], "access_bits": [{}, {}]}}"##, dids[0], dids[1], access_bits[0], access_bits[1]),
                "http://localhost:4000/update-file-meta",
            ])
            .output()
            .expect("failed to update metadata");
    }

    // get random initial files
    pub fn get_random_files(cid: &str) -> String {
        let output = Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"cid": "{cid}"}}"##),
                "http://localhost:4000/get-random-files",
            ])
            .output()
            .expect("failed to update metadata");

        let result = String::from_utf8_lossy(&output.stdout);
        let response: HashMap<String, String> = serde_json::from_str(&result).unwrap_or_default();
        response.get("res").unwrap_or(&String::new()).into()
    }

    pub fn revoke_app_access(file_key: HashKey, app_did: &str, revoke: bool) -> bool {
        let output = Command::new("curl")
            .args([
                "--header",
                "Content-Type: application/json",
                "--request",
                "POST",
                "--data",
                &format!(r##"{{"file_key": "{file_key}", "app_did": "{app_did}", "revoke": {revoke}}}"##),
                "http://localhost:4000/revoke-app-access",
            ])
            .output()
            .expect("failed to authenticate account");

        let result = String::from_utf8_lossy(&output.stdout);
        let bool_str = result
            .split(":")
            .map(|e| e.trim_end_matches('}'))
            .skip(1)
            .next()
            .unwrap_or("false");

        bool_str.parse::<bool>().unwrap()
    }
}

pub mod interface {
    use super::super::*;
    use super::net;

    pub fn create_new_account(did: &str, passw: &str) -> bool {
        let password = util::gen_hash(passw);
        net::create_new_account(did, password, "")
    }

    pub fn account_is_auth(did: &str, passw: &str) -> bool {
        let password = util::gen_hash(passw);
        net::account_is_auth(did, password, true)
    }

    pub fn account_exists(did: &str) -> bool {
        net::account_is_auth(did, 0, false)
    }

    pub fn get_file_info(hk: HashKey) -> (u64, String) {
        net::get_file_info(hk)
    }

    /// update the file cid for the smart contract, for others to read latest image
    pub fn update_file_meta(
        cid: &str,
        hashkey: HashKey,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        net::update_file_meta(cid, hashkey, "", dids, access_bits);
    }

    /// get initial random files to populate the database quickly
    pub fn get_init_files(did: &str) -> String {
        net::get_random_files(did)
    }

    /// revoke an apps access to user data
    pub fn revoke_app_access(file_key: HashKey, app_did: &str, revoke: bool) -> bool {
        net::revoke_app_access(file_key, app_did, revoke)
    }
}
