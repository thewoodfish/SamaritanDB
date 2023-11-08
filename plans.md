## SamaritanDB + Application Use Case Draft

### Key Components

1. Database
1. Contract
1. Application

### Database

We have expectations of the database and based on that, we can figure out the technical components that we need.

1. Storage ( disk and memory ), in a document

2. Accept HTTP request from applications (GET, POST)

3. Generate Database client.
   The database client can perform a couple

4. Configure the database:
   What do we need to configure:

- The address/DID controlling the database node.
- The contract that the database configures around.

What is needed from the contract?

- The storage to query.
- The event to wathc out for.
- The function to call by the database.

```rust
    // contract storage
    #[ink(storage)]
    pub struct RandomContract {
        /// The storage accessed by SamaritanDB
        authorize: Mapping<(AccountId, AccountId), bool>
        ...
    }

    // event
    #[ink(event)]
    pub struct AuthorizationChange {
        #[ink(topic)]
        producer: AccountId,

        #[ink(topic)]
        consumer: AccountId,

        #[ink(topic)]
        state: bool
    }

    // single function that must be present on every contract
    pub fn is_authorized(&self, producer: AccountId, consumer: AccountId) -> bool {
        let can_access = authorize.get(&(producer, consumer)).unwrap_or(false);
        if can_access {
            true
        } else {
            false
        }
    }

```

5. Accept config request from the client

### Applications

We have expectations of the applications. There are three webpages:

1. Signup
   Here the user submits his name, email, username, DID, profile picture to the application. The application then stores the data in a database. Say a MYSQL "users" table or Mongo's "users" collection.

1. Home Page
   Select the users (Paginate them) from the "users" table and display them on the home page. Important information to note is their DID. This is the information we will be sending to the contract.

1. Single Content Creator Page
   Here the con

#### Very Important

This post field is COMPULSORY in every SamaritanDB server request:

- **consumer**: The address of the person that wants to view the data

### Contract

#### Written by third-party (Content Creator App)

1. The contract storage

```rust
    // type definitions
    type DataAddress = Vec<u8>

    struct UserInfo {
        balance: Balance
        data_address: DataAddress,
        subscription_fee: Balance,
    }

    // storage
    #[ink(storage)]
    pub struct Contract {
        /// Storage for the user and their data content address
        users: Mapping<AccountId, UserInfo>,
        /// The storage accessed by SamaritanDB
        authorize: Mapping<(AccountId, AccountId), bool>
        ...
    }
```

2. Here are some the application's contract function

```rust
    /// single function that must be present on every contract
    pub fn is_authorized(&self, producer: AccountId, consumer: AccountId) -> bool {
        let can_access = authorize.get(&(producer, consumer)).unwrap_or(false);
        if can_access {
            true
        } else {
            false
        }
    }

    /// returns the server address of the creator
    pub fn get_creator_server_address(&self, creator_addr: AccountId, server: AccountId) -> DataAddress {
        // viewer is the caller of the contract
        let viewer = self.env().caller();

        if let Some(creator) = self.users.get(&creator_addr) {
            // get the subscription fee the creator has set
            let fee = self.subscription.get(creator).unwrap_or(&0);

            // check if the viewer has subscribed
            if self.authorized.contains(&(creator, viewer)) {
                // the viewer has subscribed

                // return the user's creator content
                // return the address of the server

            } else {
                // return error stating that the user has not subscribed to the creators content
            }
        }
    }

    /// subscribe to the creators content
    pub fn subscribe(&self, creator: AccountId) {
        // viewer is the caller of the contract
        let viewer = self.env().caller();

        // get the subscription fee the creator has set
        let fee = self.subscription.get(creator).unwrap_or(&0);

        // first check if the viewer has enough money
        if Self::get_balance(viewer) > fee {
            // try to pay the subscription
            Self::transfer(viewer, creator);

            // SUBSCRIBE THE VIEWER. GIVING THE USER ACCESS
            self.authorize.insert(&(creator, viewer));

            // EMIT THE EVENT THE DATABASE MIGHT BE LISTENING FOR
            self.env()
                .emit_event(AuthorizationChange { creator, viewer, true });

        } else {
            // return error stating that we could not subscribe the viewer
        }
    }

    /// cancel subscription
    pub fn cancel_subscription(&self, creator_addr: AccountId) {
        // cancel subscription
        // modifying the contract authoraization storage the database looks out for
        // emiting the event the database listens for
    }

    pub fn update_data_addresss(&self, user: AccountId) {
        // updates the server address to find the users data/content
    }

    pub fn get_balance(&self, user: AccountId) -> Balance {
        // returns the balance of a user
    }

    fn transfer(&self, from: AccountId, to: AccountId, amount: Balance) {
        // transfer from one account to the other
    }

    /// other functions
    ...

```
