// Copyright (c) 2023 Algorealm, Inc.

#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
mod db_contract {
    use ink::storage::Mapping;
    use scale_info::prelude::vec::Vec;

    /// Node multiaddress type
    type Multiaddr = Vec<u8>;
    /// Decentralized Identifier type
    type DID = Vec<u8>;
    /// IPFS content identifier
    type CID = Vec<u8>;

    #[derive(scale::Decode, scale::Encode, Default, Clone)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct AccountInfo {
        did_document_uri: Vec<u8>,
        hashtable_cid: Vec<u8>,
        // This helps authenticate applications during node initialization
        auth_material: Vec<u8>,
    }

    #[ink(storage)]
    pub struct DbContract {
        /// Stores the possible bootnodes of the network
        nodes: Vec<Multiaddr>,
        /// Stores a mapping of a DID and its important corresponding data
        accounts: Mapping<DID, AccountInfo>,
        /// Stores nodes that run an applications operations
        subscribers: Mapping<DID, Vec<Multiaddr>>,
        /// Restricted access mapping (restricted application -> users)
        restricted: Mapping<DID, Vec<DID>>,
    }

    impl DbContract {
        /// Constructor that initializes the contract storage
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                nodes: Vec::with_capacity(10),
                accounts: Default::default(),
                subscribers: Default::default(),
                restricted: Default::default(),
            }
        }

        /// Creates an account on Samaritan OS
        #[ink(message, payable)]
        pub fn new_account(&mut self, did: DID, hashtable_cid: CID, auth_material: Vec<u8>) {
            let did_document_uri = Vec::new();
            let account = AccountInfo {
                did_document_uri,
                hashtable_cid,
                auth_material,
            };
            self.accounts.insert(&did, &account);
        }

        /// Checks if a DID exists
        #[ink(message, payable)]
        pub fn check_did_existence(&self, did: DID) -> bool {
            self.accounts.contains(&did)
        }

        /// Adds your network address to the list of nodes using FIFO.
        /// This helps us eventuallu remove nodes that may exit without the proper bookkeeping
        #[ink(message, payable)]
        pub fn add_address(&mut self, addr: Multiaddr) {
            // Check if the address already exists in the nodes vector
            if !self.nodes.contains(&addr) {
                // If the vector has reached its maximum height, remove the oldest item before adding a new one
                if self.nodes.len() >= 10 {
                    self.nodes.remove(0);
                }
                // Add the address to the end of the vector
                self.nodes.push(addr);
            }
        }

        /// Remove node address from bootnodes
        #[ink(message, payable)]
        pub fn remove_address(&mut self, addr: Multiaddr) {
            // Check if the address already exists in the nodes vector
            if self.nodes.contains(&addr) {
                // remove address
                let filtered_nodes = self
                    .nodes
                    .iter()
                    .cloned()
                    .filter(|address| *address != addr)
                    .collect::<Vec<_>>();

                self.nodes = filtered_nodes;
            }
        }

        /// Retrieves the list of bootnodes available
        #[ink(message, payable)]
        pub fn get_node_addresses(&self) -> Vec<u8> {
            self.nodes
                .iter()
                .flat_map(|addr| {
                    let separator: &[u8] = b"$$$";
                    addr.iter()
                        .chain(separator.iter())
                        .copied()
                        .collect::<Vec<u8>>()
                })
                .collect()
        }

        /// Retrieves the hashtable CID of an account
        #[ink(message, payable)]
        pub fn get_account_ht_cid(&self, did: DID, auth_material: Vec<u8>) -> Vec<u8> {
            if let Some(account_info) = self.accounts.get(&did) {
                if account_info.auth_material == auth_material {
                    account_info.hashtable_cid.clone()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }

        /// Updates the hashtable CID of an account
        #[ink(message, payable)]
        pub fn update_account_ht_cid(&mut self, did: DID, ht_cid: Vec<u8>) {
            if let Some(account) = self.accounts.get(&did) {
                let mut account = account.clone();
                account.hashtable_cid = ht_cid;
                self.accounts.insert(&did, &account);
            }
        }

        /// Subscribe to join nodes supporting application
        #[ink(message, payable)]
        pub fn subscribe_node(&mut self, did: DID, addr: Multiaddr) {
            if let Some(subs) = self.subscribers.get(&did) {
                if !subs.contains(&addr) {
                    // append to the vector of multiaddresses
                    let mut subscribers = subs.clone();
                    subscribers.push(addr);
                    self.subscribers.insert(&did, &subscribers);
                }
            } else {
                // create new, this node is the first of many
                let mut subscribers: Vec<Multiaddr> = Vec::new();
                subscribers.push(addr);
                self.subscribers.insert(&did, &subscribers);
            }
        }

        /// Stop supporting application
        #[ink(message, payable)]
        pub fn unsubscribe_node(&mut self, did: DID, address: Multiaddr) {
            if let Some(nodes) = self.subscribers.get(&did) {
                let filtered_nodes = nodes
                    .iter()
                    .cloned()
                    .filter(|addr| *addr != address)
                    .collect::<Vec<_>>();
                self.subscribers.insert(&did, &filtered_nodes);
            }
        }

        /// Get all nodes supporting an application
        #[ink(message, payable)]
        pub fn get_subscribers(&mut self, did: DID) -> Vec<u8> {
            if let Some(nodes) = self.subscribers.get(&did) {
                let separator = b"$$$".to_vec();
                nodes
                    .iter()
                    .flat_map(|vector| vector.iter().chain(separator.iter()))
                    .copied()
                    .collect()
            } else {
                Vec::new()
            }
        }

        /// Manage the access of an application to a users data space
        #[ink(message, payable)]
        pub fn manage_access(&mut self, app_did: DID, user_did: DID, allow: bool) {
            if allow {
                // unrestrict
                if let Some(users) = self.restricted.get(&app_did) {
                    let special_users = users
                        .iter()
                        .cloned()
                        .filter(|did| *did != user_did)
                        .collect::<Vec<_>>();

                    self.restricted.insert(&app_did, &special_users);
                }
            } else {
                // restrict
                let user_list = if let Some(users) = self.restricted.get(&app_did) {
                    let mut users = users.clone();
                    if !users.contains(&user_did) {
                        users.push(user_did);
                    }
                    users
                } else {
                    let mut users = Vec::new();
                    users.push(user_did);
                    users
                };

                self.restricted.insert(app_did, &user_list);
            }
        }

        /// Get all the actors that have denied an application access to their data space
        #[ink(message, payable)]
        pub fn get_blockers(&self, app_did: DID) -> Vec<u8> {
            if let Some(users) = self.restricted.get(&app_did) {
                let separator = b"$$$".to_vec();
                return users
                    .iter()
                    .flat_map(|user| user.iter().chain(separator.iter()))
                    .copied()
                    .collect();
            }

            Vec::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[ink::test]
        fn add_address_works() {
            let mut db = DbContract::new();
            let addr = "/ip4/192.168.44.205/tcp/1509".as_bytes().to_vec();
            db.add_address(addr.clone());

            // Remove the "$$$" separator from addr
            let mut result = addr.clone();
            result.push(b'#');
            result.push(b'#');

            // assert
            assert_eq!(db.get_node_addresses(), result);
        }

        #[ink::test]
        fn creation_works() {
            let mut db = DbContract::new();
            let did = "did:sam:apps:subfgns89fgg09sgs0j9fusj0fjd"
                .as_bytes()
                .to_vec();
            // let ht_cid = "Qmsdujfhsd8sg8s8483nhn10vddfi".as_bytes().to_vec();
            let cid = "QmfSnGmfexFsLDkbgN76Qhx2W8sxrNDobFEQZ6ER5qg2wW"
                .as_bytes()
                .to_vec();
            db.new_account(did.clone(), cid.clone());
            // db.update_account_ht_cid(did.clone(), ht_cid.clone());
            assert_eq!(db.get_account_ht_cid(did), cid);
        }

        #[ink::test]
        fn subscribing_flow_works() {
            let mut db = DbContract::new();
            let did = "did:sam:apps:subfgns89fgg09sgs0j9fusj0fjd"
                .as_bytes()
                .to_vec();
            let addr = "/ip4/192.168.44.205/tcp/1509".as_bytes().to_vec();

            // subscribe
            db.subscribe_node(did.clone(), addr.clone());

            // get subscribers
            assert_eq!(
                db.get_subscribers(did.clone()),
                "/ip4/192.168.44.205/tcp/1509$$$".as_bytes().to_vec(),
            );

            // delete subscribers
            db.unsubscribe_node(did.clone(), addr.clone());
            assert_eq!(db.get_subscribers(did.clone()), Vec::new());
        }
    }
}
