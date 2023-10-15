// Copyright (c) 2023 Algorealm, Inc.

use crate::{prelude::*, util};

use std::{
    io::Write,
    process::{self, Command as CliCommand, Stdio},
    sync::Arc,
};

/// This function packages raw cli commands into useful enum
pub fn pckg_raw_cmd(cmd: &str) -> Command {
    match cmd.split_ascii_whitespace().collect::<Vec<&str>>()[..] {
        ["quit"] => Command::Quit,
        ["help"] => Command::Help,
        ["init", did, mnemonic] => Command::Init { did, mnemonic },
        ["set", did, sam_did, key, value] => Command::Set {
            did,
            sam_did: Some(sam_did),
            key,
            value,
        },
        ["set", did, key, value] => Command::Set {
            did,
            sam_did: None,
            key,
            value,
        },
        ["get", did, key] => Command::Get {
            did,
            sam_did: None,
            key,
        },
        ["get", did, sam_did, key] => Command::Get {
            did,
            sam_did: Some(sam_did),
            key,
        },
        ["del", did, key] => Command::Del {
            did,
            sam_did: None,
            key,
        },
        ["del", did, sam_did, key] => Command::Del {
            did,
            sam_did: Some(sam_did),
            key,
        },
        ["exists", did, key] => Command::Exists {
            did,
            sam_did: None,
            key,
        },
        ["exists", did, sam_did, key] => Command::Exists {
            did,
            sam_did: Some(sam_did),
            key,
        },
        ["keys", did, sam_did, offset, length] => Command::Keys {
            did,
            sam_did: Some(sam_did),
            offset: offset.parse::<usize>().ok(),
            length: length.parse::<usize>().ok(),
        },
        ["keys", did, offset, length] => Command::Keys {
            did,
            sam_did: None,
            offset: offset.parse::<usize>().ok(),
            length: length.parse::<usize>().ok(),
        },
        ["truncate", did, sam_did] => Command::Truncate {
            did,
            sam_did: Some(sam_did),
        },
        ["truncate", did] => Command::Truncate { did, sam_did: None },
        ["leave", did, mnemonic] => Command::Leave { did, mnemonic },
        ["info"] => Command::Info,
        ["config", did, flag, param] => Command::Config { did, flag, param },
        _ => Command::Invalid,
    }
}

/// check if IPFS is installed
pub fn is_ipfs_installed() -> bool {
    let output = CliCommand::new("ipfs")
        .arg("--help")
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute IPFS command"));

    output.status.success()
}

/// start the IPFS daemon
pub async fn start_ipfs_daemon() {
    let output = CliCommand::new("ipfs")
        .arg("daemon")
        .output()
        .expect("Failed to start IPFS daemon");

    if output.status.success() {
        util::log_info("IPFS daemon started successfully");
    } else {
        util::log_error("Failed to start IPFS daemon");
        process::exit(1);
    }
}

/// Stop the IPFS deamon on the machine
#[allow(dead_code)]
pub fn stop_ipfs_daemon() {
    let output = CliCommand::new("ipfs")
        .arg("shutdown")
        .output()
        .expect("Failed to stop IPFS daemon");

    if output.status.success() {
        util::log_info("IPFS daemon stopped successfully");
    } else {
        util::log_error("Failed to stop IPFS daemon");
        process::exit(1);
    }
}

