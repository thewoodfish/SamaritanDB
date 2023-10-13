// Copyright (c) 2023 Algorealm, Inc.

use async_std::sync::RwLock;
use dialoguer::{theme::ColorfulTheme, Input};
use prelude::*;
use std::{sync::Arc, thread};

mod cli;
mod contract;
mod network;
mod prelude;
mod server;
mod setup;
mod util;

#[async_std::main]
async fn main() {
    // introduce the database
    util::acknowledge();
    // check if machine is ready for the database operations
    setup::perform_machine_check();
    // more setup
    setup::setup_node();
    // read configuration file
    let db_config = Arc::new(setup::read_config_file());
    // define database state
    let state = Arc::new(RwLock::new(DBState::new()));
    // set up node identity
    let (mut net_client, cmd_sender) = network::setup_peer(db_config.clone()).await;
    // add database state to network manager
    net_client.db_state = state.clone();

    // set up database server on a stand-alone thread
    let cfg = db_config.clone();
    let db_state = state.clone();
    let sender = cmd_sender.clone();
    thread::spawn(move || server::setup_server(cfg, db_state, sender));

    // listen for network events
    let sender_channel = cmd_sender.clone();
    async_std::task::spawn(async move {
        net_client.run(sender_channel).await;
    });

    // main entry
    loop {
        let db_state = state.clone();
        let cmd_sender = cmd_sender.clone();
        if let Ok(cmd) = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("SamaritanDB")
            .interact_text()
        {
            let command = cli::pckg_raw_cmd(&cmd);
            if !matches!(command, Command::Invalid) {
                async_std::task::block_on(async {
                    let _ = command
                        .parse()
                        .execute(db_config.clone(), cmd_sender, db_state.clone())
                        .await;
                });
            } else {
                println!("~ Error: Invalid command syntax");
                continue;
            }
        }
    }
}
