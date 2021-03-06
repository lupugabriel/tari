// Copyright 2019. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use log::*;
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tari_common::{DatabaseType, GlobalConfig};
use tari_comms::{
    builder::CommsNode,
    control_service::ControlServiceConfig,
    multiaddr::Multiaddr,
    peer_manager::{node_identity::NodeIdentity, NodeId, Peer, PeerFeatures, PeerFlags},
};
use tari_core::{
    base_node::{
        service::{BaseNodeServiceConfig, BaseNodeServiceInitializer},
        BaseNodeStateMachine,
        BaseNodeStateMachineConfig,
        OutboundNodeCommsInterface,
    },
    chain_storage::{
        create_lmdb_database,
        BlockchainBackend,
        BlockchainDatabase,
        LMDBDatabase,
        MemoryDatabase,
        Validators,
    },
    consensus::ConsensusManager,
    mempool::{Mempool, MempoolConfig, MempoolValidators},
    proof_of_work::DiffAdjManager,
    transactions::{
        crypto::keys::SecretKey as SK,
        types::{CryptoFactories, HashDigest, PrivateKey, PublicKey},
    },
    validation::{
        block_validators::{FullConsensusValidator, StatelessValidator},
        chain_validators::{ChainTipValidator, GenesisBlockValidator},
        horizon_state_validators::HorizonStateHeaderValidator,
        transaction_validators::{FullTxValidator, TxInputAndMaturityValidator},
    },
};
use tari_mmr::MerkleChangeTrackerConfig;
use tari_p2p::{
    comms_connector::pubsub_connector,
    initialization::{initialize_comms, CommsConfig},
    services::comms_outbound::CommsOutboundServiceInitializer,
};
use tari_service_framework::{handles::ServiceHandles, StackBuilder};
use tari_utilities::{hex::Hex, message_format::MessageFormat};
use tokio::runtime::Runtime;

const LOG_TARGET: &str = "base_node::initialization";

pub enum NodeType {
    LMDB(BaseNodeStateMachine<LMDBDatabase<HashDigest>>),
    Memory(BaseNodeStateMachine<MemoryDatabase<HashDigest>>),
}

impl NodeType {
    pub fn get_flag(&self) -> Arc<AtomicBool> {
        match self {
            NodeType::LMDB(n) => n.get_interrupt_flag(),
            NodeType::Memory(n) => n.get_interrupt_flag(),
        }
    }

    pub async fn run(self) {
        async move {
            match self {
                NodeType::LMDB(n) => n.run().await,
                NodeType::Memory(n) => n.run().await,
            }
        }
        .await;
    }
}

/// Tries to construct a node identity by loading the secret key and other metadata from disk and calculating the
/// missing fields from that information.
pub fn load_identity(path: &Path) -> Result<NodeIdentity, String> {
    if !path.exists() {
        return Err(format!("Identity file, {}, does not exist.", path.to_str().unwrap()));
    }

    let id_str = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "The node identity file, {}, could not be read. {}",
            path.to_str().unwrap_or("?"),
            e.to_string()
        )
    })?;
    let id = NodeIdentity::from_json(&id_str).map_err(|e| {
        format!(
            "The node identity file, {}, has an error. {}",
            path.to_str().unwrap_or("?"),
            e.to_string()
        )
    })?;
    info!(
        "Node ID loaded with public key {} and Node id {}",
        id.public_key().to_hex(),
        id.node_id().to_hex()
    );
    Ok(id)
}

fn new_node_id(private_key: PrivateKey, control_addr: &str) -> Result<NodeIdentity, String> {
    let address = control_addr.parse::<Multiaddr>().map_err(|e| {
        format!(
            "Error. '{}' is not a valid control port address. {}",
            control_addr,
            e.to_string()
        )
    })?;
    let features = PeerFeatures::COMMUNICATION_NODE;
    NodeIdentity::new(private_key, address, features)
        .map_err(|e| format!("We were unable to construct a node identity. {}", e.to_string()))
}

