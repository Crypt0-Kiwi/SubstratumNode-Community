// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use multinode_integration_tests_lib::neighborhood_constructor::construct_neighborhood;
use multinode_integration_tests_lib::substratum_mock_node::SubstratumMockNode;
use multinode_integration_tests_lib::substratum_node::{
    PortSelector, SubstratumNode, SubstratumNodeUtils,
};
use multinode_integration_tests_lib::substratum_node_cluster::SubstratumNodeCluster;
use multinode_integration_tests_lib::substratum_node_server::SubstratumNodeServer;
use multinode_integration_tests_lib::substratum_real_node::SubstratumRealNode;
use node_lib::blockchain::blockchain_interface::{contract_address, DEFAULT_CHAIN_ID};
use node_lib::hopper::live_cores_package::LiveCoresPackage;
use node_lib::json_masquerader::JsonMasquerader;
use node_lib::masquerader::Masquerader;
use node_lib::neighborhood::neighborhood_database::NeighborhoodDatabase;
use node_lib::neighborhood::neighborhood_test_utils::{db_from_node, make_node_record};
use node_lib::neighborhood::node_record::NodeRecord;
use node_lib::sub_lib::cryptde::{decodex, CryptDE, PublicKey};
use node_lib::sub_lib::cryptde_null::CryptDENull;
use node_lib::sub_lib::data_version::DataVersion;
use node_lib::sub_lib::dispatcher::Component;
use node_lib::sub_lib::hopper::{IncipientCoresPackage, MessageType};
use node_lib::sub_lib::proxy_client::ClientResponsePayload;
use node_lib::sub_lib::proxy_server::{ClientRequestPayload, ProxyProtocol};
use node_lib::sub_lib::route::{Route, RouteSegment};
use node_lib::sub_lib::sequence_buffer::SequencedPacket;
use node_lib::sub_lib::stream_key::StreamKey;
use node_lib::test_utils::{find_free_port, make_meaningless_stream_key};
use std::io;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

const HTTP_REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: booga.com\r\n\r\n";
const HTTP_RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nbooga";

#[test]
// Given: Originating Node is real_node; exit Node is fictional Node with exit_key.
// Given: A stream is established from the client through the originating Node.
// When: Client (browser?) drops connection to originating Node.
// Then: Originating Node sends ClientRequestPayload to exit Node with empty SequencedPacket having last_data = true.
fn actual_client_drop() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let (real_node, mock_node, exit_key) = create_neighborhood(&mut cluster);
    let exit_cryptde = CryptDENull::from(&exit_key, cluster.chain_id);
    let mut client = real_node.make_client(8080);
    let masquerader = JsonMasquerader::new();
    client.send_chunk(HTTP_REQUEST);
    mock_node
        .wait_for_package(&masquerader, Duration::from_secs(2))
        .unwrap();

    client.shutdown();

    let (_, _, lcp) = mock_node
        .wait_for_package(&masquerader, Duration::from_secs(2))
        .unwrap();
    let payload = match decodex::<MessageType>(&exit_cryptde, &lcp.payload).unwrap() {
        MessageType::ClientRequest(p) => p,
        mt => panic!("Unexpected: {:?}", mt),
    };
    assert!(payload.sequenced_packet.data.is_empty());
    assert!(payload.sequenced_packet.last_data);
}

#[test]
// Given: Originating Node is real_node; exit Node is fictional Node with exit_key.
// Given: A stream is established from the client through the originating Node.
// When: Originating Node receives empty SequencedPacket having last_data = true.
// Then: Originating Node drops connection to client (browser?).
// Then: Originating Node does _not_ send CORES package back to exit Node.
fn reported_server_drop() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let (real_node, mock_node, exit_key) = create_neighborhood(&mut cluster);
    let exit_cryptde = CryptDENull::from(&exit_key, cluster.chain_id);
    let mut client = real_node.make_client(8080);
    let masquerader = JsonMasquerader::new();
    client.send_chunk(HTTP_REQUEST);
    let (_, _, lcp) = mock_node
        .wait_for_package(&masquerader, Duration::from_secs(2))
        .unwrap();
    let (stream_key, return_route_id) =
        context_from_request_lcp(lcp, real_node.cryptde_null().unwrap(), &exit_cryptde);

    mock_node
        .transmit_package(
            mock_node.port_list()[0],
            create_server_drop_report(&mock_node, &real_node, stream_key, return_route_id),
            &masquerader,
            real_node.public_key(),
            real_node.socket_addr(PortSelector::First),
        )
        .unwrap();

    wait_for_client_shutdown(&real_node);
    ensure_no_further_traffic(&mock_node, &masquerader);
}

