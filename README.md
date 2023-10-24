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

   - In the `conf.json` file, update the `chain_keys` by setting the value to your funded Rococo Contract keys. This step is crucial as the database will interact with the blockchain, including submitting transactions.

## Commands

SamaritanDB provides a set of command-line tools to interact with the database. Here are some common commands:

- `samaritandb init`: Initialize the database.
- `samaritandb add <file>`: Add data to the database.
- `samaritandb get <key>`: Retrieve data from the database.
- `samaritandb remove <key>`: Remove data from the database.
- `samaritandb list`: List all stored data.

For more detailed information on available commands, please refer to the [Commands](#commands) section in the SamaritanDB documentation.

## How It Works

SamaritanDB works by providing a decentralized and secure data store for users. It employs libp2p and IPFS for peer-to-peer communication and data storage. Smart contracts, built using ink! and managed with Cargo Contract, enable the creation and enforcement of data access rules.

For a detailed explanation of how SamaritanDB functions, please consult the [How It Works](#how-it-works) section in the SamaritanDB documentation.

## License

&copy; Copyright 2023 Algorealm, Inc. All rights reserved. SamaritanDB is licensed under the [MIT License](https://github.com/yourusername/samaritandb/blob/main/LICENSE).
