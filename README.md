# SamaritanDB (Key-Value Database)

Assuming you've read the [SamaritanOS wiki](https://algorealm.gitbook.io/samaritanos-a-d-system-for-digital-identity), you know why we are working on SamaritanDB and why it is important. 
<br>
SamaritanDB (KVD) is an in-memory decentralised and distributed key-value database which plays a great role in bringing our vision for the internet to fruition.
<br>
## Prototype
In our bid to break through assumptions and barriers, we have build a prototype that works - version 1.0 (Chevalier). This prototype makes applications store data in a way that leaves control to the user and is free of every centralised storage provider. It is distributed and decentralized. The prototype is also fastly changing as ideas and thinking are being refined and reshaped to create the most effective and efficient product. We will not decieve ourselves.
<br>
## Testing the prototype
You can download the prototype from the page for your suitable devices and configure (nothing to do really) and run them immediately to test the database network.
## Stack
- The database is written in Rust.
- It is built with libp2p as its major networking stack.
- It interacts very often with IPFS
- It also interacts periodically with the [SamaritanOS contract](https://github.com/algorealmInc/samaritanos-contract).
## Prequisites
- You must have IPFS installed on your machine.
- You must be open-minded.
## Commands 
- `new <app|user> <Password>` - This command creates a new identity, whether for an application or a user on the internet and assigns it a DID and private keys.
- `init <application_DID> <Password>` - This command initialized an application on the particular node that runs the database. This means the node begins to care and participate in maintaining, storing and supplying the applications data in the network.
- `set <application_DID> [samaritan_DID] <key> <value>` - This commands stores data about a user or generic data belonging to an application.
- `get <application_DID> [samaritan_DID] <key>` - This commands retrieves data about a user or generic data belonging to an application.
- `del <application_DID> [samaritan_DID] <key>` - This commands deletes data about a user or generic data belonging to an application.
- `exists <application_DID> [samaritan_DID] <key>` - This commands checks if a data entry exists locally in memory.
- `quit` - This command shuts down the database node.
- `truncate <application_DID> [samaritan_DID]` - This commands deletes the entire data entries of a user or an application.
- `info` - This command provides basic information about the database node and its network state.
- `leave <application_DID>` - This command stops the database node from participating in any operation concerning an application, network operations especially. It frees itself of all the applications data.
- `config <application_DID> <flag> <param>` - This command makes configuration changes to the database per application e.g configuring the pinning server URL where subsequent IPFS writes would be POSTed.
- `access <application_DID> <samaritan_DID> <allow|deny>` - This command changes the data access settings of an application in relation to a user.

## Setting Up



