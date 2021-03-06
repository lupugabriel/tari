// Copyright 2020, The Tari Project
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

use crate::{
    backoff::ConstantBackoff,
    connection_manager::{
        dialer::{Dialer, DialerRequest},
        listener::PeerListener,
        manager::ConnectionManagerEvent,
        next::ConnectionManagerConfig,
    },
    noise::NoiseConfig,
    peer_manager::{Peer, PeerFeatures, PeerFlags},
    protocol::ProtocolId,
    test_utils::node_identity::build_node_identity,
    transports::MemoryTransport,
};
use futures::{
    channel::{mpsc, oneshot},
    AsyncReadExt,
    AsyncWriteExt,
    SinkExt,
    StreamExt,
};
use multiaddr::Protocol;
use std::{error::Error, sync::Arc, time::Duration};
use tari_shutdown::Shutdown;
use tari_test_utils::unpack_enum;
use tokio::{runtime::Runtime, time::timeout};

#[test]
fn listen() -> Result<(), Box<dyn Error>> {
    let mut rt = Runtime::new()?;
    let (event_tx, mut event_rx) = mpsc::channel(1);
    let mut shutdown = Shutdown::new();
    let noise_config = NoiseConfig::new(build_node_identity(PeerFeatures::COMMUNICATION_NODE));
    let listener = PeerListener::new(
        rt.handle().clone(),
        "/memory/0".parse()?,
        MemoryTransport,
        noise_config.clone(),
        event_tx.clone(),
        vec![],
        shutdown.to_signal(),
    );

    let listener_fut = rt.spawn(listener.run());

    rt.block_on(async move {
        let listen_event = event_rx.next().await.unwrap();
        unpack_enum!(ConnectionManagerEvent::Listening(address) = listen_event);
        unpack_enum!(Protocol::Memory(port) = address.pop().unwrap());
        assert!(port > 0);

        shutdown.trigger().unwrap();

        timeout(Duration::from_secs(5), listener_fut).await.unwrap().unwrap();

        Ok(())
    })
}

#[test]
fn smoke() {
    // This test sets up Dialer and Listener components, uses the Dialer to dial the Listener,
    // asserts the emitted events are correct, opens a substream, sends a small message over the substream,
    // receives and checks the message and then disconnects and shuts down.
    let mut rt = Runtime::new().unwrap();
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let mut shutdown = Shutdown::new();

    let node_identity1 = build_node_identity(PeerFeatures::COMMUNICATION_NODE);
    let noise_config1 = NoiseConfig::new(node_identity1.clone());
    let expected_proto = ProtocolId::from_static(b"/tari/test-proto");
    let supported_protocols = vec![expected_proto.clone()];
    let listener = PeerListener::new(
        rt.handle().clone(),
        "/memory/0".parse().unwrap(),
        MemoryTransport,
        noise_config1,
        event_tx.clone(),
        supported_protocols.clone(),
        shutdown.to_signal(),
    );

    let listener_fut = rt.spawn(listener.run());

    let node_identity2 = build_node_identity(PeerFeatures::COMMUNICATION_NODE);
    let noise_config2 = NoiseConfig::new(node_identity2.clone());
    let (mut request_tx, request_rx) = mpsc::channel(1);
    let dialer = Dialer::new(
        rt.handle().clone(),
        ConnectionManagerConfig::default(),
        MemoryTransport,
        noise_config2,
        Arc::new(ConstantBackoff::new(Duration::from_millis(100))),
        request_rx,
        event_tx,
        supported_protocols,
        shutdown.to_signal(),
    );

    let dialer_fut = rt.spawn(dialer.run());

    rt.block_on(async move {
        // Get the listening address of the peer
        let listen_event = event_rx.next().await.unwrap();
        unpack_enum!(ConnectionManagerEvent::Listening(address) = listen_event);

        let mut peer = Peer::new(
            node_identity1.public_key().clone(),
            node_identity1.node_id().clone(),
            vec![address].into(),
            PeerFlags::empty(),
            PeerFeatures::COMMUNICATION_NODE,
        );
        peer.set_id_for_test(1);

        let (reply_tx, reply_rx) = oneshot::channel();
        request_tx
            .send(DialerRequest::Dial(Box::new(peer), reply_tx))
            .await
            .unwrap();

        let mut outbound_peer_conn = reply_rx.await.unwrap().unwrap();

        // Open a substream
        {
            let mut out_stream = outbound_peer_conn.open_substream("/tari/test-proto").await.unwrap();
            out_stream.stream.write_all(b"HELLO").await.unwrap();
            out_stream.stream.flush().await.unwrap();
        }

        // Read PeerConnected events - we don't know which connection is which
        unpack_enum!(ConnectionManagerEvent::PeerConnected(conn1) = event_rx.next().await.unwrap());
        unpack_enum!(ConnectionManagerEvent::PeerConnected(conn2) = event_rx.next().await.unwrap());

        // Next event should be a NewInboundSubstream has been received
        let listen_event = event_rx.next().await.unwrap();
        {
            unpack_enum!(ConnectionManagerEvent::NewInboundSubstream(public_key, proto, in_stream) = listen_event);
            assert_eq!(&*public_key, node_identity2.public_key());
            assert_eq!(proto, expected_proto);

            let mut buf = [0u8; 5];
            in_stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, *b"HELLO");
        }

        let _ = conn1.disconnect().await;
        let _ = conn2.disconnect().await;

        shutdown.trigger().unwrap();

        timeout(Duration::from_secs(5), listener_fut).await.unwrap().unwrap();
        timeout(Duration::from_secs(5), dialer_fut).await.unwrap().unwrap();
    });
}
