use drbd_reactor::drbd::{
    Connection, Device, EventType, Path, PeerDevice, PluginUpdate, Resource, Role,
};

#[test]
fn get_resource_update() {
    let mut r = Resource {
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

    // update with self
    assert!(r
        .get_resource_update(&EventType::Exists, &r.clone())
        .is_none());

    let mut u = r.clone();
    u.may_promote = false;
    let up = r.get_resource_update(&EventType::Exists, &u).unwrap();
    match up {
        PluginUpdate::Resource(u) => {
            assert_eq!(u.event_type, EventType::Exists);
            assert_eq!(u.old.may_promote, true);
            assert_eq!(u.new.may_promote, false);
        }
        _ => panic!("not a resorce update"),
    }

    // destroy still needs to be an update
    assert!(r.get_resource_update(&EventType::Destroy, &u).is_some());
}

#[test]
fn get_device_update() {
    let mut r = Resource::with_name("foo");
    let d = Device {
        volume: 0,
        ..Default::default()
    };
    let ds = d.clone();
    r.devices.push(d);

    // update with existing
    assert!(r.get_device_update(&EventType::Exists, &ds).is_none());

    let mut u = ds.clone();
    u.quorum = true;
    let up = r.get_device_update(&EventType::Exists, &u).unwrap();
    match up {
        PluginUpdate::Device(u) => {
            assert_eq!(u.event_type, EventType::Exists);
            assert_eq!(u.old.quorum, false);
            assert_eq!(u.new.quorum, true);
            assert_eq!(u.volume, 0);
        }
        _ => panic!("not a device update"),
    }

    // destroy still needs to be an update
    assert!(r.get_device_update(&EventType::Destroy, &u).is_some());
}

#[test]
fn get_connection_update() {
    let mut r = Resource::with_name("foo");
    let mut c = Connection {
        peer_node_id: 1,
        ..Default::default()
    };

    let pd = PeerDevice {
        peer_node_id: 1,
        volume: 1,
        ..Default::default()
    };
    c.peerdevices.push(pd);
    let cs = c.clone();
    r.connections.push(c);

    // update with existing
    assert!(r.get_connection_update(&EventType::Exists, &cs).is_none());

    let mut u = cs.clone();
    u.congested = true;
    let up = r.get_connection_update(&EventType::Exists, &u).unwrap();
    match up {
        PluginUpdate::Connection(u) => {
            assert_eq!(u.event_type, EventType::Exists);
            assert_eq!(u.old.congested, false);
            assert_eq!(u.new.congested, true);
            assert_eq!(u.peer_node_id, 1);
        }
        _ => panic!("not a connection update"),
    }
    //check that updated did not delete existing pd
    assert!(r.connections[0].peerdevices.len() == 1);

    // destroy still needs to be an update
    assert!(r.get_connection_update(&EventType::Destroy, &u).is_some());
}

#[test]
fn get_peerdevice_update() {
    let mut r = Resource::with_name("foo");
    let mut c = Connection {
        peer_node_id: 1,
        ..Default::default()
    };

    let pd = PeerDevice {
        peer_node_id: 1,
        volume: 1,
        ..Default::default()
    };

    let pds = pd.clone();
    c.peerdevices.push(pd);
    r.connections.push(c);

    // update with existing
    assert!(r.get_peerdevice_update(&EventType::Exists, &pds).is_none());

    let mut u = pds.clone();
    u.peer_client = true;
    let up = r.get_peerdevice_update(&EventType::Change, &u).unwrap();
    match up {
        PluginUpdate::PeerDevice(u) => {
            assert_eq!(u.event_type, EventType::Change);
            assert_eq!(u.old.peer_client, false);
            assert_eq!(u.new.peer_client, true);
            assert_eq!(u.peer_node_id, 1);
            assert_eq!(u.volume, 1);
        }
        _ => panic!("not a peerdevice update"),
    }

    // destroy still needs to be an update
    assert!(r.get_peerdevice_update(&EventType::Destroy, &u).is_some());
}

#[test]
fn get_path_update() {
    let mut r = Resource::with_name("foo");
    let c = Connection {
        peer_node_id: 1,
        ..Default::default()
    };

    r.connections.push(c);

    let p = Path {
        peer_node_id: 1,
        ..Default::default()
    };

    assert!(r.get_path_update(&EventType::Change, &p).is_none());
    // but updates resource state
    assert_eq!(r.connections[0].paths[0].peer_node_id, 1);

    assert!(r.get_path_update(&EventType::Destroy, &p).is_none());
    assert!(r.connections[0].paths.is_empty());
}
