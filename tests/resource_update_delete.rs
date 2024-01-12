use drbd_reactor::drbd::{Connection, Device, PeerDevice, Resource, Role};

#[test]
fn resource_update() {
    let mut r = Resource::with_name("foo");
    let update = Resource {
        name: "foo".to_string(),
        role: Role::Primary,
        suspended: true,
        write_ordering: "foo".to_string(),
        may_promote: true,
        promotion_score: 23,
        force_io_failures: false,
        devices: vec![],
        connections: vec![],
    };
    r.update(&update);

    assert_eq!(r, update);
}

fn get_device_update() -> Resource {
    let mut r = Resource::with_name("foo");
    let d0 = Device {
        volume: 0,
        ..Default::default()
    };
    let d1 = Device {
        volume: 1,
        ..Default::default()
    };

    r.update_device(&d0);
    r.update_device(&d1);
    assert_eq!(r.devices.len(), 2);
    assert_eq!(r.devices[0].volume, 0);
    assert_eq!(r.devices[1].volume, 1);
    assert_eq!(r.devices[1].minor, 0);

    let d1_mod = Device {
        volume: 1,
        minor: 1,
        ..Default::default()
    };

    r.update_device(&d1_mod);
    assert_eq!(r.devices.len(), 2);
    assert_eq!(r.devices[1].volume, 1);
    assert_eq!(r.devices[1].minor, 1);

    r
}

#[test]
fn device_update() {
    get_device_update();
}

#[test]
fn device_delete() {
    let mut r = get_device_update();
    r.delete_device(1);
    assert_eq!(r.devices.len(), 1);
    assert_eq!(r.devices[0].volume, 0);

    r.delete_device(0);
    assert_eq!(r.devices.len(), 0);
}

fn get_connection_update() -> Resource {
    let mut r = Resource::with_name("foo");
    let c0 = Connection {
        peer_node_id: 1,
        ..Default::default()
    };
    let c1 = Connection {
        peer_node_id: 2,
        ..Default::default()
    };

    r.update_connection(&c0);
    r.update_connection(&c1);
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.connections[0].peer_node_id, 1);
    assert!(!r.connections[0].congested);
    assert_eq!(r.connections[1].peer_node_id, 2);

    let c0_mod = Connection {
        peer_node_id: 1,
        congested: true,
        ..Default::default()
    };

    r.update_connection(&c0_mod);
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.connections[0].peer_node_id, 1);
    assert!(r.connections[0].congested);

    r
}

#[test]
fn connection_update() {
    get_connection_update();
}

#[test]
fn connection_delete() {
    let mut r = get_connection_update();

    r.delete_connection(1);
    assert_eq!(r.connections.len(), 1);
    assert_eq!(r.connections[0].peer_node_id, 2);

    r.delete_connection(2);
    assert_eq!(r.connections.len(), 0);
}

fn get_peerdevice_update() -> Resource {
    let mut r = Resource::with_name("foo");
    let pd10 = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        ..Default::default()
    };
    let pd11 = PeerDevice {
        peer_node_id: 1,
        volume: 1,
        ..Default::default()
    };

    let pd20 = PeerDevice {
        peer_node_id: 2,
        volume: 0,
        ..Default::default()
    };
    let pd21 = PeerDevice {
        peer_node_id: 2,
        volume: 1,
        ..Default::default()
    };

    r.update_peerdevice(&pd10);
    r.update_peerdevice(&pd11);
    r.update_peerdevice(&pd20);
    r.update_peerdevice(&pd21);
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.connections[0].peerdevices.len(), 2);
    assert_eq!(r.connections[1].peerdevices.len(), 2);
    assert_eq!(r.connections[0].peerdevices[0].peer_node_id, 1);
    assert_eq!(r.connections[0].peerdevices[0].volume, 0);
    assert_eq!(r.connections[0].peerdevices[1].peer_node_id, 1);
    assert_eq!(r.connections[0].peerdevices[1].volume, 1);
    assert_eq!(r.connections[1].peerdevices[0].peer_node_id, 2);
    assert_eq!(r.connections[1].peerdevices[0].volume, 0);
    assert_eq!(r.connections[1].peerdevices[0].sent, 0);
    assert_eq!(r.connections[1].peerdevices[1].peer_node_id, 2);
    assert_eq!(r.connections[1].peerdevices[1].volume, 1);

    let pd20_mod = PeerDevice {
        peer_node_id: 2,
        volume: 0,
        sent: 23,
        ..Default::default()
    };

    r.update_peerdevice(&pd20_mod);
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.connections[0].peerdevices.len(), 2);
    assert_eq!(r.connections[1].peerdevices.len(), 2);
    assert_eq!(r.connections[1].peerdevices[0].sent, 23);

    r
}

#[test]
fn peerdevice_update() {
    get_peerdevice_update();
}

#[test]
fn peerdevice_delete() {
    let mut r = get_peerdevice_update();

    r.delete_peerdevice(1, 0);
    r.delete_peerdevice(2, 1);
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.connections[0].peerdevices.len(), 1);
    assert_eq!(r.connections[1].peerdevices.len(), 1);
    assert_eq!(r.connections[0].peerdevices[0].volume, 1);
    assert_eq!(r.connections[0].peerdevices[0].peer_node_id, 1);
    assert_eq!(r.connections[1].peerdevices[0].volume, 0);
    assert_eq!(r.connections[1].peerdevices[0].peer_node_id, 2);
}
