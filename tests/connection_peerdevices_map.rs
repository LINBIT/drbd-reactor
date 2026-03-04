use drbd_reactor::drbd::{DiskState, EventType, PeerDevice, PluginUpdate, Resource};

fn make_peerdevice(peer_node_id: i32, volume: i32) -> PeerDevice {
    PeerDevice {
        peer_node_id,
        volume,
        ..Default::default()
    }
}

#[test]
fn insert_and_count() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 5));
    r.update_peerdevice(&make_peerdevice(1, 2));
    r.update_peerdevice(&make_peerdevice(1, 0));
    assert_eq!(r.connections.len(), 1);
    assert_eq!(r.get_connection(1).unwrap().peerdevices.len(), 3);
}

#[test]
fn insert_creates_connection() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 0));
    r.update_peerdevice(&make_peerdevice(2, 0));
    assert_eq!(r.connections.len(), 2);
}

#[test]
fn insert_is_retrievable() {
    let mut r = Resource::with_name("foo");
    let pd = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        peer_client: true,
        ..Default::default()
    };
    r.update_peerdevice(&pd);

    // Same state => no update
    assert!(r
        .get_peerdevice_update(&EventType::Exists, &pd)
        .is_none());
}

#[test]
fn update_in_place() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 0));
    r.update_peerdevice(&make_peerdevice(1, 1));

    let modified = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        sent: 42,
        ..Default::default()
    };
    r.update_peerdevice(&modified);
    assert_eq!(r.get_connection(1).unwrap().peerdevices.len(), 2);

    // Verify change is observable via get_peerdevice_update
    let probe = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        peer_client: true,
        ..Default::default()
    };
    let up = r
        .get_peerdevice_update(&EventType::Exists, &probe)
        .unwrap();
    match up {
        PluginUpdate::PeerDevice(pdu) => {
            assert_eq!(pdu.peer_node_id, 1);
            assert_eq!(pdu.volume, 0);
            assert!(!pdu.old.peer_client);
            assert!(pdu.new.peer_client);
        }
        _ => panic!("expected peerdevice update"),
    }
}

#[test]
fn delete_existing() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 0));
    r.update_peerdevice(&make_peerdevice(1, 1));
    r.update_peerdevice(&make_peerdevice(1, 2));

    r.delete_peerdevice(1, 1);
    assert_eq!(r.get_connection(1).unwrap().peerdevices.len(), 2);

    // Volume 1 is gone
    let pd = PeerDevice {
        peer_node_id: 1,
        volume: 1,
        peer_client: true,
        ..Default::default()
    };
    let up = r.get_peerdevice_update(&EventType::Exists, &pd).unwrap();
    match up {
        PluginUpdate::PeerDevice(pdu) => {
            assert!(!pdu.old.peer_client);
            assert!(pdu.new.peer_client);
        }
        _ => panic!("expected peerdevice update"),
    }
}

#[test]
fn delete_nonexistent() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 0));

    r.delete_peerdevice(1, 99);
    assert_eq!(r.get_connection(1).unwrap().peerdevices.len(), 1);
}

#[test]
fn get_peerdevice_update_no_change() {
    let mut r = Resource::with_name("foo");
    let pd = make_peerdevice(1, 0);
    r.update_peerdevice(&pd);

    assert!(r
        .get_peerdevice_update(&EventType::Exists, &pd)
        .is_none());
}

#[test]
fn get_peerdevice_update_with_change() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 0));

    let modified = PeerDevice {
        peer_node_id: 1,
        volume: 0,
        peer_disk_state: DiskState::UpToDate,
        ..Default::default()
    };
    let up = r
        .get_peerdevice_update(&EventType::Change, &modified)
        .unwrap();
    match up {
        PluginUpdate::PeerDevice(pdu) => {
            assert_eq!(pdu.event_type, EventType::Change);
            assert_eq!(pdu.peer_node_id, 1);
            assert_eq!(pdu.volume, 0);
            assert_eq!(pdu.old.peer_disk_state, DiskState::DUnknown);
            assert_eq!(pdu.new.peer_disk_state, DiskState::UpToDate);
        }
        _ => panic!("expected peerdevice update"),
    }
}

#[test]
fn get_peerdevice_update_destroy() {
    let mut r = Resource::with_name("foo");
    let pd = make_peerdevice(1, 0);
    r.update_peerdevice(&pd);

    let up = r
        .get_peerdevice_update(&EventType::Destroy, &pd)
        .unwrap();
    match up {
        PluginUpdate::PeerDevice(pdu) => {
            assert_eq!(pdu.event_type, EventType::Destroy);
        }
        _ => panic!("expected peerdevice update"),
    }
    assert_eq!(r.get_connection(1).unwrap().peerdevices.len(), 0);
}

#[test]
fn serde_round_trip() {
    let mut r = Resource::with_name("foo");
    r.update_peerdevice(&make_peerdevice(1, 2));
    r.update_peerdevice(&make_peerdevice(1, 0));

    let json = serde_json::to_string(&r).unwrap();
    let r2: Resource = serde_json::from_str(&json).unwrap();
    assert_eq!(r, r2);
}
