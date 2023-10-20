// Copyright (c) 2023 Algorealm, Inc.

use async_std::sync::RwLock;
use base58::FromBase58;
use futures::channel::mpsc;
use futures::channel::mpsc::Sender;
use futures::channel::oneshot;
use futures::prelude::*;
use futures::select;
use libp2p::gossipsub::IdentTopic;
use libp2p::gossipsub::Message;
use libp2p::gossipsub::MessageId;
use libp2p::multiaddr::{self, Multiaddr, Protocol};
use libp2p::swarm::SwarmEvent;
use libp2p::Swarm;
use libp2p::{
    core::upgrade,
    gossipsub, identify,
    kad::{record::store::MemoryStore, Kademlia, KademliaEvent},
    noise, ping,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmBuilder},
    tcp, yamux, StreamProtocol, Transport,
};
use libp2p_identity::{Keypair, PeerId};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;
use std::hash::Hasher;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::cli;
use crate::{contract, prelude::*, util};

/// There are two loops going on to bootstrap the node. We use the contract to get boot nodes.
/// When we have up to 8 distinct peers in our DHT, we should stop asking the contract.
/// Also when we have at lease one node in our routing table, we should perform Kademlia bootstrap(), only once
/// The flags below help us achieve that

static KADEMLIA_BOOTSTRAP_CALLED: AtomicBool = AtomicBool::new(false);
static UP_TO_8_PEERS: AtomicBool = AtomicBool::new(false);

/// Number of times to try gosssiping again, if failure
const GOSSIP_RETRY_COUNT: u8 = 2;
/// Duration for ping to recover and update itself - 10 minites
const PING_RECOVERY_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Number of history windows in message cache
const MCACHE_LEN: usize = 890;
/// Maximumum number of times a node should update an applications network before passing the ⚽️
const MAX_DATA_UPDATE_COUNT: usize = 7;
/// Frequency for sending SYNC message
const SYNC_FREQUENCY: Duration = Duration::from_secs(10);
/// Extra time to cater for network delay
const ADDITIONAL_WAIT_PERIOD: Duration = Duration::from_secs(10);
/// Any node that has x ping failures in y minutes would be disconnected
const PING_ERROR_THRESHOLD: u8 = 20;
/// Interval between querying the contract for bootstrap nodes
const BOOTSTRAP_INTERVAL: u64 = 300;
/// Amount of time to wait before querying the contract for an applications data access state
const DATA_ACCESS_MONITORING_WAIT_TIME: u64 = 3;

/// The enum represents different peer gossip code. It helps the node differentiate between incoming messages and their corresponsing responses.
pub enum GossipMsgType {
    Data,
    Config,
    Subscribe,
    Unsubscribe,
    Unknown,
}

/// This enum contains the different types of write data
pub enum WriteDataType {
    Access,
    Delete,
    Truncate,
    Write,
    Unknown,
}

/// This enum contains the type of different configurations changes to be made
pub enum NetConfigType {
    PinningServer,
    Unknown,
}

/// A Control message received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlAction {
    /// Node broadcasts known messages per topic - IHave control message.
    IHave {
        /// The topic of the messages.
        topic: String,
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<Vec<u8>>,
    },
    /// The node requests specific message ids (peer_id + sequence _number) - IWant control message.
    IWant {
        /// The ID of the peer requesting the messages
        /// The topic (application) the data belongs to
        topic: String,
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<Vec<u8>>,
    },
    /// The node has been added to the mesh - Graft control message.
    Graft {
        /// peer we're grafting
        peer: SerializedGossipPeer,
        /// The mesh topic the peer should be added to.
        topic: String,
    },
    /// The node has been removed from the mesh - Prune control message.
    Prune {
        /// The mesh topic the peer should be removed from.
        topic: String,
        /// A list of peers to be proposed to the removed peer as peer exchange
        peers: Vec<SerializedGossipPeer>,
        /// The backoff time in seconds before we allow to reconnect
        backoff: Option<u64>,
    },
    /// The node gives a peer the messages requested with IWANT
    IGive {
        /// The list of data that the peer is returning
        messages: Vec<Vec<u8>>,
    },
    /// The node is letting its peers know its still actively updating the network data state for an application
    Sync {
        /// The number of times this node has updated the data image
        update_count: usize,
        /// The application we're interested in
        topic: String,
        /// The list of peers that recieve the message (important for the algorithm)
        peers: Vec<SerializedGossipPeer>,
    },
}

/// A gossipsub RPC message construct
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rpc {
    /// Messages we want to send
    pub messages: Vec<String>,
    /// List of Gossipsub control messages.
    pub control_msgs: Vec<ControlAction>,
}

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "DatabaseEvent")]
pub struct DatabaseBehaviour {
    request_response: request_response::cbor::Behaviour<Rpc, Rpc>,
    kademlia: Kademlia<MemoryStore>,
    gossipsub: gossipsub::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

#[derive(Debug)]
pub enum DatabaseEvent {
    RequestResponse(request_response::Event<Rpc, Rpc>),
    Kademlia(KademliaEvent),
    Gossipsub(gossipsub::Event),
    Ping(ping::Event),
    Identify(identify::Event),
}

impl From<request_response::Event<Rpc, Rpc>> for DatabaseEvent {
    fn from(event: request_response::Event<Rpc, Rpc>) -> Self {
        DatabaseEvent::RequestResponse(event)
    }
}

impl From<KademliaEvent> for DatabaseEvent {
    fn from(event: KademliaEvent) -> Self {
        DatabaseEvent::Kademlia(event)
    }
}

impl From<gossipsub::Event> for DatabaseEvent {
    fn from(event: gossipsub::Event) -> Self {
        DatabaseEvent::Gossipsub(event)
    }
}

impl From<identify::Event> for DatabaseEvent {
    fn from(event: identify::Event) -> Self {
        DatabaseEvent::Identify(event)
    }
}

impl From<ping::Event> for DatabaseEvent {
    fn from(event: ping::Event) -> Self {
        DatabaseEvent::Ping(event)
    }
}

/// The hashmap repesents a map of PeerIDs => (Outbound Error, Latest inbound timestamp). If number of timouts = 5 in 5 minutes, disconnect peer
type PingManager = HashMap<PeerId, (u8, Instant)>;

/// Message to be sent from on channel to the other
pub enum ChannelMsg {
    /// manage routing table bootstraping
    ManageRoutingTableBootstrap {
        sender: Option<mpsc::Sender<ChannelMsg>>,
        data: HashMap<PeerId, Multiaddr>,
        // This is the flag that would determine if we would continue to monitor the contract for other nodes
        monitor_contract: bool,
    },
    /// subscripe to 'an application' and begin gossip
    SubscribeNodeToApplication {
        // DID of application we're subscribing to
        did: String,
        // Database state
        state: Arc<RwLock<DBState>>,
        // Database config
        cfg: Arc<DBConfig>,
        // The CID of the applications hashtable
        hashtable_cid: String,
        // One way channel to delay return of handler while handling network ops
        sender: oneshot::Sender<Result<String, Error>>,
        // Extra useful channel for firing up IPFS synching
        ipfs_helper_channel: Sender<ChannelMsg>,
    },
    /// Gossip data just written to peers
    GossipWriteData {
        // application performing the write
        did: String,
        // Key
        key: String,
        // Value
        value: String,
        // DID that owns this data
        owner_did: String,
        // Modification timestamp. This helps to resolve conflict and lets us know which to write to memory.
        write_time: u64,
        // Database config
        cfg: Arc<DBConfig>,
    },
    /// Gossip delete event to peers
    GossipDeleteEvent {
        // application performing the deletion
        did: String,
        // Key
        key: String,
        // DID that owns this data
        owner_did: String,
        // Modification timestamp. This helps to resolve conflict and lets us know which to enforce
        write_time: u64,
        // Database config
        cfg: Arc<DBConfig>,
    },
    /// Prepare to send GRAFT message
    GossipGraftMsg {
        // Database config
        cfg: Arc<DBConfig>,
        // applications and peers we want to gossip
        peers: HashMap<String, Vec<GossipPeer>>,
    },
    /// Prepare to send GRAFT message
    GossipPruneMsg {
        // applications and peers we want to gossip
        peers: HashMap<String, Vec<GossipPeer>>,
    },
    /// Prepare to send Sync message
    GossipSyncMsg {
        // number of times we have updated the IPFS/contract state
        update_count: usize,
        // application we're interested in
        did: String,
        // Peers we will be contacting
        peers: Vec<GossipPeer>,
    },
    /// Set flag to notify network daemon that data has been modified on this node
    SetNetworkSyncFlag(String), // the string value is the value of the application
    /// Gossip truncate event to peers
    GossipTruncateEvent {
        // application performing the deletion
        did: String,
        // DID that owns this data
        owner_did: String,
        // Modification timestamp. This helps to resolve conflict and lets us know which to enforce
        write_time: u64,
        // Database config
        cfg: Arc<DBConfig>,
    },
    /// unsubscribe from 'an application' and stop recieving updates
    UnsubscribeNodeFromApplication {
        // DID of application we're unubscribing from
        did: String,
        // Database config
        cfg: Arc<DBConfig>,
        // One way channel to delay return of handler while handling network ops
        sender: oneshot::Sender<Result<String, Error>>,
        // Extra useful channel for firing up IPFS synching
    },
    /// shutdown node and inform peers
    Shutdown {
        // Database config
        cfg: Arc<DBConfig>,
        // One way channel to delay return of handler while handling network ops
        sender: oneshot::Sender<Result<String, Error>>,
    },
    /// display most important about the database
    DBInfo,
    /// Inform peers about data access setting modification
    GossipAccessChange {
        // whether to deny or allow access
        perm: bool,
        // application performing the deletion
        did: String,
        // DID that owns this data
        owner_did: String,
        // Modification timestamp. This helps to resolve conflict and lets us know which to enforce
        access_modification_time: u64,
        // Database config
        cfg: Arc<DBConfig>,
    },
    /// Change pinning server of an application
    ChangePinningServer {
        // application performing the deletion
        did: String,
        // new URL we are changing to
        url: String,
        // One way channel to delay return of handler while handling network ops
        sender: oneshot::Sender<Result<String, Error>>,
    },
}

/// Gossipsub parameter configurable constants
#[derive(Debug, Default)]
pub struct GossipSubConfig {
    // The desired outbound degree of the network
    pub d: usize,
    // Lower bound for outbound degree
    pub d_low: usize,
    // Upper bound for outbound degree
    pub d_high: usize,
    // the outbound degree for gossip emission
    pub d_lazy: usize,
    // Time between heartbeats	1 second
    pub heartbeat_interval: Duration,
    // Time-to-live for each topic's fanout state
    pub _fanout_ttl: Duration,
    // Number of history windows to use when emitting gossip
    pub mcache_gossip: usize,
    // Expiry time for cache of seen message ids
    pub _seen_ttl: Duration,
}

/// Structs that holds all the important structures for gossip RPC
#[derive(Debug, Default)]
struct RpcManager {
    seen_msgs: SeenMsgs,
    msg_cache: MessageCache,
}

/// Cache for our most resent RPC data
#[derive(Debug, Default)]
pub struct SeenMsgs {
    msgs: VecDeque<MessageId>,
}

/// Cache entry
impl SeenMsgs {
    /// Method to add data (MessageId) to the cache
    pub fn add_data(&mut self, data: MessageId) {
        // Check if the cache is full (exceeding the history length)
        if self.msgs.len() >= MCACHE_LEN {
            // If the cache is full, remove the oldest data (MessageId) from the front of the deque
            self.msgs.pop_front();
        }

        // Add the new data (MessageId) to the back of the deque
        self.msgs.push_back(data);
    }

