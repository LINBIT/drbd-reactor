use drbd_reactor::drbd::{Connection, EventType, Path, Resource};

fn make_path(peer_node_id: i32, local: &str, peer: &str) -> Path {
    Path {
        peer_node_id,
        local: local.to_string(),
        peer: peer.to_string(),
        ..Default::default()
    }
}

#[test]
fn insert_and_count() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&Connection {
        peer_node_id: 1,
        ..Default::default()
    });
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));
    r.update_path(&make_path(1, "10.0.1.1:7000", "10.0.1.2:7000"));
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 2);
}

#[test]
fn insert_creates_connection() {
    let mut r = Resource::with_name("foo");
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));
    assert_eq!(r.connections.len(), 1);
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 1);
}

#[test]
fn update_in_place() {
    let mut r = Resource::with_name("foo");
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));

    let modified = Path {
        peer_node_id: 1,
        local: "10.0.0.1:7000".to_string(),
        peer: "10.0.0.2:7000".to_string(),
        established: true,
        ..Default::default()
    };
    r.update_path(&modified);
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 1);
}

#[test]
fn delete_existing() {
    let mut r = Resource::with_name("foo");
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));
    r.update_path(&make_path(1, "10.0.1.1:7000", "10.0.1.2:7000"));

    r.delete_path(1, "10.0.0.1:7000", "10.0.0.2:7000");
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 1);
}

#[test]
fn delete_nonexistent() {
    let mut r = Resource::with_name("foo");
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));

    r.delete_path(1, "99.99.99.99:7000", "99.99.99.99:7000");
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 1);
}

#[test]
fn get_path_update_insert_and_destroy() {
    let mut r = Resource::with_name("foo");
    r.update_connection(&Connection {
        peer_node_id: 1,
        ..Default::default()
    });

    let p = make_path(1, "10.0.0.1:7000", "10.0.0.2:7000");

    // get_path_update always returns None but updates internal state
    assert!(r.get_path_update(&EventType::Change, &p).is_none());
    assert_eq!(r.get_connection(1).unwrap().paths.len(), 1);

    assert!(r.get_path_update(&EventType::Destroy, &p).is_none());
    assert!(r.get_connection(1).unwrap().paths.is_empty());
}

#[test]
fn serde_round_trip() {
    let mut r = Resource::with_name("foo");
    r.update_path(&make_path(1, "10.0.1.1:7000", "10.0.1.2:7000"));
    r.update_path(&make_path(1, "10.0.0.1:7000", "10.0.0.2:7000"));

    let json = serde_json::to_string(&r).unwrap();
    let r2: Resource = serde_json::from_str(&json).unwrap();
    assert_eq!(r, r2);
}
