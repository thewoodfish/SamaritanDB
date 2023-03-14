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
                "--suri",
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:#?}", output);

        output.status.success()
    }

    /// check if an account exists or is authenticated
    pub fn account_is_auth(
        cfg: &Arc<Config>,
        did: &str,
        password: AuthContent,
        is_auth: bool,
    ) -> bool {
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
                &format!("{}", is_auth),
                "--suri",
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:#?}", output);

        let binding = String::from_utf8_lossy(&output.stdout);
        util::parse_contract_boolean(&binding)
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
        let output = Command::new("cargo")
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
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:#?}", output);
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
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:#?}", output);
        output.status.success()
    }

    pub fn get_random_files(cfg: &Arc<Config>, did: &str) -> (String, String) {
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "get_files_info",
                "--args",
                &util::str_to_hex(did),
                "--suri",
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        let binding = String::from_utf8_lossy(&output.stdout);
        let basic_info = util::parse_contract_return_data(&binding);

        // get extra data
        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_address(),
                "--message",
                "get_files_extra_info",
                "--args",
                &util::str_to_hex(did),
                "--suri",
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        let binding = String::from_utf8_lossy(&output.stdout);
        let extra_info = util::parse_contract_tuple_vector(&binding);

        (basic_info, extra_info)
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
                "blast they annual column pave umbrella again olympic exotic vibrant lemon visa",
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "--dry-run",
            ])
            .current_dir("./sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:#?}", output);

        let binding = String::from_utf8_lossy(&output.stdout);
        (
            util::parse_first_tuple_u64(&binding),
            util::parse_contract_return_data(&binding),
        )
    }
}

pub mod interface {
    use std::sync::Arc;

    use super::super::util;
    use super::*;
    use crate::sam_prelude::{Config, HashKey};

    pub fn create_new_account(cfg: &Arc<Config>, did: &str, passw: &str) -> bool {
        let password = util::gen_hash(passw);
        net::create_new_account(cfg, did, password, "")
    }

    pub fn account_is_auth(cfg: &Arc<Config>, did: &str, passw: &str) -> bool {
        let password = util::gen_hash(passw);
        net::account_is_auth(cfg, did, password, true)
    }

    pub fn account_exists(cfg: &Arc<Config>, did: &str) -> bool {
        net::account_is_auth(cfg, did, 0, false)
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

    /// get initial random files to populate the database quickly
    pub fn get_init_files(cfg: &Arc<Config>, did: &str) -> (String, String) {
        net::get_random_files(cfg, did)
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
}
