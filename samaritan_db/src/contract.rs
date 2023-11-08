// Copyright (c) 2023 Algorealm
// This file is a part of SamaritanDb

/// the main module to call the contract
pub mod net {
    use super::super::util;
    use crate::sam_prelude::{Config, HashKey};
    use std::process::Command;
    use std::sync::Arc;
    type AuthContent = u64;

    /// send nessage to contract to create account
    pub fn create_new_account(
        cfg: &Arc<Config>,
        did: &str,
        password: AuthContent,
        _did_doc_cid: &str,
        ht_cid: &str,
    ) -> bool {
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "create_new_account",
                "--args",
                &util::str_to_hex(did),
                &format!("{}", password),
                &util::str_to_hex("empty"),
                &util::str_to_hex(ht_cid),
                "--suri",
                &cfg.get_contract_keys(),
                "--skip-confirm"
                // "--url",
                // "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        output.status.success()
    }

    /// check if an account exists or is authenticated
    pub fn account_is_auth(cfg: &Arc<Config>, did: &str, password: AuthContent) -> (bool, String) {
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "account_is_auth",
                "--args",
                &util::str_to_hex(did),
                &format!("{}", password),
                "--suri",
                &cfg.get_contract_keys(),
                // "--url",
                // "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        let binding = String::from_utf8_lossy(&output.stdout);
        (
            util::parse_contract_boolean(&binding),
            util::parse_contract_return_data(&binding),
        )
    }

    /// update file details
    pub fn update_file_meta(
        cfg: &Arc<Config>,
        cid: &str,
        hashkey: HashKey,
        _metadata: &str,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        let metadata = "Algorealms SamaritanDB";
        Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "update_file_meta",
                "--args",
                &util::str_to_hex(cid),
                &format!("{}", hashkey),
                &util::str_to_hex(metadata),
                &util::str_to_hex(&dids[0]),
                &util::str_to_hex(if !dids[1].is_empty() {
                    &dids[1]
                } else {
                    "did:sam:root:apps:xxxxxxxxxxxx"
                }),
                &format!("{}", access_bits[0]),
                &format!("{}", access_bits[1]),
                "--suri",
                &cfg.get_contract_keys(),
                "--skip-confirm"
                // "--url",
                // "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");
    }

    pub fn revoke_app_access(
        cfg: &Arc<Config>,
        file_key: HashKey,
        app_did: &str,
        revoke: bool,
    ) -> bool {
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "revoke_access",
                "--args",
                &util::str_to_hex(app_did),
                &format!("{}", file_key),
                &format!("{}", revoke),
                "--suri",
                &cfg.get_contract_keys(),
                "--skip-confirm"
                // "--url",
                // "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");
        output.status.success()
    }

    /// get specific file info
    pub fn get_file_info(cfg: &Arc<Config>, hk: HashKey) -> (u64, String) {
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "get_file_sync_info",
                "--args",
                &format!("{}", hk),
                "--suri",
                &cfg.get_contract_keys(),
                // "--url",
                // "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        let binding = String::from_utf8_lossy(&output.stdout);
        (
            util::parse_first_tuple_u64(&binding),
            util::parse_contract_return_data(&binding),
        )
    }

    /// update hashmap
    pub fn update_hashtable(cfg: &Arc<Config>, cid: &str, did: &str) {
        Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "update_hashtable",
                "--args",
                &util::str_to_hex(cid),
                &util::str_to_hex(did),
                "--suri",
                &cfg.get_contract_keys(),
                "--skip-confirm"
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");
    }
}

pub mod interface {
    use std::sync::Arc;

    use super::super::util;
    use super::*;
    use crate::sam_prelude::{Config, HashKey};

    pub fn create_new_account(cfg: &Arc<Config>, did: &str, passw: &str, ht_cid: &str) -> bool {
        let password = util::gen_hash(passw);
        net::create_new_account(cfg, did, password, "", ht_cid)
    }

    pub fn account_is_auth(cfg: &Arc<Config>, did: &str, passw: &str) -> (bool, String) {
        let password = util::gen_hash(passw);
        net::account_is_auth(cfg, did, password)
    }

    pub fn account_exists(cfg: &Arc<Config>, did: &str) -> (bool, String) {
        net::account_is_auth(cfg, did, 0)
    }

    pub fn get_file_info(cfg: &Arc<Config>, hk: HashKey) -> (u64, String) {
        net::get_file_info(cfg, hk)
    }

    /// update the file cid for the smart contract, for others to read latest image
    pub fn update_file_meta(
        cfg: &Arc<Config>,
        cid: &str,
        hashkey: HashKey,
        dids: &[String; 2],
        access_bits: &[bool; 2],
    ) {
        net::update_file_meta(cfg, cid, hashkey, "", dids, access_bits);
    }

    /// revoke an apps access to user data
    pub fn revoke_app_access(
        cfg: &Arc<Config>,
        file_key: HashKey,
        app_did: &str,
        revoke: bool,
    ) -> bool {
        net::revoke_app_access(cfg, file_key, app_did, revoke)
    }

    /// update the state of the hashmap
    pub fn update_hashtable(cfg: &Arc<Config>, cid: &str, did: &str) {
        net::update_hashtable(cfg, cid, did);
    }
}