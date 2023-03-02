#![cfg_attr(not(feature = "std"), no_std)]

#[ink::contract]
mod sam_contract {
    use ink::{prelude::vec::Vec, storage::Mapping};
    use ink_prelude::string::String;

    type DID = String;
    type IpfsCid = String;
    type HashKey = String;
    type AuthContent = String;
    type DatabaseMetadata = String;

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct FileMeta {
        access_list: Vec<AccountId>,
        cid: IpfsCid,
        timestamp: u64,
        db_meta: DatabaseMetadata,
    }

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    struct UserInfo {
        auth_content: AuthContent,
        did_doc_cid: IpfsCid,
    }

    #[ink(storage)]
    #[derive(Default)]
    pub struct SamOs {
        /// Storage for DIDs and their documents and auth material
        auth_list: Mapping<DID, UserInfo>,
        /// Storage for user documents metadata
        files_meta: Mapping<HashKey, FileMeta>,
        /// Stores the access list of a file for easy retreival
        access_list: Mapping<DID, (HashKey, u64)>,
    }

    impl SamOs {
        /// Constructor that initializes all the contract storage to default
        #[ink(constructor)]
        pub fn new() -> Self {
            Self::default()
        }

        #[ink(message)]
        pub fn create_new_account(
            &mut self,
            did: DID,
            auth_content: AuthContent,
            did_doc_cid: IpfsCid,
        ) -> Result<(), String>{
            let user = UserInfo {
                auth_content,
                did_doc_cid,
            };
            self.auth_list.insert(did, &user);

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[ink::test]
        fn it_works() {
            let mut sam = SamOs::new();
            assert_eq!(sam.create_new_account(
                "did:sam:root:DSOSAI0M83YAULZZLR6DEJMD3IWDKDHCRMJAR4HTFBHCW".to_string(),
                "DSOSAI0M83YAULZZLR6DEJMD3IWDKDHCRMJA".to_string(),
                "uhskjkfuai90arf0j9afsda".to_string(),
            ), Ok(()));
        }
    }
}