/// Create a new node id and save it to disk
pub fn create_and_save_id(path: &Path, control_addr: &str) -> Result<NodeIdentity, String> {
    let mut rng = rand::OsRng::new().unwrap();
    let pk = PrivateKey::random(&mut rng);
    // build config file
    let id = new_node_id(pk, control_addr)?;
    let node_str = id.to_json().unwrap();
    if let Some(p) = path.parent() {
        if !p.exists() {
            std::fs::create_dir_all(p)
                .map_err(|e| format!("Could not create identity data folder. {}", e.to_string()))?;
        }
    }
    std::fs::write(path, node_str.as_bytes()).map_err(|e| {
        format!(
            "Error writing identity file, {}. {}",
            path.to_str().unwrap_or("??"),
            e.to_string()
        )
    })?;
    Ok(id)
}

pub fn configure_and_initialize_node(
    config: &GlobalConfig,
    id: NodeIdentity,
    rt: &mut Runtime,
) -> Result<(CommsNode, NodeType), String>
{
    let id = Arc::new(id);
    let factories = Arc::new(CryptoFactories::default());
    let peers = assign_peers(&config.peer_seeds);
    let result = match &config.db_type {
        DatabaseType::Memory => {
            let rules = ConsensusManager::default();
            let backend = MemoryDatabase::<HashDigest>::default();
            let mut db = BlockchainDatabase::new(backend).map_err(|e| e.to_string())?;
            let validators = Validators::new(
                FullConsensusValidator::new(rules.clone(), factories.clone(), db.clone()),
                StatelessValidator::new(),
                HorizonStateHeaderValidator::new(rules.clone(), db.clone()),
                GenesisBlockValidator::new(),
                ChainTipValidator::new(rules.clone(), factories.clone(), db.clone()),
            );
            db.set_validators(validators);
            let mempool_validator = MempoolValidators::new(
                FullTxValidator::new(factories.clone(), db.clone()),
                TxInputAndMaturityValidator::new(db.clone()),
            );
            let mempool = Mempool::new(db.clone(), MempoolConfig::default(), mempool_validator);
            let diff_adj_manager = DiffAdjManager::new(db.clone()).map_err(|e| e.to_string())?;
            rules.set_diff_manager(diff_adj_manager).map_err(|e| e.to_string())?;
            let (comms, handles) =
                setup_comms_services(rt, id.clone(), peers, &config.peer_db_path, db.clone(), mempool, rules);
            let outbound_interface = handles.get_handle::<OutboundNodeCommsInterface>().unwrap();
            (
                comms,
                NodeType::Memory(BaseNodeStateMachine::new(
                    &db,
                    &outbound_interface,
                    BaseNodeStateMachineConfig::default(),
                )),
            )
        },
        DatabaseType::LMDB(p) => {
            let rules = ConsensusManager::default();
            let mct_config = MerkleChangeTrackerConfig {
                min_history_len: 900,
                max_history_len: 1000,
            };
            let backend = create_lmdb_database(&p, mct_config).map_err(|e| e.to_string())?;
            let mut db = BlockchainDatabase::new(backend).map_err(|e| e.to_string())?;
            let validators = Validators::new(
                FullConsensusValidator::new(rules.clone(), factories.clone(), db.clone()),
                StatelessValidator::new(),
                HorizonStateHeaderValidator::new(rules.clone(), db.clone()),
                GenesisBlockValidator::new(),
                ChainTipValidator::new(rules.clone(), factories.clone(), db.clone()),
            );
            db.set_validators(validators);
            let mempool_validator = MempoolValidators::new(
                FullTxValidator::new(factories.clone(), db.clone()),
                TxInputAndMaturityValidator::new(db.clone()),
            );
            let mempool = Mempool::new(db.clone(), MempoolConfig::default(), mempool_validator);
            let diff_adj_manager = DiffAdjManager::new(db.clone()).map_err(|e| e.to_string())?;
            rules.set_diff_manager(diff_adj_manager).map_err(|e| e.to_string())?;
            let (comms, handles) =
                setup_comms_services(rt, id.clone(), peers, &config.peer_db_path, db.clone(), mempool, rules);
            let outbound_interface = handles.get_handle::<OutboundNodeCommsInterface>().unwrap();
            (
                comms,
                NodeType::LMDB(BaseNodeStateMachine::new(
                    &db,
                    &outbound_interface,
                    BaseNodeStateMachineConfig::default(),
                )),
            )
        },
    };
    Ok(result)
}