    /// Check if a message ID exists in the cache.
    pub fn contains(&self, msg_id: &MessageId) -> bool {
        self.msgs.contains(msg_id)
    }
}

/// Struct for managing gossip message cache
#[derive(Debug, Default)]
struct MessageCache {
    data: HashMap<String, HashMap<MessageId, Vec<u8>>>,
    current_window: Vec<MessageId>,
}

impl MessageCache {
    // Method to add a message to the current window and the cache
    pub fn put(&mut self, topic: String, message_id: MessageId, message_data: Vec<u8>) {
        // Create the topic entry if it doesn't exist
        let topic_map = self.data.entry(topic).or_insert_with(HashMap::new);
        topic_map.insert(message_id.clone(), message_data);

        self.current_window.push(message_id);
        if self.current_window.len() > MCACHE_LEN {
            self.current_window.remove(0);
        }
    }

    // Method to retrieve a message from the cache by its ID, if it is still present
    pub fn get(&self, topic: &str, message_id: &MessageId) -> Option<&Vec<u8>> {
        if let Some(topic_map) = self.data.get(topic) {
            topic_map.get(message_id)
        } else {
            None
        }
    }

    // Method to retrieve the message IDs for messages in the most recent history windows
    // scoped to a given topic
    pub fn get_gossip_ids(&self, topic: &str, mcache_gossip: usize) -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        let windows_to_examine = self.current_window.iter().rev().take(mcache_gossip);
        for message_id in windows_to_examine.cloned() {
            if let Some(topic_map) = self.data.get(topic) {
                if topic_map.contains_key(&message_id) {
                    result.push(message_id.0);
                }
            }
        }
        result
    }

    // Method to shift the current window, discarding messages older than the history length of the cache (mcache_len)
    pub fn shift(&mut self) {
        if self.current_window.len() > MCACHE_LEN {
            let num_to_remove = self.current_window.len() - MCACHE_LEN;
            self.current_window.drain(0..num_to_remove);
        }
    }
}

/// Struct for managing peers. It contains a mapping between topic of interest(application DID) and peers
pub struct PeersManager {
    config: GossipSubConfig,
    mesh_peers: HashMap<String, Vec<GossipPeer>>,
    metadata_peers: HashMap<String, Vec<GossipPeer>>,
    // Fan-out peers
    other_topics: HashMap<String, Vec<GossipPeer>>,
}

impl PeersManager {
    // create a new peer manager with a custom config
    pub fn new_with_config(config: GossipSubConfig) -> Self {
        PeersManager {
            config,
            mesh_peers: HashMap::new(),
            metadata_peers: HashMap::new(),
            other_topics: HashMap::new(),
        }
    }

    // check if fanout peers exists for the did
    // pub fn fanout_peers_exists(&self, did: &str) -> bool {
    //     self.other_topics.contains_key(did)
    // }

    // get fanout peers for a particular application
    pub fn get_fanout_peers(&self, did: &str, outbound_degree: usize) -> Vec<GossipPeer> {
        let mut fanout_peers = if let Some(peers) = self.other_topics.get(did) {
            peers.to_owned()
        } else {
            Default::default()
        };

        // shuffle vector to select randomly as possible
        fanout_peers.shuffle(&mut rand::thread_rng());
        fanout_peers.iter().take(outbound_degree).cloned().collect()
    }
}

/// A Peer in the network
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipPeer {
    pub peer_id: PeerId,
    pub multi_addr: Multiaddr,
}

impl GossipPeer {
    // Serialize the peer_id
    pub fn serialize_id(&self) -> String {
        self.peer_id.to_base58()
    }

    // Serialize the multiaddress
    pub fn serialize_addr(&self) -> String {
        self.multi_addr.to_string()
    }
}

/// A Peer in the network with fields as strings
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SerializedGossipPeer {
    pub peer_id: String,
    pub multi_addr: String,
}

impl SerializedGossipPeer {
    // convert to the deserialized form
    pub fn deserialize(&self) -> DBResult<GossipPeer> {
        let peer_id = PeerId::from_bytes(&self.peer_id.from_base58().unwrap_or_default()).ok();
        let multi_address: Option<Multiaddr> = self.multi_addr.parse().ok();

        if let Some(peer_id) = peer_id {
            if let Some(multi_addr) = multi_address {
                return Ok(GossipPeer {
                    peer_id,
                    multi_addr,
                });
            }
        }

        Err(DBError::ParseError)
    }
}

impl From<GossipPeer> for SerializedGossipPeer {
    fn from(peer: GossipPeer) -> Self {
        SerializedGossipPeer {
            peer_id: peer.serialize_id(),
            multi_addr: peer.serialize_addr(),
        }
    }
}

/// Struct to manage IPFS data synchronizationa and contract update
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IPFSManager {
    /// Peer ID
    peer_id: String,
    /// Application DIDs database is managing and their corresponding hashtable IPFS cid
    cids: HashMap<String, String>,
    /// This flag helps the node know if its the one to perform the synching
    is_node_sync_turn: HashMap<String, bool>,
    /// The number of times we have made a data image update
    data_update_count: HashMap<String, usize>,
    /// Sync recieved peers
    sync_peers: HashMap<String, Vec<SerializedGossipPeer>>,
    /// Last time we recieved a SYNC message
    last_sync_time: HashMap<String, Instant>,
    /// Flag to know if data was altered after latest upload
    modified: HashMap<String, bool>,
    /// URL to POST to for external IPFS pinning. Pinning is independent of the database currently
    pinning_servers: HashMap<String, (String, u64)>,
}

/// Struct to assist the database in managing the restrictions enforced from the contract
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataAccessManager {
    /// restrictions: App DID -> [(User DID, enforced?), ...]
    restrictions: HashMap<String, Vec<String>>,
}

/// Network Client help us interact with the network layer across the application
pub struct NetworkClient {
    pub swarm: Swarm<DatabaseBehaviour>,
    // ONLY used at the initial stages of bootstrapping
    msg_receiver: mpsc::Receiver<ChannelMsg>,
    // For Pings
    ping_manager: PingManager,
    // Database State
    pub db_state: Arc<RwLock<DBState>>,
    // Gossip Manager
    gossipsub: Arc<RwLock<PeersManager>>,
    // RPC manager
    rpc: RpcManager,
    // application data CIDs. For synchronization
    ipfs: Arc<RwLock<IPFSManager>>,
    // network data access changes manager
    access_manager: Arc<RwLock<DataAccessManager>>,
}