/// Retrieve the boot nodes from the smart contract
pub async fn get_boot_nodes(cfg: &DBConfig) -> String {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "get_node_addresses",
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "-dry-run",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(output) => {
                let binding: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
                return util::parse_contract_return_data(&binding);
            }
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `get_node_addresses()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Add node to the contract to serve as a possible bootnode
pub async fn add_boot_node(cfg: &DBConfig, addr: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "add_address",
                "--args",
                &util::str_to_hex(addr),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error("contract invocation returned an error: fn -> `create_account`");
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Add node to the contract to serve as a possible bootnode
pub async fn get_application_ht_cid(cfg: &DBConfig, did: &str, auth: &str) -> String {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "get_account_ht_cid",
                "--args",
                &util::str_to_hex(did),
                auth,
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "-dry-run",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(output) => {
                let binding: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
                return util::parse_contract_return_data(&binding);
            }
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `get_account_ht_cid()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Fetch data from IPFS
pub fn fetch_from_ipfs(cid: &str) -> Result<String, Error> {
    let output = CliCommand::new("ipfs").args(["cat", cid]).output()?;
    util::log_info(&format!("ipfs file download, cid: {}", cid));

    let result = output.stdout;
    // try to decrypt the bytes
    if let Ok(data) = util::decrypt_data(result) {
        Ok(String::from_utf8_lossy(&data).into())
    } else {
        Err(Box::new(DBError::DecryptionError))
    }
}

/// Write data to IPFS
pub fn write_to_ipfs(data: &Vec<u8>) -> Result<String, Error> {
    // Create a new IPFS process with the 'add' subcommand
    let mut ipfs_add = CliCommand::new("ipfs")
        .arg("add")
        .arg("-Q") // Use '-Q' flag to output only the CID (hash)
        .stdin(Stdio::piped()) // Open a pipe to write the data to the process
        .stdout(Stdio::piped()) // Open a pipe to read the process output
        .spawn()?;

    // Write the data to the IPFS process
    if let Some(stdin) = ipfs_add.stdin.as_mut() {
        stdin.write_all(data)?;
    }

    // Wait for the process to finish and collect the output
    let output = ipfs_add.wait_with_output()?;

    // Convert the output bytes to a String (the CID/hashed content identifier)
    let cid = String::from_utf8(output.stdout)?.trim().to_string();
    util::log_info(&format!(
        "{} bytes written to IPFS with CID: {}",
        data.len(),
        cid
    ));

    Ok(cid)
}

/// Select nodes running application
pub async fn get_subscribers(cfg: Arc<DBConfig>, did: &str) -> String {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "get_subscribers",
                "--args",
                &util::str_to_hex(did),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                "-dry-run",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(output) => {
                let binding: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
                return util::parse_contract_return_data(&binding);
            }
            Err(_) => {
                util::log_error("contract invocation returned an error: fn -> `get_subscribers()`. Trying again...");
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Subscribe node to application operations
pub async fn subscribe_node(cfg: Arc<DBConfig>, did: &str, addr: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "subscribe_node",
                "--args",
                &util::str_to_hex(did),
                &util::str_to_hex(addr),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error("contract invocation returned an error: fn -> `subscribe_node()`. Trying again...");
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Unsubscribe node from managing application operations
pub async fn unsubscribe_node(cfg: Arc<DBConfig>, did: &str, addr: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "unsubscribe_node",
                "--args",
                &util::str_to_hex(did),
                &util::str_to_hex(addr),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `unsubscribe_node()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Update the hashtable CID of an application
pub async fn update_ht_cid(cfg: Arc<DBConfig>, did: &str, cid: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "update_account_ht_cid",
                "--args",
                &util::str_to_hex(did),
                &util::str_to_hex(cid),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `new_account()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Remove node from bootnodes in the contract, if it's one of them
pub async fn remove_boot_node(cfg: Arc<DBConfig>, addr: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "remove_address",
                "--args",
                &util::str_to_hex(addr),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error("contract invocation returned an error: fn -> `remove_address()`. Trying again...");
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Restrict an application's access to user data
pub async fn restrict_application(cfg: Arc<DBConfig>, did: &str, owner_did: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "restrict",
                "--args",
                &util::str_to_hex(did),
                &util::str_to_hex(owner_did),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `restrict()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Restrict an application's access to user data
pub async fn unrestrict_application(cfg: Arc<DBConfig>, did: &str, owner_did: &str) {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "unrestrict",
                "--args",
                &util::str_to_hex(did),
                &util::str_to_hex(owner_did),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(_) => return,
            Err(_) => {
                util::log_error(
                    "contract invocation returned an error: fn -> `unrestrict()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

/// Pin IPFS file on local node, so it can serve the network
pub fn pin_ipfs_cid(cid: &str) {
    match CliCommand::new("ipfs")
        .arg("pin")
        .arg("add")
        .arg(cid)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                util::log_info(&format!("Successfully pinned CID ({}) on local node", cid));
            } else {
                util::log_error(&format!("Error pinning CID ({}) on local node", cid));
            }
        }
        Err(_) => util::log_error(&format!("Error pinning CID ({}) on local node", cid)),
    }
}

/// Get users that block an app from accessing their data space
pub async fn get_application_access_blockers(cfg: Arc<DBConfig>, did: &str) -> String {
    loop {
        match CliCommand::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                &cfg.get_contract_addr(),
                "--message",
                "get_blockers",
                "--args",
                &util::str_to_hex(did),
                "--suri",
                &cfg.get_chain_keys(),
                "--url",
                "wss://rococo-contracts-rpc.polkadot.io",
                // "-dry-run",
            ])
            .current_dir("./contract")
            .output()
        {
            Ok(output) => {
                let binding: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
                return util::parse_contract_return_data(&binding);
            }
            Err(_) => {
                util::log_error("contract invocation returned an error: fn -> `get_application_access_blockers()`. Trying again...");
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}
