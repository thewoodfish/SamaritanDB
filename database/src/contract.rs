// Copyright (c) 2023 Algorealm, Inc.

use crate::{cli, prelude::DBConfig};
use std::sync::Arc;

/// retrieve boot nodes from contract
pub async fn get_boot_nodes(cfg: &DBConfig) -> String {
    cli::get_boot_nodes(cfg).await
}

/// remove boot nodes from contract
pub async fn remove_boot_node(cfg: Arc<DBConfig>, addr: &str) {
    cli::remove_boot_node(cfg, addr).await;
}

/// add node multiaddress to contract bootnodes
pub async fn add_mulitaddress(cfg: &DBConfig, addr: &str) {
    cli::add_boot_node(cfg, addr).await
}

/// fetch an applications hashtable CID from contract
pub async fn get_app_ht_cid(cfg: &DBConfig, did: &str, auth: &str) -> String {
    cli::get_application_ht_cid(cfg, did, auth).await
}

/// fetch all nodes running an application
pub async fn get_subscribers(cfg: Arc<DBConfig>, did: &str) -> String {
    cli::get_subscribers(cfg, did).await
}

/// add node to application subscribers
pub async fn subscribe_node(cfg: Arc<DBConfig>, did: &str, addr: &str) {
    cli::subscribe_node(cfg, did, addr).await
}

/// update the hashtable CID of an application
pub async fn update_ht_cid(cfg: Arc<DBConfig>, did: &str, cid: &str) {
    cli::update_ht_cid(cfg, did, cid).await
}

/// unsubscribe node from the application
pub async fn unsubscribe_node(cfg: Arc<DBConfig>, did: &str, addr: &str) {
    cli::unsubscribe_node(cfg, did, addr).await
}

/// restrict or unrestrict an application's access to user data
pub async fn modify_app_access(cfg: Arc<DBConfig>, did: &str, owner_did: &str, perm: bool) {
    if perm {
        cli::restrict_application(cfg.clone(), &did, &owner_did).await;
    } else {
        cli::unrestrict_application(cfg.clone(), &did, &owner_did).await;
    }
}

/// get the list of users that block an app from accessing their data space
pub async fn get_application_access_blockers(cfg: Arc<DBConfig>, did: &str) -> String {
    cli::get_application_access_blockers(cfg, did).await
}