impl NetworkClient {
    /// set up look to react to async events
    pub(crate) async fn run(&mut self, sender_channel: mpsc::Sender<ChannelMsg>) {
        loop {
            select! {
                swarm_event = self.swarm.next() => {
                    match swarm_event {
                        Some(event) => {
                            match event {
                                // Handle swarm events
                                SwarmEvent::NewListenAddr { address, .. } => {
                                    util::log_info(&format!("Listening on {:?}", address))
                                }
                                SwarmEvent::Behaviour(DatabaseEvent::Kademlia(KademliaEvent::RoutingUpdated { peer, .. })) => {
                                    util::log_info(&format!("New peer added to the DHT: {:?}", peer));
                                }
                                SwarmEvent::Behaviour(DatabaseEvent::Ping(ping::Event { peer, connection: _, result })) => {
                                    match result {
                                        Ok(duration) => {
                                            // Inbound stream
                                            util::log_info(&format!("Received ping response from peer: {:?}, duration: {:?}", peer, duration));
                                            // add entry into PingManager if it doesn't exist already
                                            match self.ping_manager.get(&peer) {
                                                Some((err_count, _)) => {
                                                    // set instant to now
                                                    self.ping_manager.insert(peer, (*err_count, Instant::now()));
                                                },
                                                None => {
                                                    self.ping_manager.insert(peer, (0, Instant::now()));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            // Outbound stream failure
                                            util::log_error(&format!("Ping error: {}", e));
                                            let ping = self.ping_manager.clone();
                                            match ping.get(&peer) {
                                                Some((err_count, instant)) => {
                                                    self.ping_manager.insert(peer, (*err_count + 1, instant.clone()));

                                                    // now proceed to check if it has reached the PING_ERROR_THRESHOLD
                                                    if ((*err_count + 1) > PING_ERROR_THRESHOLD) && ((Instant::now() - *instant) > PING_RECOVERY_WINDOW) {
                                                        // disconnect peer
                                                        if let Err(_) = self.swarm.disconnect_peer_id(peer) {
                                                            util::log_error(&format!("Failed to disconnect from unresponsive peer {peer:?}"));
                                                        } else {
                                                            util::log_error(&format!("Peer disconected due to ping failures: {peer:?}"));
                                                        }

                                                        // remove from manager
                                                        self.ping_manager.remove(&peer);
                                                    }
                                                },
                                                None => {
                                                    self.ping_manager.insert(peer, (1, Instant::now()));
                                                }
                                            }
                                        }
                                    }
                                }
                                SwarmEvent::Behaviour(DatabaseEvent::Identify(identify::Event::Sent { peer_id, .. })) => {
                                    util::log_info(&format!("Sent identify info to {peer_id:?}"))
                                }
                                // Prints out the info received via the identify event
                                SwarmEvent::Behaviour(DatabaseEvent::Identify(identify::Event::Received { peer_id, info })) => {
                                    util::log_info(&format!("Received identify info from {peer_id}: {info:?}"));
                                }
                                // Handle the RPC events
                                SwarmEvent::Behaviour(DatabaseEvent::RequestResponse(
                                    request_response::Event::Message { message, .. },
                                )) => match message {
                                    // handle incoming requests
                                    request_response::Message::Request {
                                        request, channel, ..
                                    } => {
                                        let rpc: Rpc = request.into();
                                        // Match the first control message, we'll be considering only one for now (as it should be though)
                                        match &rpc.control_msgs[0] {
                                            // IHAVE message received from some peer
                                            ControlAction::IHave { topic, message_ids } => {
                                                let peer = &rpc.messages[0];
                                                util::log_info(&format!("IHAVE: Recieved message from {}", peer));

                                                // The messages we want
                                                let mut iwant_msgs = Vec::new();
                                                // lets examine each and decide which one's we might need
                                                for bytes in message_ids {
                                                    let msg_id = MessageId::new(&bytes[..]);
                                                    // First check the last seen messages. If its there, we need not bother
                                                    if !self.rpc.seen_msgs.contains(&msg_id) {
                                                        // now check per topic, if its exists
                                                        if let None = self.rpc.msg_cache.get(&topic, &msg_id) {
                                                            // add to the messages we want
                                                            iwant_msgs.push(msg_id.0);
                                                        }
                                                    }
                                                }

                                                // now send an IWANT message to our kind peer
                                                let mut control_msgs = Vec::new();
                                                control_msgs.push(ControlAction::IWant { topic: topic.to_owned(), message_ids: iwant_msgs });

                                                let msg = Rpc {
                                                    // we're sending the peerID as the message content
                                                    messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                                    control_msgs
                                                };

                                                // Send the RPC
                                                let _ = self.swarm.behaviour_mut().request_response.send_response(channel, msg);
                                            }

                                            ControlAction::IWant { .. } => {}

                                            // We have received request to GRAFT a peer into our full-message peer
                                            ControlAction::Graft { topic, peer } => {
                                                util::log_info(&format!("GRAFT: Recieved graft request from {}", peer.peer_id));

                                                // We'll just add the peer straight and let the daemon do the rest
                                                let graft_peer = peer.deserialize();
                                                if let Ok(peer) = graft_peer {
                                                    if let Some(mesh_peers) = self.gossipsub.write().await.mesh_peers.get_mut(topic) {
                                                        if !mesh_peers.contains(&peer) {
                                                            // add to our mesh peer
                                                            mesh_peers.push(peer.clone());
                                                        }
                                                    }
                                                }
                                            }

                                            // PRUNE message received from some peer
                                            ControlAction::Prune {
                                                topic,
                                                peers,
                                                backoff: _,
                                            } => {
                                                // remove the peer from the node's full-message peers
                                                let sender_peer = &rpc.messages[0];
                                                util::log_info(&format!("PRUNE: Recieved {} peers from {}", peers.len(), topic));
                                                let mut mesh_peers = if let Some(peers) = self.gossipsub.read().await.mesh_peers.get(topic) {
                                                    peers.iter().filter(|peer| peer.peer_id.to_base58() != *sender_peer).cloned()
                                                    .collect::<Vec<GossipPeer>>()
                                                } else {
                                                    Default::default()
                                                };

                                                // Peer Exchange: Add peers to full-message peers
                                                let _ = peers.iter().map(|peer| peer.deserialize()).filter_map(Result::ok).map(|peer| {
                                                    mesh_peers.push(peer.to_owned());
                                                }).collect::<Vec<_>>();
                                            }

                                            // Handle the data we requested through IWant
                                            ControlAction::IGive { messages } => {
                                                for msg in messages {
                                                    // First stringify the bytes
                                                    if let Ok(msg) = String::from_utf8(msg.to_owned()) {
                                                        // handle the message the way we'll handle a message gossip form our full-message peers
                                                        let msg_data = msg.split("$$").collect::<Vec<_>>();
                                                        // global applicable variables
                                                        let did = msg_data[1];
                                                        let msg_type = msg_data[0].parse::<u8>().unwrap_or_default();

                                                        util::log_info(&format!("IGIVE: recieved {} bytes from peer: {}", msg.as_bytes().len(), did));

                                                        let msg_type = match msg_type {
                                                            0 => GossipMsgType::Data,
                                                            1 => GossipMsgType::Config,
                                                            _ => GossipMsgType::Unknown,
                                                        };

                                                        match msg_type {
                                                            // handle data
                                                            GossipMsgType::Data => {
                                                                // let the network manager know
                                                                self.ipfs.write().await.modified.insert(did.to_owned(), true);
                                                                // now handle the data in respect to the local database
                                                                handle_gossiped_data(self.db_state.clone(), did.to_owned(), msg_data[2].to_owned(), msg_data[3].to_owned(), msg_data[4].to_owned(), msg_data[5].to_owned(), msg_data[8].to_owned()).await;
                                                            },
                                                            // handle config
                                                            GossipMsgType::Config => {
                                                                // handle change in configuration
                                                                let cfg = msg_data[8].parse::<u8>().unwrap_or_default();
                                                                let update_time = msg_data[5].parse::<u64>().unwrap_or_default();
                                                                let cfg_type = match cfg {
                                                                    0 => NetConfigType::PinningServer,
                                                                    _ => NetConfigType::Unknown,
                                                                };

                                                                match cfg_type {
                                                                    // modify pinning server
                                                                    NetConfigType::PinningServer => {
                                                                        let url = msg_data[2].to_owned();
                                                                        let now = util::current_unix_epoch();
                                                                        let mut ipfs_lock = self.ipfs.write().await;

                                                                        if let Some(cfg) = ipfs_lock.pinning_servers.get_mut(did) {
                                                                            // make sure its the latest update
                                                                            if update_time > cfg.1 {
                                                                                // change the URL
                                                                                cfg.0 = url.clone();
                                                                                // update timestamp incase of network conflict
                                                                                cfg.1 = now;
                                                                            }
                                                                        } else {
                                                                            let url_cfg = (url.clone(), now);
                                                                            ipfs_lock.pinning_servers.insert(did.to_string(), url_cfg);
                                                                        }

                                                                        util::log_info(&format!("Pinning server changed: Application: ({}), server URL: ({})", did, url.clone()));
                                                                    },
                                                                    _ => {}
                                                                }
                                                            },
                                                            _ => {}
                                                        }
                                                    } else {
                                                        util::log_error(&format!("Could not decode full-message bytes from peer"));
                                                    }
                                                }
                                            },

                                            ControlAction::Sync { update_count, topic, peers } => {
                                                util::log_info(&format!("SYNC: recieved synching info for: {topic}"));
                                                // To avoid duplicate providers, make sure we're not also providing
                                                // So we check if we're also with the ball, if we are, we resolve the conflict
                                                let syncing = self.ipfs.read().await.is_node_sync_turn.get(topic).unwrap_or(&false).to_owned();
                                                // compare the times of update to each other
                                                if syncing {
                                                    // try to resolve (The node with the greater update count stays)
                                                    let mut ipfs_lock = self.ipfs.write().await;
                                                    if let Some(node_update_count) = ipfs_lock.data_update_count.get(topic) {
                                                        if *node_update_count <= *update_count {
                                                            // give up with updating, the real guy is back
                                                            util::log_info(&format!("conflict: data handler status relinquished"));
                                                            // reset count
                                                            ipfs_lock.data_update_count.insert(topic.to_owned(), 0);
                                                            ipfs_lock.is_node_sync_turn.insert(topic.to_owned(), false);
                                                        }
                                                    }
                                                }

                                                // save sync peers first
                                                self.ipfs.write().await.sync_peers.insert(topic.to_owned(), peers.to_owned());

                                                // save the last time we recieved a SYNC message
                                                self.ipfs.write().await.last_sync_time.insert(topic.to_owned(), Instant::now());
                                            }
                                        }
                                    }

                                    // handle outgoing requests
                                    request_response::Message::Response {
                                        request_id: _,
                                        response,
                                    } => {
                                        let rpc: Rpc = response.into();
                                        // Match the first control message, we'll be considering only one for now (as it should be though)
                                        match &rpc.control_msgs[0] {
                                            // IHAVE message received from some peer
                                            ControlAction::IHave { .. } => { /* no response as IHAVE */}
                                            // One of our metadata peers has requested some data about an application
                                            ControlAction::IWant { topic, message_ids } => {
                                                util::log_info(&format!("IWANT: Recieved {} messages from peer: {}", message_ids.len(), topic));
                                                // get the topic and the data if they still exist in the cache
                                                if let Some(topic_data) = self.rpc.msg_cache.data.get(topic) {
                                                    let mut return_data: Vec<Vec<u8>> = Vec::new();
                                                    for (message_id, (msg_id, msg)) in message_ids.iter().zip(topic_data.iter()) {
                                                        if *message_id == msg_id.0 {
                                                            return_data.push(msg.to_owned());
                                                        }
                                                    }

                                                    // send if any of the data exists
                                                    if return_data.len() > 0 {
                                                        // send
                                                        let mut control_msgs = Vec::new();
                                                        control_msgs.push(ControlAction::IGive { messages: return_data });

                                                        let msg = Rpc {
                                                            // we're sending the peerID as the message content
                                                            messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                                            control_msgs
                                                        };

                                                        if let Ok(recipient_peer) = PeerId::from_bytes(rpc.messages[0].as_bytes()) {
                                                            // Send the RPC
                                                            let _ = self.swarm.behaviour_mut().request_response.send_request(&recipient_peer, msg);
                                                        }
                                                    }
                                                }
                                            }

                                            ControlAction::Graft { .. } => { /* we're not grafting through here for now */ }
                                            ControlAction::Prune { .. } => { /* no pruning response */}
                                            ControlAction::IGive { .. } => { /* no giving response */}
                                            ControlAction::Sync { .. } => { /* no syncing response */}
                                        }
                                    }
                                },
                                // Handle incoming message stream from gossip peers
                                SwarmEvent::Behaviour(DatabaseEvent::Gossipsub(gossipsub::Event::Message {
                                    propagation_source: peer_id,
                                    message_id,
                                    message,
                                })) => {
                                    let _ = if let Some(source) = message.source { source.to_owned() } else { peer_id };
                                    util::log_info(&format!("Recieved {} bytes from peer: {}", message.data.len(), peer_id.to_base58()));

                                    // get message data
                                    let msg_data = String::from_utf8(message.data.clone()).unwrap_or_default();
                                    let msg_data = msg_data.split("$$").collect::<Vec<_>>();
                                    let msg_code = msg_data[0].parse::<u8>();
                                    // global applicable variables
                                    let did = msg_data[1];
                                    match msg_code {
                                        Ok(code) => {
                                            let msg_type = match code {
                                                0 => GossipMsgType::Data,
                                                1 => GossipMsgType::Config,
                                                2 => GossipMsgType::Subscribe,
                                                3 => GossipMsgType::Unsubscribe,
                                                _ => GossipMsgType::Unknown,
                                            };
                                            match msg_type {
                                                GossipMsgType::Data => {
                                                    // handle normal gossip message
                                                    // first check if node is subscribed to the application
                                                    if self.db_state.read().await.is_application_init(did) {
                                                        // Check if message is from this node
                                                        if *self.swarm.local_peer_id() != peer_id {
                                                            // check if we've seen it before
                                                            if !self.rpc.seen_msgs.contains(&message_id) {
                                                                // let the network manager know
                                                                self.ipfs.write().await.modified.insert(did.to_owned(), true);

                                                                // now handle the data in respect to the local database
                                                                handle_gossiped_data(self.db_state.clone(), did.to_owned(), msg_data[2].to_owned(), msg_data[3].to_owned(), msg_data[4].to_owned(), msg_data[5].to_owned(), msg_data[8].to_owned()).await;
                                                                // Now, we fufill all righteousness

                                                                // Add it to our seen cache
                                                                self.rpc.seen_msgs.add_data(message_id.clone());
                                                                // Second, add it to our message cache
                                                                self.rpc.msg_cache.put(did.to_owned(), message_id.clone(), message.data.clone());

                                                                // send message
                                                                gossip_msg(&mut self.swarm, message, did, self.gossipsub.clone(), &mut self.rpc, message_id).await;

                                                                // shift cache
                                                                self.rpc.msg_cache.shift();
                                                            }
                                                        }
                                                    } else {
                                                        // first add the peer to our fanout peers
                                                        let multi_address: Result<Multiaddr, _> = msg_data[6].parse();
                                                        if let Ok(multi_addr) = multi_address {
                                                            let peer = GossipPeer {
                                                                peer_id,
                                                                multi_addr: multi_addr.clone()
                                                            };

                                                            let mut gossipsub_lock = self.gossipsub.write().await;

                                                            match gossipsub_lock.other_topics.entry(did.to_string()) {
                                                                Entry::Occupied(mut entry) => {
                                                                    let fanout_peers = entry.get_mut();
                                                                    fanout_peers.push(peer);
                                                                }
                                                                Entry::Vacant(entry) => {
                                                                    let mut fanout_peers: Vec<GossipPeer> = Vec::new();
                                                                    fanout_peers.push(peer);
                                                                    entry.insert(fanout_peers);
                                                                }
                                                            }

                                                            // add to routing table
                                                            self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multi_addr);
                                                        }

                                                        // Then send a peer a graft message, so it can remove this node from its full-message peers
                                                        // construct an RPC

                                                        // Take nodes fanout peers to perfom peer exchange
                                                        let pe_peers = if let Some(peers) = self.gossipsub.read().await.other_topics.get(did) {
                                                            peers.iter().cloned().map(|peer| {
                                                                peer.into()
                                                            }).collect::<Vec<SerializedGossipPeer>>()
                                                        } else {
                                                            Default::default()
                                                        };

                                                        let mut control_msgs = Vec::new();
                                                        control_msgs.push(ControlAction::Prune { topic: did.to_string(), peers: pe_peers, backoff: None });

                                                        let message = Rpc {
                                                            // we're sending the peerID as the message content
                                                            messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                                            control_msgs
                                                        };

                                                        // send using request-response
                                                        self.swarm.behaviour_mut().request_response.send_request(&peer_id, message);

                                                    }
                                                },
                                                GossipMsgType::Subscribe => {
                                                    // handle subscription message
                                                    // First check if we have any interest in the application the message is about
                                                    if self.db_state.read().await.is_application_init(did) {
                                                        if self.ipfs.read().await.peer_id != peer_id.to_base58() {
                                                            if let Ok(multi_addr) = msg_data[2].parse::<Multiaddr>() {
                                                                self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                                                let mut gossipsub_lock = self.gossipsub.write().await;
                                                                gossipsub_lock.mesh_peers
                                                                    .entry(did.to_string())
                                                                    .or_insert_with(Vec::new)
                                                                    .push(GossipPeer { peer_id, multi_addr });
                                                                util::log_info(&format!("GRAFT: Added peer {} to mesh peers - {}", msg_data[3], did));
                                                            }
                                                        }
                                                    } else if let Ok(multi_addr) = msg_data[2].parse::<Multiaddr>() {
                                                        if let Ok(peer_id) = PeerId::from_bytes(msg_data[3].as_bytes()) {
                                                            let peer = GossipPeer { peer_id, multi_addr: multi_addr.clone() };
                                                            let mut gossipsub_lock = self.gossipsub.write().await;
                                                            gossipsub_lock.other_topics
                                                                .entry(did.to_string())
                                                                .or_insert_with(Vec::new)
                                                                .push(peer);
                                                            self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multi_addr);
                                                        }
                                                    }

                                                },
                                                GossipMsgType::Unsubscribe => {
                                                    // handle unsubscribe message
                                                    // Remove peer from full-message peers and then add it to fan-out peers
                                                    if self.db_state.read().await.is_application_init(did) {
                                                        if self.ipfs.read().await.peer_id != peer_id.to_base58() {
                                                            if let Ok(multi_addr) = msg_data[2].parse::<Multiaddr>() {
                                                                self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                                                let mut gossipsub_lock = self.gossipsub.write().await;
                                                                gossipsub_lock.mesh_peers.remove(did);
                                                                gossipsub_lock.other_topics
                                                                    .entry(did.to_string())
                                                                    .or_insert_with(Vec::new)
                                                                    .push(GossipPeer { peer_id, multi_addr: multi_addr.clone() });

                                                                util::log_info(&format!("PRUNE: Peer `{}` removed from mesh peers - `{}`", msg_data[3], did));
                                                            }
                                                        }
                                                    } else {
                                                        util::log_error(&format!("UNSUBSCRIBE: Application not recognized: `{}`", did))
                                                    }
                                                },
                                                GossipMsgType::Config => {
                                                    // handle change in configuration
                                                    let cfg = msg_data[8].parse::<u8>().unwrap_or_default();
                                                    let update_time = msg_data[5].parse::<u64>().unwrap_or_default();
                                                    let cfg_type = match cfg {
                                                        0 => NetConfigType::PinningServer,
                                                        _ => NetConfigType::Unknown,
                                                    };

                                                    match cfg_type {
                                                        // modify pinning server
                                                        NetConfigType::PinningServer => {
                                                            let url = msg_data[2].to_owned();
                                                            let now = util::current_unix_epoch();
                                                            let mut ipfs_lock = self.ipfs.write().await;

                                                            if let Some(cfg) = ipfs_lock.pinning_servers.get_mut(did) {
                                                                // make sure its the latest update
                                                                if update_time > cfg.1 {
                                                                    // change the URL
                                                                    cfg.0 = url.clone();
                                                                    // update timestamp incase of network conflict
                                                                    cfg.1 = now;
                                                                }
                                                            } else {
                                                                let url_cfg = (url.clone(), now);
                                                                ipfs_lock.pinning_servers.insert(did.to_string(), url_cfg);
                                                            }

                                                            util::log_info(&format!("Pinning server changed: Application: ({}), server URL: ({})", did, url.clone()));

                                                            // gossip it to all peers
                                                            // send message
                                                            gossip_msg(&mut self.swarm, message, did, self.gossipsub.clone(), &mut self.rpc, message_id).await;

                                                            // shift cache
                                                            self.rpc.msg_cache.shift();
                                                        },
                                                        NetConfigType::Unknown => {}
                                                    }

                                                },
                                                GossipMsgType::Unknown => { /* ignore */},
                                            }
                                        },
                                        Err(_) => {
                                            // handle parsing error here by doing nothing
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        None => break, // Exit the loop when swarm stream ends
                    }
                }
                msg_event = self.msg_receiver.next() => {
                    match msg_event {
                        Some(event) => {
                            match event {
                                ChannelMsg::ManageRoutingTableBootstrap {
                                    sender: _,
                                    data,
                                    monitor_contract: _,
                                } => {
                                    // try to add to our nodes routing table
                                    for (peer_id, multiaddress) in data {
                                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddress);
                                    }

                                    // if bootstrap() has not been called
                                    if !KADEMLIA_BOOTSTRAP_CALLED.load(Ordering::SeqCst) {
                                        KADEMLIA_BOOTSTRAP_CALLED.store(true, Ordering::SeqCst);
                                        let _ = self.swarm.behaviour_mut().kademlia.bootstrap();
                                    }

                                    let mut peers_count = 0;
                                    let _ = self.swarm.behaviour_mut().kademlia.kbuckets().map(|k| {
                                        peers_count += k.num_entries();
                                    }).collect::<Vec<_>>();

                                    // also check for the routing table
                                    if peers_count > 7 {
                                        UP_TO_8_PEERS.store(true, Ordering::SeqCst);
                                        util::log_info("Bootstrapping from contract complete");
                                    }
                                },
                                ChannelMsg::SubscribeNodeToApplication {
                                    did,
                                    state,
                                    cfg,
                                    hashtable_cid,
                                    sender,
                                    ipfs_helper_channel
                                } => {
                                    // first add the cid to the IPFS synchronization state
                                    self.ipfs.write().await.cids.insert(did.clone(), hashtable_cid);

                                    // get ip
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        let local_pid = self.swarm.local_peer_id().clone();
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid))
                                    } else { Multiaddr::empty() };

                                    // create new topic {did} and subscribe to it
                                    let topic = IdentTopic::new(did.clone());

                                    if let Err(_) = self.swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                                        util::log_error(&format!("Failed to subscribe to topic {did}"));

                                        // clean up memory relating to DID
                                        state.write().await.remove_application(&did);

                                        // return back to caller
                                        let _ = sender.send(Err(Box::new(DBError::InitializationError)));
                                    } else {
                                        // We need to find peers that is subscribed to the application
                                        // First we check our local peer manager
                                        let mut gossipsub_lock = self.gossipsub.write().await;
                                        let mut fanout_peers = match gossipsub_lock.other_topics.entry(did.to_string()) {
                                            Entry::Occupied(_) => {
                                                gossipsub_lock.get_fanout_peers(&did, gossipsub_lock.config.d.into())
                                            }
                                            Entry::Vacant(_) => Vec::new(),
                                        };

                                        // try to get from the contract if we dont have the number we need: (gossip peers + full-message peers)
                                        let d = gossipsub_lock.config.d.clone();
                                        let d_lazy = gossipsub_lock.config.d_lazy.clone();
                                        let subscribed_peers = if fanout_peers.len() < (d + d_lazy) as usize {
                                            drop(gossipsub_lock);
                                            // Next, check contract for peers that are running operations for the application
                                            let subscribed_peers = util::parse_multiaddresses(&contract::get_subscribers(cfg.clone(), &did).await);
                                            util::log_info(&format!("Peers subscribed to {}: {}", did, subscribed_peers.len()));
                                            subscribed_peers
                                        } else {
                                            HashMap::new()
                                        };

                                        // we want to add this node to the list of subscribers
                                        let ndid = did.clone();
                                        let multiaddr = multi_addr.clone();
                                        let config = cfg.clone();
                                        async_std::task::spawn(async move {
                                            if !multiaddr.is_empty() {
                                                util::log_info(&format!("subscribing node to application: {ndid}"));
                                                contract::subscribe_node(config, &ndid, &format!("{multiaddr}")).await;
                                            }
                                        });

                                        // Fill up our fanout peers with the peers we get from the contract
                                        for (peer_id, transport_multiaddr) in subscribed_peers.iter() {
                                            // make sure we're not adding ourselves
                                            let node_pid = self.ipfs.read().await.peer_id.clone();
                                            if *peer_id != node_pid {
                                                // add to application gossip peers
                                                if let Ok(peer_id) = PeerId::from_bytes(&peer_id.from_base58().unwrap_or_default()) {
                                                    // construct multiaddress
                                                    let multi_address: Result<Multiaddr, _> = Multiaddr::from_str(transport_multiaddr);
                                                    if let Ok(multi_addr) = multi_address {
                                                        let peer = GossipPeer {
                                                            multi_addr: multi_addr.clone(),
                                                            peer_id
                                                        };
                                                        if !fanout_peers.contains(&peer) {
                                                            fanout_peers.push(peer);
                                                        }

                                                        // add to our routing table
                                                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multi_addr);
                                                    }
                                                }

                                                // if we have enough for our full-message and metadata peers, stop looping
                                                let gossipsub_lock = self.gossipsub.read().await;
                                                let d = gossipsub_lock.config.d.clone();
                                                let d_lazy = gossipsub_lock.config.d_lazy.clone();
                                                if fanout_peers.len() >= (d + d_lazy) as usize {
                                                    break
                                                }
                                            }
                                        }

                                        // spin up task to handle network data sync
                                        let config = cfg.clone();
                                        let state = self.db_state.clone();
                                        let ipfs = self.ipfs.clone();
                                        let gossip = self.gossipsub.clone();
                                        let ipfs_sender_channel = ipfs_helper_channel.clone();
                                        let d_id = did.clone();
                                        async_std::task::spawn(async move {
                                            manage_data_synchronization(d_id, config, state, ipfs, gossip, ipfs_sender_channel).await;
                                        });

                                        // spin up task to read from the contract periodically to enforce data access changes as soon as possible
                                        let _did = did.clone();
                                        let _cfg = cfg.clone();
                                        let _a_mngr = self.access_manager.clone();
                                        let _state = self.db_state.clone();
                                        let _sender = sender_channel.clone();
                                        async_std::task::spawn(async move {
                                            manage_data_access_changes(_did, _cfg, _a_mngr, _state, _sender).await;
                                        });

                                        let mut full_msg_peers: Vec<GossipPeer> = Vec::new();
                                        let mut metadata_peers: Vec<GossipPeer> = Vec::new();

                                        for (index, peer) in fanout_peers.iter().enumerate() {
                                            if index < Into::<usize>::into(self.gossipsub.read().await.config.d) {
                                                // equip our full-message mesh peers
                                                self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer.peer_id);
                                                full_msg_peers.push(peer.clone());
                                            } else {
                                                // equip our metadata-only peers
                                                metadata_peers.push(peer.clone());
                                            }
                                        }

                                        // insert into our mesh
                                        self.gossipsub.write().await.mesh_peers.insert(did.clone(), full_msg_peers.clone());

                                        // insert into our metadata peers
                                        self.gossipsub.write().await.metadata_peers.insert(did.clone(), metadata_peers.clone());

                                        // send subscribe message to our mesh peers, so they can GRAFT us into their peer addrees book
                                        let message = format!("{}$${}$${}$${}", GossipMsgType::Subscribe as u8, did.clone(), /* add multiaddress, so it can be dailed */ multi_addr, self.swarm.local_peer_id().clone());
                                        let _ = self.swarm
                                            .behaviour_mut().gossipsub
                                            .publish(IdentTopic::new(did.clone()), message.as_bytes());

                                        util::log_info(&format!("Subscribe message published for application: {did}"));
                                        let _ = sender.send(Ok(format!("Application `{did}` has been added to database")));

                                        // dail all our full-message peers
                                        for peer in full_msg_peers {
                                            match self.swarm.dial(peer.clone().multi_addr) {
                                                Ok(_) => {
                                                    util::log_info(&format!("DIAL: successful dailing of {:?}", peer.peer_id.to_base58()));
                                                }
                                                Err(_) => {
                                                    util::log_error(&format!("DIAL: failed to dial {} as {}", peer.peer_id.to_base58(), peer.multi_addr));
                                                }
                                            }
                                        }

                                        // If we don't receive SYNC messages for 3 minutes, seize power and start data update
                                        let ipfs = self.ipfs.clone();
                                        async_std::task::spawn(async move {
                                            // sleep for 3 minutes
                                            let three_minutes = Duration::from_secs(180);
                                            async_std::task::sleep(three_minutes).await;
                                            // when it wakes, perform necessary checks
                                            let node_turn = ipfs.read().await.is_node_sync_turn.get(&did).unwrap_or(&false).to_owned();
                                            // Get the current time
                                            let now = Instant::now();
                                            // Define a duration of 10 minutes ago
                                            let ten_minutes = Duration::from_secs(10 * 60);
                                            // Subtract 10 minutes from the current time
                                            let ten_minutes_ago = now - ten_minutes;
                                            let three_minutes_ago = now - three_minutes;
                                            // get last sync
                                            let last_sync = ipfs.read().await.last_sync_time.get(&did).unwrap_or(&ten_minutes_ago).to_owned();
                                             // get sync peers
                                            let sync_peers = ipfs
                                                .read()
                                                .await
                                                .sync_peers
                                                .get(&did)
                                                .unwrap_or(&Default::default())
                                                .to_owned();

                                            if (!node_turn && last_sync < three_minutes_ago) && sync_peers.len() == 0 {
                                                // seize power
                                                ipfs.write().await
                                                    .is_node_sync_turn
                                                    .insert(did, true);
                                            }

                                            util::log_info("~ acquired responsibility for network's data update");
                                        });
                                    }
                                }
                                ChannelMsg::GossipWriteData { did, key, value, owner_did, write_time, cfg } => {
                                    // We want to send our just written data to our various peers
                                    // get multiaddress
                                    let local_pid = self.swarm.local_peer_id().clone();
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid.clone()))
                                    } else { Multiaddr::empty() };

                                    // arrange the message into a custom format
                                    let message = format!("{}$${}$${}$${}$${}$${}$${}$${}$${}", GossipMsgType::Data as u8, did, key, value, owner_did, write_time, multi_addr, self.swarm.local_peer_id().clone(), WriteDataType::Write as u8);

                                    // publish message
                                    publish_msg(&mut self.swarm, message, did, self.gossipsub.clone(), "Message published to metadata peer").await;
                                }

                                // send GRAFT messages to peers
                                ChannelMsg::GossipGraftMsg { cfg, peers } => {
                                    if let Some(ip) = local_ip::get() {
                                        let peer_id = self.swarm.local_peer_id().clone();
                                        let multi_addr =  construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(peer_id));

                                        // We'll send our details, for easy grafting
                                        let graft_peer = GossipPeer {
                                            peer_id,
                                            multi_addr
                                        };

                                        for (topic, peers) in peers {
                                            for peer in peers {
                                                let mut control_msgs = Vec::new();
                                                control_msgs.push(ControlAction::Graft { peer: graft_peer.clone().into(), topic: topic.clone() });

                                                let message = Rpc {
                                                    // we're sending the peerID as the message content
                                                    messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                                    control_msgs
                                                };

                                                // send using request-response
                                                self.swarm.behaviour_mut().request_response.send_request(&peer.peer_id, message);
                                            }
                                        }
                                    }
                                },
                                // send PRUNE messages to peers
                                ChannelMsg::GossipPruneMsg { peers } => {
                                    for (topic, peers) in peers {
                                        // First find peers we can serve up to the peers we are pruning (Peer Exchange)
                                        let pe_peers = if let Some(peers) = self.gossipsub.read().await.other_topics.get(&topic) {
                                            peers.iter().cloned().map(|peer| {
                                                peer.into()
                                            }).collect::<Vec<SerializedGossipPeer>>()
                                        } else {
                                            Default::default()
                                        };

                                        for peer in peers.iter() {
                                            let mut control_msgs = Vec::new();
                                            control_msgs.push(ControlAction::Prune { topic: topic.clone(), peers: pe_peers.to_owned().into(), backoff: None });

                                            let message = Rpc {
                                                // we're sending the peerID as the message content
                                                messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                                control_msgs
                                            };

                                            // send using request-response
                                            self.swarm.behaviour_mut().request_response.send_request(&peer.peer_id, message);
                                        }
                                    }
                                },
                                // send SYNC messages to peers
                                ChannelMsg::GossipSyncMsg { update_count, did, peers } => {
                                    // serialize it
                                    let s_peers = peers.iter().map(|p| Into::<SerializedGossipPeer>::into(p.to_owned())).collect::<Vec<_>>();
                                    for peer in peers {
                                        let mut control_msgs = Vec::new();
                                        control_msgs.push(ControlAction::Sync { update_count, topic: did.clone(), peers: s_peers.to_owned() });

                                        let message = Rpc {
                                            // we're sending the peerID as the message content
                                            messages: vec![self.swarm.local_peer_id().clone().to_base58()],
                                            control_msgs
                                        };
                                        // send using request-response
                                        self.swarm.behaviour_mut().request_response.send_request(&peer.peer_id, message);
                                    }

                                    // now check if we're done with our part and ready to pass the ⚽️
                                    let mut ipfs = self.ipfs.write().await;
                                    let count = ipfs.data_update_count.get(&did).unwrap_or(&0).to_owned();
                                    if count != 0  {
                                        // first check if increasing it will reach its cap
                                        if count + 1 == MAX_DATA_UPDATE_COUNT {
                                            // stop synching and pass the ⚽️
                                            util::log_info(&format!("⚽️: data handler status passed on to peer"));
                                            // reset count
                                            ipfs.data_update_count.insert(did.to_owned(), 0);
                                            ipfs.is_node_sync_turn.insert(did.to_owned(), false);

                                            // update sync time for the relinquishing peer to the future,
                                            // so this peer would not be the first to acquire the syching rights
                                            ipfs.last_sync_time.insert(did, Instant::now() + Duration::from_secs(20));
                                        } else {
                                            // increase it
                                            ipfs.data_update_count.insert(did.to_owned(), count + 1);
                                        }
                                    } else {
                                        // begin counting
                                        ipfs.data_update_count.insert(did.to_owned(), count + 1);
                                    }
                                },
                                // Let our peers know that we've deleted the data
                                ChannelMsg::GossipDeleteEvent { cfg, did, key, write_time, owner_did } => {
                                    // get multiaddress
                                    let local_pid = self.swarm.local_peer_id().clone();
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid.clone()))
                                    } else { Multiaddr::empty() };

                                    // arrange the message into a custom format
                                    let message = format!("{}$${}$${}$${}$${}$${}$${}$${}$${}", GossipMsgType::Data as u8, did, key, /* no value */"", owner_did, write_time, multi_addr, self.swarm.local_peer_id().clone(), WriteDataType::Delete as u8);

                                    // publish message
                                    publish_msg(&mut self.swarm, message, did, self.gossipsub.clone(), "Delete event published to metadata peers").await;
                                },

                                // Set modified flag to true for application, so it can be written to the network if need be
                                ChannelMsg::SetNetworkSyncFlag(did) =>  {
                                    self.ipfs.write().await.modified.insert(did, true);
                                },
                                // Let our peers know we just truncated data belonging to a DID
                                ChannelMsg::GossipTruncateEvent { cfg, did, write_time, owner_did } => {
                                    // get multiaddress
                                    let local_pid = self.swarm.local_peer_id().clone();
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid.clone()))
                                    } else { Multiaddr::empty() };

                                    // arrange the message into a custom format
                                    let message = format!("{}$${}$${}$${}$${}$${}$${}$${}$${}", GossipMsgType::Data as u8, did, /* no key */ "", /* no value */"", owner_did, write_time, multi_addr, self.swarm.local_peer_id().clone(), WriteDataType::Truncate as u8);

                                    // publish message
                                    publish_msg(&mut self.swarm, message, did, self.gossipsub.clone(), "Truncate event published to metadata peers").await;
                                },
                                // Unsubscribe from topic (application) and stop recieving data from peers
                                ChannelMsg::UnsubscribeNodeFromApplication {
                                    did,
                                    cfg,
                                    sender,
                                } => {
                                    // first remove the cid from the IPFS synchronization state
                                    self.ipfs.write().await.cids.remove(&did);

                                    // get ip
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        let local_pid = self.swarm.local_peer_id().clone();
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid))
                                    } else { Multiaddr::empty() };

                                    // update the contract to reflect our decision
                                    contract::unsubscribe_node(cfg, &did, &format!("{multi_addr}")).await;
                                    util::log_info(&format!("UNSUBSCRIBE: contract entry removed {}", did));

                                    // Remove all full-message and meta peers for an application
                                    // Then add them to our fan out peers in-case of our next subscription
                                    let mut gossip_lock = self.gossipsub.write().await;
                                    let combined_peers = {
                                        let mut combined_peers = Vec::new();

                                        combined_peers.extend(
                                            gossip_lock
                                                .other_topics
                                                .get(&did)
                                                .unwrap_or(&Default::default())
                                                .iter()
                                                .cloned(),
                                        );

                                        combined_peers.extend(
                                            gossip_lock
                                                .metadata_peers
                                                .get(&did)
                                                .unwrap_or(&Default::default())
                                                .iter()
                                                .chain(gossip_lock.mesh_peers.get(&did).unwrap_or(&Default::default()).iter())
                                                .cloned(),
                                        );

                                        combined_peers
                                    };

                                    if !combined_peers.is_empty() {
                                        gossip_lock.other_topics.insert(did.clone(), combined_peers);
                                    }

                                    drop(gossip_lock);

                                    // send an unsubscribe message to our mesh peers, so they can PRUNE us from their peer addrees book
                                    let message = format!("{}$${}$${}$${}", GossipMsgType::Unsubscribe as u8, did.clone(), multi_addr, self.swarm.local_peer_id().clone());
                                    let _ = self.swarm
                                        .behaviour_mut().gossipsub
                                        .publish(IdentTopic::new(did.clone()), message.as_bytes());
                                    util::log_info(&format!("Unsubscribe message published for application: {did}"));

                                    // get the topic and unsubscribe from it
                                    let topic = IdentTopic::new(did.clone());
                                    if let Ok(_) = self.swarm.behaviour_mut().gossipsub.unsubscribe(&topic) {
                                        // remove all the application peer entries
                                        let mut gossip_lock = self.gossipsub.write().await;
                                        gossip_lock.mesh_peers.remove(&did);
                                        gossip_lock.metadata_peers.remove(&did);
                                    }

                                    let _ = sender.send(Ok(format!("Application `{did}` has been removed from database")));
                                },
                                // Perform all the necessary bookkeeping before shutting down the process
                                ChannelMsg::Shutdown { cfg, sender } => {
                                    // get ip
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        let local_pid = self.swarm.local_peer_id().clone();
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid))
                                    } else { Multiaddr::empty() };

                                    for (did, _) in self.ipfs.read().await.cids.clone() {
                                        let did = did.to_owned();
                                        let cfg = cfg.to_owned();

                                        // update the contract to reflect our decision
                                        contract::unsubscribe_node(cfg, &did, &format!("{multi_addr}")).await;
                                        util::log_info(&format!("UNSUBSCRIBE: contract entry removed {}", did));
                                        // send an unsubscribe message to our mesh peers, so they can PRUNE us from their peer addrees book
                                        let message = format!("{}$${}$${}$${}", GossipMsgType::Unsubscribe as u8, did.clone(), multi_addr, self.swarm.local_peer_id().clone());
                                        let _ = self.swarm
                                            .behaviour_mut().gossipsub
                                            .publish(IdentTopic::new(did.clone()), message.as_bytes());

                                        util::log_info(&format!("Unsubscribe message published for application: {did}"));
                                    }

                                    // remove the database from possible bootnodes
                                    util::log_info("Shutting down database...");
                                    contract::remove_boot_node(cfg, &format!("{multi_addr}")).await;

                                    util::log_info("Database is offline");
                                    let _ = sender.send(Ok(format!("Shutdown complete")));
                                },
                                // Display most important database info
                                ChannelMsg::DBInfo => {
                                    util::log_info("Database info requested...");

                                    println!();
                                    println!("~ SamaritanDB v1.0 (Chevalier)");
                                    println!("~ Peer ID: {}", self.swarm.local_peer_id());
                                    println!("~ Listening Ports: ");
                                    let _ = self.swarm.listeners().map(|addr| {
                                        println!("~ \t{}", addr);
                                    }).collect::<Vec<_>>();

                                    println!("~ Connected Peers: {}", self.swarm.network_info().num_peers());
                                    println!("~ Applications Running: ({} apps)", self.ipfs.read().await.cids.len());
                                    let _ = self.ipfs.read().await.cids.iter().map(|(did, _)| {
                                        println!("~ \t{}", did);
                                    }).collect::<Vec<_>>();

                                    let gossipsub = self.gossipsub.read().await;
                                    print_peers(&gossipsub.mesh_peers, "Full-Message").await;
                                    print_peers(&gossipsub.metadata_peers, "Metadata").await;

                                    println!("~ Pinning Servers: ");
                                    let _ = self.ipfs.read().await.pinning_servers.iter().map(|(did, cid)| {
                                        println!("~ \t{}: {}", did, cid.0);
                                    }).collect::<Vec<_>>();

                                    println!();
                                    println!("[[ a work of art ]] (c) Copyright 2023. Algorealm, Inc.");
                                    println!();
                                },
                                // We have changed the access settings for an application, so we need to notify our peers
                                ChannelMsg::GossipAccessChange { perm, did, owner_did, access_modification_time, cfg } => {
                                    // first, notify contract to add applicated to our restricted list
                                    contract::modify_app_access(cfg.clone(), &did, &owner_did, perm).await;

                                    // get multiaddress
                                    let local_pid = self.swarm.local_peer_id().clone();
                                    let multi_addr = if let Some(ip) = local_ip::get() {
                                        construct_multiaddress(&ip.to_string(), cfg.get_tcp_port(), cfg.get_tcp_port(), Some(local_pid.clone()))
                                    } else { Multiaddr::empty() };

                                    // arrange the message into a custom format
                                    let message = format!("{}$${}$${}$${}$${}$${}$${}$${}$${}", GossipMsgType::Data as u8, did, perm, "", owner_did, access_modification_time, multi_addr, self.swarm.local_peer_id().clone(), WriteDataType::Access as u8);

                                    // publish message
                                    publish_msg(&mut self.swarm, message, did, self.gossipsub.clone(), "Message published to metadata peer").await;
                                },
                                // Change pinning server for an application
                                ChannelMsg::ChangePinningServer { did, url, sender } => {
                                    // make sure the application is running on the database
                                    if self.ipfs.read().await.cids.get(&did).is_some() {
                                        util::log_info("Attempting to change pinning server");
                                        println!("~ Configuring...");

                                        let now = util::current_unix_epoch();
                                        let mut ipfs_lock = self.ipfs.write().await;
                                        if let Some(cfg) = ipfs_lock.pinning_servers.get_mut(&did) {
                                            // change the URL
                                            cfg.0 = url.clone();
                                            // update timestamp incase of network conflict
                                            cfg.1 = now;
                                        } else {
                                            let url_cfg = (url.clone(), now);
                                            ipfs_lock.pinning_servers.insert(did.clone(), url_cfg);
                                        }

                                        util::log_info(&format!("Pinning server changed: Application: ({}), server URL: ({})", did.clone(), url.clone()));
                                        // gossip it to all peers
                                        drop(ipfs_lock);
                                        // arrange the message into a custom format
                                        let message = format!("{}$${}$${}$${}$${}$${}$${}$${}$${}", GossipMsgType::Config as u8, did, url, "", "", now, "", self.swarm.local_peer_id().clone(), NetConfigType::PinningServer as u8);

                                        // publish message
                                        publish_msg(&mut self.swarm, message, did, self.gossipsub.clone(), "Config message published to metadata peers").await;
                                        let _ = sender.send(Ok(format!("Configuration change complete")));
                                    } else {
                                        util::log_error(&format!(
                                            "attempting to manage config of uninitialized application: {did}"
                                        ));
                                        println!("~ Initialize application: `{did}` to change config");
                                    }
                                }
                            }
                        }
                        None => break, // Exit the loop when msg_receiver stream ends
                    }
                }
            }
        }
    }
}

