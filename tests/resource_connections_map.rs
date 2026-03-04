use drbd_reactor::drbd::{
    Connection, ConnectionState, EventType, PeerDevice, PluginUpdate, Resource,
};

fn make_connection(peer_node_id: i32) -> Connection {
    Connection {
        peer_node_id,
        ..Default::default()
    }
}

#[test]
fn insert_and_count() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(5));
    r.update_connection(&make_connection(2));
    r.update_connection(&make_connection(0));
    assert_eq!(r.connections.len(), 3);
}

#[test]
fn insert_is_retrievable() {
    let mut r = Resource::with_name("foo");
    let c = Connection {
        peer_node_id: 3,
        congested: true,
        ..Default::default()
    };
    r.update_connection(&c);

    assert!(r.get_connection_update(&EventType::Exists, &c).is_none());
}

#[test]
fn update_in_place() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(1));
    r.update_connection(&make_connection(2));

    let modified = Connection {
        peer_node_id: 1,
        congested: true,
        ..Default::default()
    };
    r.update_connection(&modified);
    assert_eq!(r.connections.len(), 2);

    let probe = Connection {
        peer_node_id: 1,
        congested: false,
        ..Default::default()
    };
    let up = r.get_connection_update(&EventType::Exists, &probe).unwrap();
    match up {
        PluginUpdate::Connection(cu) => {
            assert_eq!(cu.peer_node_id, 1);
            assert!(cu.old.congested);
            assert!(!cu.new.congested);
        }
        _ => panic!("expected connection update"),
    }
}

#[test]
fn delete_existing() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(0));
    r.update_connection(&make_connection(1));
    r.update_connection(&make_connection(2));

    r.delete_connection(1);
    assert_eq!(r.connections.len(), 2);

    let c = Connection {
        peer_node_id: 1,
        congested: true,
        ..Default::default()
    };
    let up = r.get_connection_update(&EventType::Exists, &c).unwrap();
    match up {
        PluginUpdate::Connection(cu) => {
            assert_eq!(cu.peer_node_id, 1);
            assert!(!cu.old.congested);
            assert!(cu.new.congested);
        }
        _ => panic!("expected connection update"),
    }
}

#[test]
fn delete_nonexistent() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(0));

    r.delete_connection(99);
    assert_eq!(r.connections.len(), 1);
}

#[test]
fn get_connection_update_no_change() {
    let mut r = Resource::with_name("foo");
    let c = make_connection(0);
    r.update_connection(&c);

    assert!(r.get_connection_update(&EventType::Exists, &c).is_none());
}

#[test]
fn get_connection_update_with_change() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(0));

    let modified = Connection {
        peer_node_id: 0,
        connection: ConnectionState::Connected,
        ..Default::default()
    };
    let up = r
        .get_connection_update(&EventType::Change, &modified)
        .unwrap();
    match up {
        PluginUpdate::Connection(cu) => {
            assert_eq!(cu.event_type, EventType::Change);
            assert_eq!(cu.peer_node_id, 0);
        }
        _ => panic!("expected connection update"),
    }
}

#[test]
fn get_connection_update_destroy() {
    let mut r = Resource::with_name("foo");
    let c = make_connection(0);
    r.update_connection(&c);

    let up = r.get_connection_update(&EventType::Destroy, &c).unwrap();
    match up {
        PluginUpdate::Connection(cu) => {
            assert_eq!(cu.event_type, EventType::Destroy);
            assert_eq!(cu.peer_node_id, 0);
        }
        _ => panic!("expected connection update"),
    }
    assert_eq!(r.connections.len(), 0);
}

#[test]
fn get_connection_update_preserves_peerdevices() {
    let mut r = Resource::with_name("foo");

    let pd = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        ..Default::default()
    };
    r.update_peerdevice(&pd);
    assert_eq!(r.connections.len(), 1);

    let c = Connection {
        peer_node_id: 1,
        congested: true,
        ..Default::default()
    };
    let up = r.get_connection_update(&EventType::Exists, &c).unwrap();
    match up {
        PluginUpdate::Connection(cu) => {
            assert!(cu.new.congested);
        }
        _ => panic!("expected connection update"),
    }

    // Peerdevice still there
    assert!(r.get_peerdevice_update(&EventType::Exists, &pd).is_none());
}

#[test]
fn serde_round_trip() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&make_connection(2));
    r.update_connection(&make_connection(0));

    let json = serde_json::to_string(&r).unwrap();
    let r2: Resource = serde_json::from_str(&json).unwrap();
    assert_eq!(r, r2);
}