#[test]
// Given: Exit Node is real_node; originating Node is mock_node.
// Given: A stream is established through the exit Node to a server.
// When: Server drops connection to exit Node.
// Then: Exit Node sends ClientRequestPayload to originating Node with empty SequencedPacket having last_data = true.
fn actual_server_drop() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let (real_node, mock_node, _) = create_neighborhood(&mut cluster);
    let server_port = find_free_port();
    let mut server = real_node.make_server(server_port);
    let masquerader = JsonMasquerader::new();
    let (stream_key, return_route_id) = arbitrary_context();
    mock_node
        .transmit_package(
            mock_node.port_list()[0],
            create_request_icp(
                &mock_node,
                &real_node,
                stream_key,
                return_route_id,
                &server,
                cluster.chain_id,
            ),
            &masquerader,
            real_node.public_key(),
            real_node.socket_addr(PortSelector::First),
        )
        .unwrap();
    server.wait_for_chunk(Duration::from_secs(2)).unwrap();
    server.send_chunk(HTTP_RESPONSE);
    mock_node
        .wait_for_package(&masquerader, Duration::from_secs(2))
        .unwrap();

    server.shutdown();

    // Send another package to trigger do_housekeeping() in the StreamHandlerPool: agh.
    mock_node
        .transmit_package(
            mock_node.port_list()[0],
            create_meaningless_icp(&mock_node, &real_node),
            &masquerader,
            real_node.public_key(),
            real_node.socket_addr(PortSelector::First),
        )
        .unwrap();
    let (_, _, lcp) = mock_node
        .wait_for_package(&masquerader, Duration::from_secs(1))
        .unwrap();
    let payload =
        match decodex::<MessageType>(mock_node.cryptde_null().unwrap(), &lcp.payload).unwrap() {
            MessageType::ClientResponse(p) => p,
            mt => panic!("Unexpected: {:?}", mt),
        };
    assert!(payload.sequenced_packet.data.is_empty());
    assert!(payload.sequenced_packet.last_data);
}

#[test]
// Given: Exit Node is real_node; originating Node is mock_node.
// Given: A stream is established through the exit Node to a server.
// When: Exit Node receives empty SequencedPacket having last_data = true.
// Then: Exit Node drops connection to server.
// Then: Exit Node does _not_ send CORES package back to originating Node.
fn reported_client_drop() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let (real_node, mock_node, _) = create_neighborhood(&mut cluster);
    let server_port = find_free_port();
    let mut server = real_node.make_server(server_port);
    let masquerader = JsonMasquerader::new();
    let (stream_key, return_route_id) = arbitrary_context();
    mock_node
        .transmit_package(
            mock_node.port_list()[0],
            create_request_icp(
                &mock_node,
                &real_node,
                stream_key,
                return_route_id,
                &server,
                cluster.chain_id,
            ),
            &masquerader,
            real_node.public_key(),
            real_node.socket_addr(PortSelector::First),
        )
        .unwrap();
    server.wait_for_chunk(Duration::from_secs(1)).unwrap();
    server.send_chunk(HTTP_RESPONSE);
    mock_node
        .wait_for_package(&masquerader, Duration::from_secs(1))
        .unwrap();

    mock_node
        .transmit_package(
            mock_node.port_list()[0],
            create_client_drop_report(&mock_node, &real_node, stream_key, return_route_id),
            &masquerader,
            real_node.public_key(),
            real_node.socket_addr(PortSelector::First),
        )
        .unwrap();

    wait_for_server_shutdown(&real_node, server_port);
    ensure_no_further_traffic(&mock_node, &masquerader);
}