/// Manage data synching with IPFS and keep the canonical data image of applications up to date
pub async fn manage_data_synchronization(
    did: String,
    cfg: Arc<DBConfig>,
    db_state: Arc<RwLock<DBState>>,
    ipfs_manager: Arc<RwLock<IPFSManager>>,
    gossipsub: Arc<RwLock<PeersManager>>,
    sender: Sender<ChannelMsg>,
) {
    // start the synchronization, one task per each application
    loop {
        let cfg = cfg.clone();
        let state = db_state.clone();
        let ipfs = ipfs_manager.clone();
        let gossip = gossipsub.clone();
        let mut sender = sender.clone();

        // make sure the node is still subscribed to the application
        if ipfs.read().await.cids.get(&did).is_some() {
            // first check if this node is the first to run the application
            let subscribed_peers =
                util::parse_multiaddresses(&contract::get_subscribers(cfg.clone(), &did).await);

            // get full-message peers
            let binding = gossip.read().await;
            let fm_peers = binding
                .mesh_peers
                .get(&did)
                .unwrap_or(&Default::default())
                .to_owned();

            // get sync peers
            let sync_peers = ipfs
                .read()
                .await
                .sync_peers
                .get(&did)
                .unwrap_or(&Default::default())
                .to_owned();

            // we're the only one, so the responsibility of update falls on us
            if subscribed_peers.len() == 1 && sync_peers.len() == 0 {
                // We have the ball(⚽️) and would hold it for x seconds before passing it, if other nodes appear
                // set ourselves up as the default handler
                ipfs.write()
                    .await
                    .is_node_sync_turn
                    .insert(did.clone(), true);

                // Per every application data, upload to IPFS and get CID
                util::log_info("~ performing IPFS and contract update");
                update_data_image(cfg, state, ipfs, did.clone()).await;
            } else {
                // we are either the one holding the ball or we are recieving acknowledgments about the IPFS update happening
                // and waiting for our turn to do the synching
                let mut ipfs_lock = ipfs.write().await;

                if let Some(node_sync_turn) = ipfs_lock.is_node_sync_turn.get(&did) {
                    if *node_sync_turn {
                        // perform update
                        drop(ipfs_lock); // Release the lock temporarily to avoid nested locks

                        util::log_info("performing IPFS and contract update");
                        update_data_image(cfg, state, ipfs.clone(), did.clone()).await;

                        // Acquire the lock again after performing the update
                        ipfs_lock = ipfs.write().await;
                        // inform peers that we're still alive and updating the data
                        if !fm_peers.is_empty() {
                            let _ = sender
                                .send(ChannelMsg::GossipSyncMsg {
                                    update_count: *ipfs_lock
                                        .data_update_count
                                        .get(&did)
                                        .unwrap_or(&0),
                                    did: did.to_owned(),
                                    peers: fm_peers,
                                })
                                .await;
                        }
                    } else {
                        // Release the lock temporarily to avoid nested locks
                        drop(ipfs_lock);
                        monitor_synching(did.to_owned(), ipfs.clone()).await;
                    }
                } else {
                    // insert new entry
                    ipfs_lock.is_node_sync_turn.insert(did.to_owned(), false);
                    // Release the lock temporarily to avoid nested locks
                    drop(ipfs_lock);
                    monitor_synching(did.to_owned(), ipfs.clone()).await;
                }
            }

            // wait for some time
            async_std::task::sleep(SYNC_FREQUENCY).await;
        } else {
            // the node has probably unsubscribed from managing the application
            break;
        }
    }
}