fn assign_peers(seeds: &[String]) -> Vec<Peer> {
    let mut result = Vec::with_capacity(seeds.len());
    for s in seeds {
        let parts: Vec<&str> = s.split("::").map(|s| s.trim()).collect();
        if parts.len() != 2 {
            warn!(target: LOG_TARGET, "Invalid peer seed: {}", s);
            continue;
        }
        let pub_key = match PublicKey::from_hex(parts[0]) {
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "{} is not a valid peer seed. The public key is incorrect. {}",
                    s,
                    e.to_string()
                );
                continue;
            },
            Ok(p) => p,
        };
        let addr = match parts[1].parse::<Multiaddr>() {
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "{} is not a valid peer seed. The address is incorrect. {}",
                    s,
                    e.to_string()
                );
                continue;
            },
            Ok(a) => a,
        };
        let node_id = match NodeId::from_key(&pub_key) {
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "{} is not a valid peer seed. A node id couldn't be derived from the public key. {}",
                    s,
                    e.to_string()
                );
                continue;
            },
            Ok(id) => id,
        };
        let peer = Peer::new(
            pub_key,
            node_id,
            addr.into(),
            PeerFlags::default(),
            PeerFeatures::empty(),
        );
        result.push(peer);
    }
    result
}

fn setup_comms_services<T>(
    rt: &mut Runtime,
    id: Arc<NodeIdentity>,
    peers: Vec<Peer>,
    peer_db_path: &str,
    db: BlockchainDatabase<T>,
    mempool: Mempool<T>,
    consensus_manager: ConsensusManager<T>,
) -> (CommsNode, Arc<ServiceHandles>)
where
    T: BlockchainBackend + 'static,
{
    let node_config = BaseNodeServiceConfig::default(); // TODO - make this configurable
    let (publisher, subscription_factory) = pubsub_connector(rt.handle().clone(), 100);
    let subscription_factory = Arc::new(subscription_factory);
    let comms_config = CommsConfig {
        node_identity: id.clone(),
        peer_connection_listening_address: "/ip4/0.0.0.0/tcp/0".parse().expect("cannot fail"), /* TODO - make this
                                                                                                * configurable */
        socks_proxy_address: None,
        control_service: ControlServiceConfig {
            listening_address: id.control_service_address(),
            socks_proxy_address: None,
            public_peer_address: None, // TODO - make this configurable
            requested_connection_timeout: Duration::from_millis(2000),
        },
        establish_connection_timeout: Duration::from_secs(10), // TODO - make this configurable
        datastore_path: peer_db_path.to_string(),
        peer_database_name: "peers".to_string(),
        inbound_buffer_size: 100,
        outbound_buffer_size: 100,
        dht: Default::default(), // TODO - make this configurable
    };

    let (comms, dht) = initialize_comms(rt.handle().clone(), comms_config, publisher).unwrap();

    for p in peers {
        debug!(target: LOG_TARGET, "Adding seed peer [{}]", p.node_id);
        comms.peer_manager().add_peer(p).unwrap();
    }

    let fut = StackBuilder::new(rt.handle().clone(), comms.shutdown_signal())
        .add_initializer(CommsOutboundServiceInitializer::new(dht.outbound_requester()))
        .add_initializer(BaseNodeServiceInitializer::new(
            subscription_factory,
            db,
            mempool,
            consensus_manager,
            node_config,
        ))
        .finish();

    info!(target: LOG_TARGET, "Initializing communications stack...");
    let handles = rt.block_on(fut).expect("Service initialization failed");
    info!(
        target: LOG_TARGET,
        "Node initialization complete. Listening for connections at {}.",
        id.control_service_address(),
    );
    (comms, handles)
}
