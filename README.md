# SamaritanDB: Empowering Data Soverignty

SamaritanDB is a decentralized data store that empowers users to maintain control and sovereignty over the data stored about them by the applications and services they interact with across the internet. To learn more about what SamaritanDB is and why it holds significance, please visit [our wiki](https://algorealm.gitbook.io/samaritandb).
This Proof of Concept (POC) serves as a critical stepping stone on our path to realizing the overarching vision and future commitments of our project. Its successful execution and validation are paramount to paving the way for the attainment of our core objectives and the promise of a brighter future

## Table of Contents

- [Technologies](#technologies)
- [Getting Started](#getting-started)
- [Commands](#commands)
- [How It Works](#how-it-works)
- [License](#license)

## Technologies

SamaritanDB leverages several key technologies, including:

- **libp2p**: A modular network stack that provides peer-to-peer communication capabilities.
- **IPFS (InterPlanetary File System)**: A protocol and network designed to create a content-addressable, peer-to-peer method of storing and sharing hypermedia in a distributed file system.
- **ink!**: A Rust-based framework for writing and executing smart contracts.
- **Cargo Contract**: A Rust setup and deployment tool for developing Wasm based smart contracts via ink!
- Other valuable Rust libraries that enhance the functionality of SamaritanDB.

## Getting Started

### Installation

To get started with SamaritanDB, follow these steps:

1. Clone the repository: `git clone https://github.com/yourusername/samaritandb.git`.
2. Install the [IPFS daemon](https://docs.ipfs.tech/install/).
3. Install and setup the rust toolchain.
4. Install [cargo contract](https://crates.io/crates/cargo-contract).

### Configuration

1. **Modify the Configuration File:**

   - Navigate to the `.resource` folder.
   - Edit the `conf.json` file.

2. **Configure Blockchain Keys:**

   - In the `conf.json` file, update the `chain_keys` by setting the value to your funded Rococo Contract keys. This step is crucial as the database will often interact with the ink! contract to function properly.

## Commands

Below are the commands available for interacting with SamaritanDB on the command-line:

- **Initialize an Application to be Managed by the Database:**
  `init <application_DID> <your_application_generated_mnemonic>`

- **Store Data About an Application or Samaritan:**
  `set <application_DID> [samaritan_DID] <key> <value>`

- **Retrieve Data:**
  `get <application_DID> [samaritan_DID] <key>`

- **Check If Data Exists:**
  `exists <application_DID> [samaritan_DID] <key>`

- **Provide Information About the Database:**
  `info`

- **Perform Necessary Bookkeeping and Shut Down the Database:**
  `quit`

- **Delete All Data for an Application or Samaritan:**
  `truncate <application_DID> [samaritan_DID]`

- **Remove an Application from the Database:**
  `leave <application_DID>`

- **Delete Data:**
  `del <application_DID> [samaritan_DID] <key>`

- **Change the Pinning Server for IPFS Updates:**
  `config -url <link>`

## What can this POC do?

- Communicate peer-to-peer without relying on a single coordinator.
- Store and retrieve data in a decentralized manner.
- Broadcast operations to peers for real-time data synchronization.
- Persist data securely using IPFS (InterPlanetary File System).
- Facilitate seamless integration of new nodes into the management of application data.
- Responsively handle data access changes within the `ink! contract`.

## How It Works

### Configuration and Initialization

After configuring the `conf.json` file and starting the database server, an application is initialized by running the `init` command. This command triggers the database to locate the application's hashtable on the `ink! contract`. It then fetches and stores the latest data image of the application in memory and joins the network of providers. As data is stored in the database, it's propagated to various databases running application operations.

### Starting

When the database is started, it first checks the environment to ensure that necessary infrastructure, such as `ipfs`, is available. The database sets up its network identity, facilitated by `libp2p`. An HTTP server port is also opened, allowing the database to listen for external connections.

### Booting

For the network to function effectively, nodes should not operate in isolation. After starting, the node queries the contract for potential boot nodes. If found, it attempts to connect to them immediately. If there are fewer than 10 nodes in the contract, the node's `multiaddress` is added to the contract storage, allowing others to connect. This is a temporary solution, with reduced reliance on the contract planned in the future.

### Joining

Upon application initialization, the database checks the location of the application's hashtable on the `ink! contract`, retrieves and stores the latest data image in memory, and joins the network of providers.

### Reading and Writing

Data written to an application is gossiped to its peers, following an eventual consistency model, where consistency is based on timestamps.

### Leaving

When a database is shut down, it notifies its directly connected peers and removes its address from the list of boot nodes, in case it happens to be there. This prevents the address from being used when it's unavailable.

### Data Persistence

Each application passes a token within the network, determining which node is responsible for persisting the data state on IPFS by pinning it on its local node and updating the applications IPFS URIs on the contract. New nodes fetch data from this node upon joining the application network of providers. This process occurs as a background task. If configured, remote HTTP servers can receive IPFS CID via POST requests and pin them locally for even more data availabilty. Once specific criteria are met, the token is passed. The token is failure-proof and can only be lost when all nodes are down.

### Data Access Change

The database runs background tasks that periodically query the `ink! contract` storage. The contract is indexed by applications the database cares about. When a change is detected, the node adjusts its internal access control accordingly. Future improvements will include listening to contract events for more immediate responses.

### HTTP Requests

The database server accepts HTTP requests on the port configured in the `conf.json` file. These requests can perform multiple singular operations simultaneously. The database authenticates each request before processing.

Understanding the `libp2p` specification is essential to grasp many of the internal network operations of the database.

## `Cargo Contract`

The `cargo contract` tool is employed exclusively for communication with the contract. It is invoked using the Rust `Command` API, which triggers the operating system to execute the utility. Upon receiving results, they are parsed and presented to the database in a user-friendly format.

Here's an example of using `cargo contract` to interact with `ink!`:

```rust
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
                    "contract invocation returned an error: fn -> `update_ht_cid()`. Trying again...",
                );
                // sleep for 5 seconds
                async_std::task::sleep(CLI_RETRY_DURATION).await;
            }
        }
    }
}

```

## What will we improve on?
1. Provide a solid internal data representation.
2. Provide a better internal structure to accomodate our objectives.
3. Provide a better and efficient overall system architecture and design.
4. Check all points on the `libp2p` networking stack e.g NAT traversal.
5. Listen and react actively to contract events.
6. Make better overall component and implementation descisions.
7. Have fun!
