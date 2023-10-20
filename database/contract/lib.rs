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
    /// IPFS content identifier type
    type CID = Vec<u8>;

    #[derive(scale::Decode, scale::Encode, Default, Clone)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct AccountInfo {
        did_document_uri: Vec<u8>, // DID document IPFS CID
        hashtable_cid: Vec<u8>,    // Application/User Hashtable CID
        auth_material: Vec<u8>, // This helps authenticate applications during node initialization
    }

    #[ink(storage)]
    pub struct DbContract {
        /// Stores the possible bootnodes of the network
        nodes: Vec<Multiaddr>,
        /// Stores data about an application/user
        accounts: Mapping<DID, AccountInfo>,
        /// Stores nodes that run an applications operations (Gossipsub)
        subscribers: Mapping<DID, Vec<Multiaddr>>,
        /// Data access mapping application to users
        restricted: Mapping<DID, Vec<DID>>,
    }

    /// Contract events
    #[ink(event)]
    pub struct AccountCreated {
        #[ink(topic)]
        did: DID,
    }

    #[ink(event)]
    pub struct BootNodeAdded {
        #[ink(topic)]
        address: Multiaddr,
    }

    #[ink(event)]
    pub struct BootNodeRemoved {
        #[ink(topic)]
        address: Multiaddr,
    }

    #[ink(event)]
    pub struct HashTableAddressUpdated {
        #[ink(topic)]
        did: DID,
        ipfs_address: CID,
    }

    #[ink(event)]
    pub struct EntryNotFound {
        #[ink(topic)]
        entry_value: Vec<u8>,
    }

    #[ink(event)]
    pub struct TopicSubscriptionComplete {
        #[ink(topic)]
        did: DID,
        node: Multiaddr,
    }

    #[ink(event)]
    pub struct TopicUnsubscriptionComplete {
        #[ink(topic)]
        did: DID,
        node: Multiaddr,
    }

    #[ink(event)]
    pub struct RestrictApplicationAccess {
        #[ink(topic)]
        user_did: DID,
        #[ink(topic)]
        application_did: DID,
    }

    #[ink(event)]
    pub struct UnrestrictApplicationAccess {
        #[ink(topic)]
        user_did: DID,
        #[ink(topic)]
        application_did: DID,
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

        /// Checks if a DID exists
        #[ink(message, payable)]
        pub fn check_did_existence(&self, did: DID) -> bool {
            self.accounts.contains(&did)
        }

        /// Creates an account on the network
        #[ink(message, payable)]
        pub fn new_account(&mut self, did: DID, hashtable_cid: CID, auth_material: Vec<u8>) {
            // Get the account Id of the
            // The document would be created on demand
            let account = AccountInfo {
                did_document_uri: Default::default(),
                hashtable_cid,
                auth_material,
            };

            self.accounts.insert(&did, &account);

            // emit event
            self.env().emit_event(AccountCreated { did });
        }

        /// Adds your network address to the list of nodes using FIFO.
        /// This helps to eventually remove nodes that may exit without the proper bookkeeping
        #[ink(message, payable)]
        pub fn add_address(&mut self, addr: Multiaddr) {
            // Check if the address already exists in the nodes vector
            if !self.nodes.contains(&addr) {
                // If the vector has reached its maximum height, remove the oldest item before adding a new one
                if self.nodes.len() >= 10 {
                    self.nodes.remove(0);
                }
                // Add the address to the end of the vector
                self.nodes.push(addr.clone());

                // emit event
                self.env().emit_event(BootNodeAdded { address: addr });
            } else {
                self.env().emit_event(EntryNotFound { entry_value: addr });
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

                // emit event
                self.env().emit_event(BootNodeRemoved { address: addr });
            } else {
                self.env().emit_event(EntryNotFound { entry_value: addr });
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
                let mut new_account = account.clone();
                new_account.hashtable_cid = ht_cid.clone();
                self.accounts.insert(&did, &new_account);

                // emit event
                self.env().emit_event(HashTableAddressUpdated {
                    did,
                    ipfs_address: ht_cid,
                });
            } else {
                // emit event indicating the absence of the account
                self.env().emit_event(EntryNotFound { entry_value: did });
            }
        }

        /// Subscribe to join nodes supporting application
        #[ink(message, payable)]
        pub fn subscribe_node(&mut self, did: DID, addr: Multiaddr) {
            if let Some(subs) = self.subscribers.get(&did) {
                if !subs.contains(&addr) {
                    // append to the vector of multiaddresses
                    let mut subscribers = subs.clone();
                    subscribers.push(addr.clone());
                    self.subscribers.insert(&did, &subscribers);
                }
            } else {
                // create new, this node is the first of many
                let mut subscribers: Vec<Multiaddr> = Vec::new();
                subscribers.push(addr.clone());
                self.subscribers.insert(&did, &subscribers);
            }

            // emit event
            self.env()
                .emit_event(TopicSubscriptionComplete { did, node: addr });
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

                // emit event
                self.env()
                    .emit_event(TopicSubscriptionComplete { did, node: address });
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

        /// Add an application to the restricted list
        #[ink(message, payable)]
        pub fn restrict(&mut self, user_did: DID, app_did: DID) {
            // check for existence of user and application
            if self.accounts.contains(&user_did) {
                if self.accounts.contains(&app_did) {
                    let users_list = if let Some(users) = self.restricted.get(&app_did) {
                        let mut users = users.clone();
                        users.push(user_did.clone());
                        users
                    } else {
                        let mut users = Vec::new();
                        users.push(user_did.clone());
                        users
                    };

                    self.restricted.insert(app_did.clone(), &users_list);

                    // emit event
                    self.env().emit_event(RestrictApplicationAccess {
                        user_did,
                        application_did: app_did,
                    });
                } else {
                    self.env().emit_event(EntryNotFound {
                        entry_value: app_did,
                    });
                }
            } else {
                self.env().emit_event(EntryNotFound {
                    entry_value: user_did,
                });
            }
        }

        /// Unrestrict an application's access to user data
        #[ink(message, payable)]
        pub fn unrestrict(&mut self, user_did: DID, app_did: DID) {
            if let Some(users) = self.restricted.get(&app_did) {
                let users_list = users
                    .iter()
                    .cloned()
                    .filter(|did| *did != user_did)
                    .collect::<Vec<_>>();

                self.restricted.insert(&app_did, &users_list);

                // emit event
                self.env().emit_event(UnrestrictApplicationAccess {
                    user_did,
                    application_did: app_did,
                });
            } else {
                self.env().emit_event(EntryNotFound {
                    entry_value: app_did,
                });
            }
        }

        /// Check if an application is restricted
        fn is_restricted(&self, did: DID, app_did: DID) -> bool {
            if let Some(entry) = self.restricted.get(&did) {
                // check if the application is part of our restriction list
                entry.contains(&app_did)
            } else {
                false
            }
        }

        /// Fetch users that have restricted applications
        #[ink(message, payable)]
        pub fn get_restriction_list(&self, app_did: DID) -> Vec<u8> {
            if let Some(users) = self.restricted.get(&app_did) {
                let separator = b"$$$".to_vec();
                return users
                    .iter()
                    .flat_map(|vector| vector.iter().chain(separator.iter()))
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

            // Add the "$$$" separator
            let mut result = addr.clone();
            result.push(b'$');
            result.push(b'$');
            result.push(b'$');

            // test for equality
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
            let auth_material = "bfdh87y*(TD*&^*S&io".as_bytes().to_vec();
            db.new_account(
                did.clone(),
                cid.clone(),
                /* authentication material */ auth_material.clone(),
            );

            assert_eq!(db.get_account_ht_cid(did, auth_material), cid);
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

        #[ink::test]
        fn access_control_flow_works() {
            let mut db = DbContract::new();

            // create user
            let did = "did:sam:user:subfgns89fgg09sgs0j9fusj0fjd"
                .as_bytes()
                .to_vec();

            let cid = "QmfSnGmfexFsLDkbgN76Qhx2W8sxrNDobFEQZ6ER5qg2wW"
                .as_bytes()
                .to_vec();

            let auth_material = "bfdh87y*(TD*&^*S&io".as_bytes().to_vec();

            db.new_account(
                did.clone(),
                cid.clone(),
                /* authentication material */ auth_material.clone(),
            );

            // create application
            let app_did = "did:sam:apps:subfgns89fgg09sgs0j9fusj0fjd"
                .as_bytes()
                .to_vec();

            let app_cid = "Qmjhggfztfiov7zfbvyzhiuW8sxrNDobFEQZ6ER5qg2wW"
                .as_bytes()
                .to_vec();

            let app_auth_material = "bfdh87y*(TD*&^*S&io".as_bytes().to_vec();

            db.new_account(
                app_did.clone(),
                app_cid.clone(),
                /* authentication material */ app_auth_material.clone(),
            );

            // restrict app access
            db.restrict(did.clone(), app_did.clone());

            // check for restrictions
            assert!(db.is_restricted(did.clone(), app_did.clone()));

            // unrestrict and check again
            db.unrestrict(did.clone(), app_did.clone());

            // check for restrictions
            assert!(!db.is_restricted(did.clone(), app_did.clone()));
        }
    }
}