/// Monitor synching operations and take control if necessary
async fn monitor_synching(did: String, ipfs: Arc<RwLock<IPFSManager>>) {
    // observe and participate
    util::log_info("monitoring network's IPFS and contract data update");
    // it is not our turn, make sure the SYNC RPC comes within the duration we expect
    let ipfs_lock = ipfs.read().await;
    let node_peer_id = &ipfs_lock.peer_id;
    let last_sync_time = ipfs_lock
        .last_sync_time
        .get(&did)
        .unwrap_or(&Instant::now())
        .to_owned();

    // we'll add a buffer duration (10 seconds) in case of network delay
    if (Instant::now() - last_sync_time) > SYNC_FREQUENCY + ADDITIONAL_WAIT_PERIOD {
        // the original syncronizer has passed the ⚽️
        // We'll check if we're the one recieveing the ball based on the last SYNC message sent
        let sync_peers = ipfs_lock
            .sync_peers
            .get(&did)
            .unwrap_or(&Default::default())
            .to_owned();

        if !sync_peers.is_empty() {
            for (mut i, peer) in sync_peers.iter().enumerate() {
                i = i + 1; // we don't want to start at zero
                           // The order determines when we'll be taking over
                let take_over_time = Duration::from_secs(i as u64 * 10);
                // Find where we are on the list, that would determine when its our turn to takeover (if it even arrives).
                // This makes provision for failure resistance of nodes on the network
                if peer.peer_id == node_peer_id.to_owned() {
                    // dispatch a future to run after the take_over_time, if we still haven't recieved any news
                    let ipfs = ipfs.clone();
                    let did = did.clone();
                    async_std::task::spawn(async move {
                        // delay for the take-over time
                        async_std::task::sleep(take_over_time).await;
                        // now check if its still the same and nobody has taken responsibility
                        let mut ipfs_lock = ipfs.write().await;
                        let last_sync_time = ipfs_lock
                            .last_sync_time
                            .get(&did)
                            .unwrap_or(&Instant::now())
                            .to_owned();

                        if Instant::now() - last_sync_time
                            > SYNC_FREQUENCY + ADDITIONAL_WAIT_PERIOD + take_over_time
                        {
                            // still the same, so take control
                            util::log_info("acquired responsibility for network's data update");
                            ipfs_lock.is_node_sync_turn.insert(did, true);
                        }
                    });
                }
            }
        }
    }
}

