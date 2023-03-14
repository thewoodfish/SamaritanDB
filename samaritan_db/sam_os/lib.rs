#![cfg_attr(not(feature = "std"), no_std)]

#[ink::contract]
mod sam_os {
    use ink::storage::Mapping;
    use scale_info::prelude::vec::Vec;

    type DID = Vec<u8>;
    type IpfsCid = Vec<u8>;
    type HashKey = u64;
    type AuthContent = u64;
    type DbMetadata = Vec<u8>;

    #[derive(scale::Decode, scale::Encode, Default)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct FileMeta {
        access_list: [DID; 2],
        cid: IpfsCid,
        nonce: u64,
        db_meta: DbMetadata,
    }

    #[derive(scale::Decode, scale::Encode, Default)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct UserInfo {
        auth_content: AuthContent,
        did_doc_cid: IpfsCid,
    }

    /// Error types
    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {}

    /// main storage structure for the SamaritanOS contract
    #[ink(storage)]
    pub struct SamOs {
        /// Storage for DIDs and their document and auth material
        auth_list: Mapping<DID, UserInfo>,
        /// Storage for data files metadata
        files_meta: Mapping<HashKey, FileMeta>,
        /// Storage for a DID and the files its allowed to access and their permissions
        access_list: Mapping<(DID, HashKey), u64>,
        /// List of file keys a DID has access to
        file_keys: Mapping<DID, Vec<HashKey>>,
    }

    /// Shorten the result type
    pub type Result<T> = core::result::Result<T, Error>;

    impl SamOs {
        /// Constructor that initializes the contract storage
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                auth_list: Default::default(),
                files_meta: Default::default(),
                access_list: Default::default(),
                file_keys: Default::default(),
            }
        }

        /// Creates a new account for a DID
        #[ink(message)]
        pub fn create_new_account(
            &mut self,
            did: DID,
            auth_content: AuthContent,
            did_doc_cid: IpfsCid,
        ) -> Result<()> {
            let user = UserInfo {
                auth_content,
                did_doc_cid,
            };

            self.auth_list.insert(did, &user);
            Ok(())
        }

        /// Checks if a DID with the provided auth material exists
        #[ink(message)]
        pub fn account_is_auth(&self, did: DID, auth_content: AuthContent, is_auth: bool) -> bool {
            // auth account
            if is_auth {
                let did_entry = self.auth_list.get(did);
                match did_entry {
                    Some(user_info) => user_info.auth_content == auth_content,
                    None => false,
                }
            } else {
                // check if the account exists at all
                // without verifying auth
                if let Some(_) = self.auth_list.get(did) {
                    true
                } else {
                    false
                }
            }
        }

        /// Gets important info for IPFS dile syncing
        #[ink(message)]
        pub fn get_file_sync_info(&self, hk: HashKey) -> (u64, IpfsCid) {
            match self.files_meta.get(hk) {
                Some(meta) => (meta.nonce, meta.cid),
                None => (1, Default::default()),
            }
        }

        /// Updates the metadata of a files
        #[ink(message)]
        pub fn update_file_meta(
            &mut self,
            cid: IpfsCid,
            hk: HashKey,
            metadata: DbMetadata,
            did_1: DID,
            did_2: DID,
            access_bit_1: bool,
            access_bit_2: bool,
        ) {
            let access_bits = [access_bit_1, access_bit_2];
            let dids = [did_1, did_2];
            let nonce = match self.files_meta.get(hk) {
                Some(meta) => meta.nonce + 1,
                None => 1,
            };

            let metadata = FileMeta {
                access_list: dids.clone(),
                cid,
                nonce,
                db_meta: metadata,
            };

            // save metadata
            self.files_meta.insert(hk, &metadata);

            // set up access list
            let mut index = 0;
            for did in dids {
                // sometimes there can be only one DID exclusive to a file
                if did != "did:sam:root:apps:xxxxxxxxxxxx".as_bytes().to_vec() {
                    let current_time = self.access_list.get((did.clone(), hk));
                    match current_time {
                        Some(time) => {
                            self.access_list.insert(
                                (did.clone(), hk),
                                if access_bits[index] { &time } else { &0 },
                            );
                            // 0 -> access denied
                        }
                        None => {
                            self.access_list.insert((did.clone(), hk), &1); // 1 -> no time limit
                        }
                    }

                    index += 1;
                    // insert the filekey
                    let keys = match self.file_keys.get(did.clone()) {
                        Some(keys) => {
                            if !keys.contains(&hk) {
                                let mut new_keys = keys.clone();
                                new_keys.push(hk);
                                new_keys
                            } else {
                                keys.clone()
                            }
                        }
                        None => {
                            let mut keys = Vec::<HashKey>::new();
                            keys.push(hk);
                            keys
                        }
                    };

                    self.file_keys.insert(did, &keys);
                }
            }
        }

        /// get info about files the DID has access to
        #[ink(message)]
        pub fn get_files_info(&self, did: DID) -> Vec<u8> {
            let mut return_data: Vec<u8> = Vec::new();
            match self.file_keys.get(did) {
                Some(keys) => {
                    let _ = keys
                        .iter()
                        .map(|hk| {
                            // get the corresponding file
                            let mut collator = Vec::<u8>::new();
                            let file = self.files_meta.get(hk).unwrap_or_default();
                            let mut did_1 = file.access_list[0].clone();
                            let mut did_2 = file.access_list[1].clone();
                            let cid = file.cid.clone();

                            collator.append(&mut did_1);
                            collator.append(&mut "--".as_bytes().to_vec()); // did separator
                            collator.append(&mut did_2);
                            collator.append(&mut "##".as_bytes().to_vec()); // separator

                            // then the cid
                            collator.append(&mut cid.to_vec());
                            collator.append(&mut "####".as_bytes().to_vec()); // chunk separator

                            // then append it to the return data
                            return_data.append(&mut collator);
                        })
                        .collect::<()>();
                }
                None => {}
            }
            return_data
        }

        /// get extra info about files the DID has access to
        #[ink(message)]
        pub fn get_files_extra_info(&self, did: DID) -> Vec<(u64, u64, u64)> {
            let mut collator: Vec<(u64, u64, u64)> = Vec::new();
            match self.file_keys.get(did) {
                Some(keys) => {
                    let _ = keys
                        .iter()
                        .map(|hk| {
                            let tuple: (u64, u64, u64);
                            let file = self.files_meta.get(hk).unwrap_or_default();
                            let did_1 = file.access_list[0].clone();

                            // get the access bits and nonce
                            let access_bit1 = self
                                .access_list
                                .get((did_1.clone(), hk))
                                .unwrap_or_default();
                            let nonce = file.nonce;

                            tuple = (nonce, access_bit1, *hk);
                            collator.push(tuple);
                        })
                        .collect::<()>();
                }
                None => {}
            }

            collator
        }

        /// Revokes a DIDs access to a file
        #[ink(message)]
        pub fn revoke_access(&mut self, did: DID, hk: HashKey, revoke: bool) {
            self.access_list
                .insert((did, hk), if revoke { &0 } else { &1 });
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[ink::test]
        fn new_works() {
            let mut sam = SamOs::new();
            let did = "did:sam:root:cdsidhfs809s9us0fs9".as_bytes().to_vec();
            sam.create_new_account(did, 4893290392, Vec::new()).ok();
            ink::env::debug_println!("{:#?}", sam);
        }
    }
}
