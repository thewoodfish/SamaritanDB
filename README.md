# SamaritanDB: Your Data, Your Rules

SamaritanDB is a decentralized data store that empowers users to maintain control and sovereignty over the data stored about them by the applications and services they interact with across the internet. To learn more about what SamaritanDB is and why it holds significance, please visit [our wiki](https://algorealm.gitbook.io/samaritandb).

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
- **Cargo Contract**: A Rust package manager for smart contracts.
- Other valuable Rust libraries that enhance the functionality of SamaritanDB.

## Getting Started

### Installation

To get started with SamaritanDB, follow these steps:

1. Clone the repository: `git clone https://github.com/yourusername/samaritandb.git`.
2. Install the [IPFS daemon](https://docs.ipfs.tech/install/).
3. Install [cargo contract](https://crates.io/crates/cargo-contract).

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

- **Manage Data Access (Allow or Deny):**
`access <application_DID> [samaritan_DID] <allow | deny>`

- **Change the Pinning Server for IPFS Updates:**
`config -url <link>`

## How It Works

SamaritanDB works by providing a decentralized and secure data store for users. It employs libp2p and IPFS for peer-to-peer communication and data storage. Smart contracts, built using ink! and managed with Cargo Contract, enable the creation and enforcement of data access rules.

For a detailed explanation of how SamaritanDB functions, please consult the [How It Works](#how-it-works) section in the SamaritanDB documentation.
 
## License

&copy; Copyright 2023 Algorealm, Inc. All rights reserved. SamaritanDB is licensed under the [MIT License](https://github.com/yourusername/samaritandb/blob/main/LICENSE).