/// Update network data image by updating IPFS and the contract
pub async fn update_data_image(
    cfg: Arc<DBConfig>,
    state: Arc<RwLock<DBState>>,
    ipfs: Arc<RwLock<IPFSManager>>,
    did: String,
) {
    util::log_info("updating IPFS and contract data");
    // ascertain that our local data state has been modified
    let modified = ipfs
        .clone()
        .read()
        .await
        .modified
        .get(&did)
        .unwrap_or(&false)
        .to_owned();

    if modified {
        let app_data = state
            .clone()
            .read()
            .await
            .get_application_data(&did)
            .unwrap_or(&Default::default())
            .to_owned();

        if app_data.len() > 0 {
            for (owner_did, ht_data) in app_data.iter() {
                let mut new_app_data = app_data.clone();
                let mut ht_data = ht_data.clone();

                // check if the data exists
                if !ht_data.is_empty() {
                    // upload data to IPFS to get its CID
                    let data_cid = util::write_to_ipfs(ht_data.get_data());
                    // compare with the cid in the database (perhaps no data changes has occured)
                    if let Ok(cid) = data_cid {
                        if cid != ht_data.cid {
                            // update it
                            ht_data.cid = cid.clone();
                            // save to database state
                            new_app_data.insert(owner_did.clone(), ht_data.clone());
                            state
                                .write()
                                .await
                                .add_application(did.clone(), new_app_data);

                            // pin on local node
                            cli::pin_ipfs_cid(&cid);

                            // send to pinning server
                            if let Some(cfg) = ipfs.read().await.pinning_servers.get(&did) {
                                let url = cfg.0.to_owned();
                                if let Ok(_) = util::send_post_request(&url, "cid", &cid) {
                                    util::log_info(&format!(
                                        "CID successfully POSTed to `{}`",
                                        url
                                    ));
                                } else {
                                    util::log_info(&format!("Failed to POST CID to `{}`", url));
                                }
                            }
                        }
                    }
                }
            }

            // after all the IPFS upload and hashtable update, lets try to update the hashtable copy too
            let db_state = state.read().await;
            let app_data = db_state
                .get_application_data(&did)
                .unwrap_or(&Default::default())
                .to_owned();

            if app_data.len() > 0 {
                // remove unecessary data
                let simple_ht: HashMap<String, SimpleHashTableData> = app_data
                    .iter()
                    .map(|(did, hash_data)| (did.to_owned(), hash_data.to_owned().into()))
                    .collect();

                if let Ok(app_ht_cid) = util::write_to_ipfs(&simple_ht) {
                    // compare with the last update
                    let cid = ipfs
                        .read()
                        .await
                        .cids
                        .get(&did)
                        .unwrap_or(&Default::default())
                        .to_owned();

                    if app_ht_cid != *cid {
                        // update it
                        ipfs.write()
                            .await
                            .cids
                            .insert(did.clone(), app_ht_cid.clone());

                        // update contract, for new nodes to get the latest data image
                        contract::update_ht_cid(cfg, &did, &app_ht_cid).await;
                        util::log_info(&format!(
                            "network data update successful - cid: {app_ht_cid}, did: {did}"
                        ));

                        // pin on local node
                        cli::pin_ipfs_cid(&app_ht_cid);

                        // send to pinning server
                        if let Some(cfg) = ipfs.read().await.pinning_servers.get(&did) {
                            let url = cfg.0.to_owned();
                            if let Ok(_) = util::send_post_request(&url, "cid", &app_ht_cid) {
                                util::log_info(&format!("CID successfully POSTed to `{}`", url));
                            } else {
                                util::log_info(&format!("Failed to POST CID to `{}`", url));
                            }
                        }
                    }
                }
            }
        }

        // then after update make sure to set modified to false
        ipfs.write().await.modified.insert(did.clone(), false);
    }
}

