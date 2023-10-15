// Copyright (c) 2023 Algorealm, Inc.

use crate::prelude::*;
use crate::{cli, util};
use async_std::process;
use std::io::Read;

/// checks if the machine has the necessary installations e.g IPFS client
pub fn perform_machine_check() {
    async_std::task::block_on(util::initialize_logger());
    // println!("Checking machine for required dependencies...");
    if !cli::is_ipfs_installed() {
        println!("Please install IPFS client on your machine to run your database. For more information, visit https://docs.ipfs.tech/install/command-line/#system-requirements");
        util::log_error("IPFS not installed on machine");
        process::exit(2);
    }
}

/// reads in all the configuration parameters
pub fn read_config_file() -> DBConfig {
    let mut file = util::open_or_create(CONFIG_FILE);

    // Read the file contents into a string
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect_variant("Failed to read configuration file");

    // Parse the Config JSON contents
    let json = serde_json::from_str::<DBConfig>(&contents).ok();
    if let Some(cfg) = json {
        cfg
    } else {
        DBConfig::new()
    }
}

pub fn setup_node() {
    // we can't wait for the daemon, it doesn't return
    if !util::is_ipfs_daemon_running() {
        async_std::task::spawn(async {
            cli::start_ipfs_daemon().await;
        });
    }
}