fn create_neighborhood(
    cluster: &mut SubstratumNodeCluster,
) -> (SubstratumRealNode, SubstratumMockNode, PublicKey) {
    let mut real_node: NodeRecord = make_node_record(1234, true);
    let mut mock_node: NodeRecord = make_node_record(2345, true);
    let mut fictional_node_1: NodeRecord = make_node_record(3456, true);
    let mut fictional_node_2: NodeRecord = make_node_record(4567, true);
    full_neighbor(&mut real_node, &mut mock_node);
    full_neighbor(&mut mock_node, &mut fictional_node_1);
    full_neighbor(&mut fictional_node_1, &mut fictional_node_2);
    let mut db: NeighborhoodDatabase = db_from_node(&real_node);
    full_neighbor(db.root_mut(), &mut mock_node);
    db.add_node(mock_node.clone()).unwrap();
    db.add_node(fictional_node_1.clone()).unwrap();
    db.add_node(fictional_node_2.clone()).unwrap();
    let (_, substratum_real_node, mut node_map) = construct_neighborhood(cluster, db, vec![]);
    let substratum_mock_node = node_map.remove(mock_node.public_key()).unwrap();
    (
        substratum_real_node,
        substratum_mock_node,
        fictional_node_2.public_key().clone(),
    )
}

fn full_neighbor(one: &mut NodeRecord, another: &mut NodeRecord) {
    one.add_half_neighbor_key(another.public_key().clone())
        .unwrap();
    another
        .add_half_neighbor_key(one.public_key().clone())
        .unwrap();
}

fn context_from_request_lcp(
    lcp: LiveCoresPackage,
    originating_cryptde: &dyn CryptDE,
    exit_cryptde: &dyn CryptDE,
) -> (StreamKey, u32) {
    let payload = match decodex::<MessageType>(exit_cryptde, &lcp.payload).unwrap() {
        MessageType::ClientRequest(p) => p,
        mt => panic!("Unexpected: {:?}", mt),
    };
    let stream_key = payload.stream_key;
    let return_route_id = decodex::<u32>(originating_cryptde, &lcp.route.hops[6]).unwrap();
    (stream_key, return_route_id)
}

fn arbitrary_context() -> (StreamKey, u32) {
    (make_meaningless_stream_key(), 12345678)
}

fn create_request_icp(
    originating_node: &SubstratumMockNode,
    exit_node: &SubstratumRealNode,
    stream_key: StreamKey,
    return_route_id: u32,
    server: &SubstratumNodeServer,
    chain_id: u8,
) -> IncipientCoresPackage {
    IncipientCoresPackage::new(
        originating_node.cryptde_null().unwrap(),
        Route::round_trip(
            RouteSegment::new(
                vec![originating_node.public_key(), exit_node.public_key()],
                Component::ProxyClient,
            ),
            RouteSegment::new(
                vec![exit_node.public_key(), originating_node.public_key()],
                Component::ProxyServer,
            ),
            originating_node.cryptde_null().unwrap(),
            originating_node.consuming_wallet(),
            return_route_id,
            Some(contract_address(chain_id)),
        )
        .unwrap(),
        MessageType::ClientRequest(ClientRequestPayload {
            version: DataVersion::new(0, 0).unwrap(),
            stream_key,
            sequenced_packet: SequencedPacket::new(Vec::from(HTTP_REQUEST), 0, false),
            target_hostname: Some(format!("{}", server.socket_addr().ip())),
            target_port: server.socket_addr().port(),
            protocol: ProxyProtocol::HTTP,
            originator_public_key: originating_node.public_key().clone(),
        }),
        exit_node.public_key(),
    )
    .unwrap()
}

fn create_meaningless_icp(
    originating_node: &SubstratumMockNode,
    exit_node: &SubstratumRealNode,
) -> IncipientCoresPackage {
    let socket_addr = SocketAddr::from_str("3.2.1.0:7654").unwrap();
    let stream_key = StreamKey::new(PublicKey::new(&[9, 8, 7, 6]), socket_addr);
    IncipientCoresPackage::new(
        originating_node.cryptde_null().unwrap(),
        Route::round_trip(
            RouteSegment::new(
                vec![originating_node.public_key(), exit_node.public_key()],
                Component::ProxyClient,
            ),
            RouteSegment::new(
                vec![exit_node.public_key(), originating_node.public_key()],
                Component::ProxyServer,
            ),
            originating_node.cryptde_null().unwrap(),
            originating_node.consuming_wallet(),
            1357,
            Some(contract_address(DEFAULT_CHAIN_ID)),
        )
        .unwrap(),
        MessageType::ClientRequest(ClientRequestPayload {
            version: DataVersion::new(0, 0).unwrap(),
            stream_key,
            sequenced_packet: SequencedPacket::new(Vec::from(HTTP_REQUEST), 0, false),
            target_hostname: Some(format!("nowhere.com")),
            target_port: socket_addr.port(),
            protocol: ProxyProtocol::HTTP,
            originator_public_key: originating_node.public_key().clone(),
        }),
        exit_node.public_key(),
    )
    .unwrap()
}