/// set up the database libp2p peer
pub async fn setup_peer(cfg: Arc<DBConfig>) -> (NetworkClient, mpsc::Sender<ChannelMsg>) {
    // first check if the contract address is set, very important
    let contract_addr = cfg.get_contract_addr();
    util::is_contract_address(contract_addr)
        .expect_variant("Invalid contract address. Could not start database node");

    let port = cfg.get_tcp_port();
    // derive the public key or synthesize a random one
    let keypair: Keypair = if !cfg.get_proto_keypair().is_empty() {
        match Keypair::from_protobuf_encoding(&cfg.get_proto_keypair()) {
            Ok(key_pair) => key_pair,
            Err(_) => {
                let kp = Keypair::generate_ed25519();
                update_config_file(&kp, port, cfg.get_chain_keys(), contract_addr);
                kp
            }
        }
    } else {
        let kp = Keypair::generate_ed25519();
        update_config_file(&kp, port, cfg.get_chain_keys(), contract_addr);
        kp
    };

    // construct the multiaddress
    let multiaddr = construct_multiaddress("0.0.0.0", port, port, None);
    // get the peerID
    let peer_id = PeerId::from_public_key(&keypair.public());
    // this is definitely the local
    let peer_id_2 = peer_id.clone();
    // log progress
    util::log_info(&format!("Database Peer ID: {}", peer_id));

    // Set up an encrypted DNS-enabled TCP Transport over the yamux protocol.
    let tcp_transport = tcp::async_io::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(
            noise::Config::new(&keypair)
                .expect_variant("Error in signing libp2p-noise static keypair"),
        )
        .multiplex(yamux::Config::default())
        .timeout(std::time::Duration::from_secs(20))
        .boxed();
    util::log_info("Transport layer configured");

    // To content-address message, we can take the hash of message and use it as an ID.
    let message_id_fn = |message: &gossipsub::Message| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };

    // gossipsub config
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .max_transmit_size(262144)
        .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
        .build()
        .expect_variant("could not configure gossipsub");
    util::log_info("Gossipsub network layer configured");

    // identity config
    let identity_config = identify::Config::new(PROTOCOL_VERSION.into(), keypair.public())
        .with_interval(Duration::from_secs(5 * 60))
        .with_cache_size(5000)
        .with_push_listen_addr_updates(true);
    util::log_info("Node identity configured");

    // Kademlia Config
    let kademlia = Kademlia::new(peer_id, MemoryStore::new(peer_id));

    // construct behaviour
    let mut swarm = SwarmBuilder::with_async_std_executor(
        tcp_transport,
        DatabaseBehaviour {
            kademlia,
            request_response: request_response::cbor::Behaviour::new(
                [(StreamProtocol::new(PROTOCOL_VERSION), ProtocolSupport::Full)],
                request_response::Config::default(),
            ),
            gossipsub: gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(keypair.clone()),
                gossipsub_config,
            )
            .expect_variant("Could not setup network driver"),
            ping: ping::Behaviour::new(ping::Config::new()),
            identify: identify::Behaviour::new(identity_config),
        },
        peer_id,
    )
    .build();
    util::log_info("Network driver configured");

    // Try to check the contract for possible boot nodes
    let bootnodes = util::parse_multiaddresses(&contract::get_boot_nodes(&cfg).await);
    util::log_info(&format!(
        "{} peer(s) fetched from contract",
        bootnodes.len()
    ));

    if bootnodes.len() > 10 {
        // populate kademlia with these nodes. If they are malicious, they would be filtered out sooner or later
        for (peer_id, multiaddress) in bootnodes.iter() {
            // make sure we're not adding ourselves
            if *peer_id != swarm.local_peer_id().to_base58() {
                // construct PeerID
                if let Ok(pid) = PeerId::from_bytes(peer_id.as_bytes()) {
                    // construct multiaddress
                    if let Ok(multiaddr) = multiaddr::from_url(&multiaddress) {
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&pid, multiaddr.clone());

                        // dail
                        let _ = swarm.dial(pid);
                    }
                }
            }
        }

        // begin bootstrap
        let _ = swarm.behaviour_mut().kademlia.bootstrap();
    } else {
        // we need to ensure our multiaddr is not 0.0.0.0
        if let Some(ip) = local_ip::get() {
            let multiaddr = construct_multiaddress(&ip.to_string(), port, port, Some(peer_id_2));
            let peer_detail = format!("{}", multiaddr);
            // we're the only one online, add node to contract
            let config = cfg.clone();
            if !multiaddr.is_empty() {
                async_std::task::spawn(async move {
                    contract::add_mulitaddress(&config, &peer_detail).await
                });
            }
        }
    }

    // Because of the un-clone-able nature of swarm and its constituents, we'll open an async communication channel
    // This channel is majorly used only for bootstrapping
    let (mut cmd_sender, cmd_receiver) = mpsc::channel::<ChannelMsg>(0);
    // Real channel used application-wide
    let (msg_sender, msg_receiver) = mpsc::channel::<ChannelMsg>(0);

    // spawn a task to handle the reciever end
    let config = cfg.clone();
    async_std::task::spawn(async move {
        handle_channel_reciever(cmd_receiver, config, peer_id_2).await
    });

    let mut net_client = NetworkClient {
        swarm,
        msg_receiver,
        ping_manager: PingManager::default(),
        db_state: Default::default(),
        gossipsub: Arc::new(RwLock::new(PeersManager::new_with_config(
            GossipSubConfig {
                d: 6,
                d_low: 4,
                d_high: 12,
                d_lazy: 6,
                heartbeat_interval: Duration::from_secs(10),
                _fanout_ttl: Duration::from_secs(60),
                mcache_gossip: 3,
                _seen_ttl: Duration::from_secs(60 * 60 * 2),
            },
        ))),
        rpc: Default::default(),
        ipfs: Default::default(),
        access_manager: Default::default(),
    };

    // update the IPFS node peer id
    net_client.ipfs.write().await.peer_id = peer_id.to_base58();

    // manage bootstrapping, send request to the sender
    cmd_sender
        .send(ChannelMsg::ManageRoutingTableBootstrap {
            sender: Some(msg_sender.clone()),
            data: Default::default(),
            monitor_contract: if bootnodes.len() > 8 { false } else { true },
        })
        .await
        .expect_variant("Failed to perform bootstrap process");

    net_client
        .swarm
        .listen_on(multiaddr)
        .expect_variant("Network driver failed to start. To diagnose, try changing the port in use in your config file.");
    util::log_info("Database is online");

    // spin a task to handle the gossipsub heartbeat
    let gossip_mngr = net_client.gossipsub.clone();
    let heartbeat_interval = net_client
        .gossipsub
        .read()
        .await
        .config
        .heartbeat_interval
        .clone();
    let msg_sender_0 = msg_sender.clone();
    let config = cfg.clone();

    // task to manage our gossip message structure
    async_std::task::spawn(async move {
        loop {
            let gossip_mngr = gossip_mngr.clone();
            let msg_sender_0 = msg_sender_0.clone();
            let config = config.clone();

            handle_gossip_heartbeat(gossip_mngr, msg_sender_0, config).await;
            // Wait for some time before performing the next bootstrap
            async_std::task::sleep(heartbeat_interval).await;
        }
    });
    (net_client, msg_sender)
}

/// handle all the incoming commands from the channel
async fn handle_channel_reciever(
    mut recv: mpsc::Receiver<ChannelMsg>,
    cfg: Arc<DBConfig>,
    peer_id: PeerId,
) {
    match recv.next().await {
        // this happens only once at application startup
        Some(ChannelMsg::ManageRoutingTableBootstrap {
            sender,
            data: _,
            monitor_contract,
        }) => {
            if monitor_contract {
                // we're the first on the network, so monitor contract for others
                loop {
                    // stop disturbing contract, we have enough peers
                    if !UP_TO_8_PEERS.load(Ordering::SeqCst) {
                        // clone the sender
                        let sender = sender.clone();
                        let mut bootnodes =
                            util::parse_multiaddresses(&contract::get_boot_nodes(&cfg).await);
                        util::log_info(&format!(
                            "{} peer(s) fetched from contract",
                            bootnodes.len()
                        ));

                        // make sure we're not considering our peerID
                        bootnodes.remove(&peer_id.to_base58());

                        if bootnodes.len() > 0 {
                            let mut peers: HashMap<PeerId, Multiaddr> = HashMap::new();
                            // populate kademlia with these nodes. If they are malicious, they would be filtered out sooner or later
                            for (peer_id, multiaddress) in bootnodes.iter() {
                                // construct PeerID
                                if let Ok(pid) =
                                    PeerId::from_bytes(&peer_id.from_base58().unwrap_or_default())
                                {
                                    // construct multiaddress
                                    let multi_address: Result<Multiaddr, _> = multiaddress.parse();
                                    if let Ok(m_addr) = multi_address {
                                        peers.insert(pid, m_addr);
                                    }
                                }
                            }

                            // send it over the channel to add to our DHT
                            if let Some(mut sender) = sender {
                                let _ = sender
                                    .send(ChannelMsg::ManageRoutingTableBootstrap {
                                        sender: None,
                                        data: peers,
                                        monitor_contract: false,
                                    })
                                    .await;
                            }
                        }

                        // Wait for some time before performing the next bootstrap
                        async_std::task::sleep(Duration::from_secs(BOOTSTRAP_INTERVAL)).await;
                    }
                }
            }
            // else just ignore
        }
        _ => {}
    }

    // try to add nodes to the routing table, either by reading the contract for nodes or by bootstrap()
}

/// update local config file for next time
fn update_config_file(keypair: &Keypair, port: u16, contract_key: &str, contract_addr: &str) {
    let keypair = keypair.clone();
    let contract_key = contract_key.to_owned();
    let contract_addr = contract_addr.to_owned();
    async_std::task::spawn(async move {
        // serialize the key we're using
        let bytes = keypair.to_protobuf_encoding().unwrap_or_default();
        let json_str = serde_json::to_string(&bytes).unwrap_or_default();
        let contract_key = if contract_key.contains("//") || contract_key.is_empty() {
            "//Alice"
        } else {
            &contract_key
        };

        let data = format!(
            r#"
        {{
            "protobuf_keypair": {},
            "tcp_port": {},
            "chain_keys": "{}",
            "contract_address": "{}",
            "server_address_port": 2027
        }}"#,
            json_str, port, contract_key, contract_addr
        );

        let _ = util::write_string_to_file(&data, CONFIG_FILE);
    });
}

fn construct_multiaddress(
    ip: &str,
    _udp_port: u16,
    tcp_port: u16,
    peer_id: Option<PeerId>,
) -> Multiaddr {
    let mut multiaddr = Multiaddr::empty();
    // multiaddr.push(Protocol::Ip4(ip.parse().unwrap()));
    // multiaddr.push(Protocol::Udp(udp_port));
    // multiaddr.push(Protocol::Quic);
    multiaddr.push(Protocol::Ip4(
        ip.parse().expect_variant("could not parse IP address"),
    ));
    multiaddr.push(Protocol::Tcp(tcp_port));
    if let Some(peer_id) = peer_id {
        multiaddr.push(Protocol::P2p(peer_id));
    }

    multiaddr
}

/// Run the heartbeat algorithm on the gossip cache
async fn handle_gossip_heartbeat(
    peers: Arc<RwLock<PeersManager>>,
    mut sender: mpsc::Sender<ChannelMsg>,
    cfg: Arc<DBConfig>,
) {
    // First, mesh maintainance
    // util::log_info("performing routine mesh maintainance");
    // Handle full-message peers
    let mesh_peers = peers.read().await.mesh_peers.clone();
    let mut prunable_peers: HashMap<String, Vec<GossipPeer>> = HashMap::new();
    let mut graftable_peers = prunable_peers.clone();
    for (topic, topic_peers) in mesh_peers {
        let peers_lock = peers.read().await;
        let d = peers_lock.config.d.clone();
        let d_low = peers_lock.config.d_low.clone();
        let d_high = peers_lock.config.d_high.clone();
        if topic_peers.len() < d {
            let mut diff = d_low - topic_peers.len();
            // now loop and try to GRAFT peers from gossip status to full-message
            if let Some(metapeers) = peers_lock.metadata_peers.get(&topic) {
                if metapeers.len() > 0 {
                    let mut topic_peers = topic_peers.to_owned();
                    let mut temp_peers = Vec::new();
                    for peer in metapeers {
                        temp_peers.push(peer.to_owned());
                        topic_peers.push(peer.to_owned());

                        // reduce difference
                        diff -= 1;
                        if diff == 0 {
                            break;
                        }
                    }

                    // add to graftable peers
                    graftable_peers.insert(topic.clone(), temp_peers);

                    // update peers
                    peers
                        .write()
                        .await
                        .mesh_peers
                        .insert(topic.to_string(), topic_peers.clone());
                }
            }
        } else if topic_peers.len() > d_high {
            let mut diff = d_high - topic_peers.len();
            // now loop and try to PRUNE peers from full-message to gossip status
            let mut topic_peers = topic_peers.to_owned();
            let mut temp_peers = Vec::new();
            while diff != 0 {
                if let Some(peer) = topic_peers.pop() {
                    temp_peers.push(peer);
                }
                diff -= 1;
            }

            // add to prunable peers
            prunable_peers.insert(topic.clone(), temp_peers);

            // update peers
            peers
                .write()
                .await
                .mesh_peers
                .insert(topic.to_string(), topic_peers.clone());
        }
    }

    if graftable_peers.len() > 0 {
        // send GRAFT message
        let _ = sender.send(ChannelMsg::GossipGraftMsg {
            cfg,
            peers: graftable_peers,
        });
    }

    if prunable_peers.len() > 0 {
        // send PRUNE message
        let _ = sender.send(ChannelMsg::GossipPruneMsg {
            peers: prunable_peers,
        });
    }
}

async fn print_peers(peers: &HashMap<String, Vec<GossipPeer>>, label: &str) {
    let mut seen_peer_ids = HashSet::new();

    for (_, peer_list) in peers {
        for peer in peer_list {
            seen_peer_ids.insert(&peer.peer_id);
        }
    }

    println!("~ {} Peers: ({} peers)", label, seen_peer_ids.len());

    for peer_id in seen_peer_ids {
        println!("~ \t{}", peer_id);
    }
}

/// Gossip message to full-message and metadata peers
async fn gossip_msg(
    swarm: &mut Swarm<DatabaseBehaviour>,
    message: Message,
    did: &str,
    gossipsub: Arc<RwLock<PeersManager>>,
    rpc: &mut RpcManager,
    message_id: MessageId,
) {
    // Now, we gossip the message to all our full-message peers
    let mut retry_count = 0;
    loop {
        if let Ok(_) = swarm
            .behaviour_mut()
            .gossipsub
            .publish(IdentTopic::new(did), message.data.clone())
        {
            util::log_info(&format!("Application data gossiped to peers: `{did}`"));
            break;
        } else {
            util::log_error(&format!("Failed to gossip data to peers: `{did}`"));
            retry_count += 1;
            if retry_count == GOSSIP_RETRY_COUNT {
                break;
            }
        }
    }

    // Acquire the lock on self.gossipsub only once
    let gossipsub_lock = gossipsub.read().await;

    // Get metadata_peers from gossipsub_lock
    if let Some(peers) = gossipsub_lock.metadata_peers.get(did) {
        // Gossip data through RPCs (IHAVE)
        // We're going to gossip all the messages in our cache for a particular application
        let mut message_ids = rpc
            .msg_cache
            .get_gossip_ids(did, gossipsub_lock.config.mcache_gossip);
        message_ids.insert(0, message_id.0.clone());

        let mut control_msgs = Vec::new();
        control_msgs.push(ControlAction::IHave {
            topic: did.to_owned(),
            message_ids,
        });

        let message = Rpc {
            // We're sending the peerID as the message content
            messages: vec![swarm.local_peer_id().clone().to_base58()],
            control_msgs,
        };

        // Send the RPC
        for peer in peers.iter() {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(&peer.peer_id, message.clone());
        }
    }
}

/// Publish message to peers (full and metadata)
async fn publish_msg(
    swarm: &mut Swarm<DatabaseBehaviour>,
    message: String,
    did: String,
    gossipsub: Arc<RwLock<PeersManager>>,
    log_msg: &str,
) {
    // If failure, we'll try again as least {} times
    let mut retry_count = 0;

    loop {
        if let Ok(_) = swarm
            .behaviour_mut()
            .gossipsub
            .publish(IdentTopic::new(did.clone()), message.as_bytes())
        {
            util::log_info(&format!("Message published to peers: `{did}`"));
            break;
        } else {
            util::log_error(&format!("Failed to publish message to peers: `{did}`"));
            retry_count += 1;
            if retry_count == GOSSIP_RETRY_COUNT {
                break;
            }
        }
    }

    // we'll be flooding if message is originating from us, so send to meta peeers too
    let mut control_msgs = Vec::new();
    control_msgs.push(ControlAction::IGive {
        messages: vec![message.as_bytes().to_vec()],
    });

    let msg = Rpc {
        // we're sending the peerID as the message content
        messages: vec![swarm.local_peer_id().clone().to_base58()],
        control_msgs,
    };

    // let's get all our metadata peers
    if let Some(meta_peers) = gossipsub.read().await.metadata_peers.get(&did) {
        for peer in meta_peers {
            let msg = msg.clone();
            // Send the RPC
            let _ = swarm
                .behaviour_mut()
                .request_response
                .send_request(&peer.peer_id, msg);
            util::log_info(&format!("{}: {}", log_msg, &peer.peer_id));
        }
    }
}

/// Manage data access changes by monitoring contract and enforcing network actors decisions locally
async fn manage_data_access_changes(
    did: String,
    cfg: Arc<DBConfig>,
    access_manager: Arc<RwLock<DataAccessManager>>,
    db_state: Arc<RwLock<DBState>>,
    sender_channel: mpsc::Sender<ChannelMsg>,
) {
    loop {
        println!("Checking status for {did}");

        let cfg = cfg.clone();
        let state = db_state.clone();
        // Get the list of users blocking an applications access
        let user_dids =
            util::extract_dids(&contract::get_application_access_blockers(cfg.clone(), &did).await);

        println!("{:#?}", user_dids);

        // Get the list of DIDs whose data the app is restricted from accessing
        let actor_dids = access_manager
            .read()
            .await
            .restrictions
            .get(&did)
            .unwrap_or(&Default::default())
            .to_owned();

        let user_dids = user_dids.into_iter().filter(|item| item != "\0").collect();
        let actor_dids = actor_dids.into_iter().filter(|item| item != "\0").collect();

        // Get the DIDs that have decided to allow the appications access their data
        let liberal_dids = util::disjoint_strings(&user_dids, &actor_dids);
        if liberal_dids.len() > 0 {
            util::log_info(&format!(
                "{} access-allowing users found for application: `{}`",
                liberal_dids.len(),
                did
            ));

            // disable restrictions
            for sam_did in liberal_dids.iter() {
                manage_db_access(
                    cfg.clone(),
                    sender_channel.clone(),
                    state.clone(),
                    did.clone(),
                    sam_did.clone(),
                    true,
                )
                .await;

                // remove it from the DID list we have saved
                access_manager.write().await.restrictions.remove(sam_did);
            }
        }

        // Get the list of DIDs whose data the app is restricted from accessing
        let actor_dids = access_manager
            .read()
            .await
            .restrictions
            .get(&did)
            .unwrap_or(&Default::default())
            .to_owned();

        let mut actor_dids = actor_dids.into_iter().filter(|item| item != "\0").collect();

        // Get the DIDs that have just been added to the restricted list
        let blocker_dids = util::disjoint_strings(&actor_dids, &user_dids);
        if blocker_dids.len() > 0 {
            util::log_info(&format!(
                "{} access-blocking users found for application: `{}`",
                blocker_dids.len(),
                did
            ));

            // enforce restrictions
            for sam_did in blocker_dids.iter() {
                manage_db_access(
                    cfg.clone(),
                    sender_channel.clone(),
                    state.clone(),
                    did.clone(),
                    sam_did.clone(),
                    false,
                )
                .await;
            }

            // Now add to list and enforce the access restrictions
            actor_dids.extend(blocker_dids.clone().into_iter());

            // save them
            access_manager
                .write()
                .await
                .restrictions
                .insert(did.clone(), actor_dids.clone());
        }

        // wait for some seconds
        async_std::task::sleep(Duration::from_secs(DATA_ACCESS_MONITORING_WAIT_TIME)).await;
    }
}