fn create_server_drop_report(
    exit_node: &SubstratumMockNode,
    originating_node: &SubstratumRealNode,
    stream_key: StreamKey,
    return_route_id: u32,
) -> IncipientCoresPackage {
    let mut route = Route::round_trip(
        RouteSegment::new(
            vec![originating_node.public_key(), exit_node.public_key()],
            Component::ProxyClient,
        ),
        RouteSegment::new(
            vec![exit_node.public_key(), originating_node.public_key()],
            Component::ProxyServer,
        ),
        originating_node.cryptde_null().unwrap(),
        originating_node.consuming_wallet(),
        return_route_id,
        Some(contract_address(DEFAULT_CHAIN_ID)),
    )
    .unwrap();
    route
        .shift(originating_node.cryptde_null().unwrap())
        .unwrap();
    let payload = MessageType::ClientResponse(ClientResponsePayload {
        version: DataVersion::new(0, 0).unwrap(),
        stream_key,
        sequenced_packet: SequencedPacket::new(vec![], 0, true),
    });

    IncipientCoresPackage::new(
        exit_node.cryptde_null().unwrap(),
        route,
        payload,
        originating_node.public_key(),
    )
    .unwrap()
}

fn create_client_drop_report(
    originating_node: &SubstratumMockNode,
    exit_node: &SubstratumRealNode,
    stream_key: StreamKey,
    return_route_id: u32,
) -> IncipientCoresPackage {
    let route = Route::round_trip(
        RouteSegment::new(
            vec![originating_node.public_key(), exit_node.public_key()],
            Component::ProxyClient,
        ),
        RouteSegment::new(
            vec![exit_node.public_key(), originating_node.public_key()],
            Component::ProxyServer,
        ),
        originating_node.cryptde_null().unwrap(),
        originating_node.consuming_wallet(),
        return_route_id,
        Some(contract_address(DEFAULT_CHAIN_ID)),
    )
    .unwrap();
    let payload = MessageType::ClientRequest(ClientRequestPayload {
        version: DataVersion::new(0, 0).unwrap(),
        stream_key,
        sequenced_packet: SequencedPacket::new(vec![], 1, true),
        target_hostname: Some(String::from("doesnt.matter.com")),
        target_port: 80,
        protocol: ProxyProtocol::HTTP,
        originator_public_key: originating_node.public_key().clone(),
    });

    IncipientCoresPackage::new(
        originating_node.cryptde_null().unwrap(),
        route,
        payload,
        exit_node.public_key(),
    )
    .unwrap()
}

fn ensure_no_further_traffic(mock_node: &SubstratumMockNode, masquerader: &dyn Masquerader) {
    match mock_node.wait_for_package(masquerader, Duration::from_secs(1)) {
        Ok((addr1, addr2, lcp)) => panic!(
            "Should not have received package, but: {:?} -> {:?}:\n{:?}",
            addr1, addr2, lcp
        ),
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => (), // expected: pass
        Err(ref e) => panic!("Unexpected error: {:?}", e),
    }
}

fn wait_for_client_shutdown(real_node: &SubstratumRealNode) {
    // This is a jury-rigged way to wait for a shutdown, since client.wait_for_shutdown() doesn't
    // work, but it serves the purpose.
    SubstratumNodeUtils::wrote_log_containing(
        real_node.name(),
        "Shutting down stream to client at 127.0.0.1",
        Duration::from_secs(1),
    );
}

fn wait_for_server_shutdown(real_node: &SubstratumRealNode, server_port: u16) {
    // This is a jury-rigged way to wait for a shutdown, since server.wait_for_shutdown() doesn't
    // work, but it serves the purpose.
    SubstratumNodeUtils::wrote_log_containing(
        real_node.name(),
        &format!(
            "Shutting down stream to server at {}:{} in response to client-drop report",
            SubstratumNodeCluster::host_ip_addr(),
            server_port
        ),
        Duration::from_secs(1),
    );
}
