use drbd_reactor::drbd::{Device, DiskState, EventType, PluginUpdate, Resource};

fn make_device(volume: i32, minor: i32) -> Device {
    Device {
        volume,
        minor,
        ..Default::default()
    }
}

#[test]
fn insert_and_count() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(5, 50));
    r.update_device(&make_device(2, 20));
    r.update_device(&make_device(0, 10));
    assert_eq!(r.devices.len(), 3);
}

#[test]
fn insert_is_retrievable() {
    let mut r = Resource::with_name("foo");
    let d = Device {
        volume: 3,
        minor: 30,
        quorum: true,
        ..Default::default()
    };
    r.update_device(&d);

    // Retrievable via get_device_update with same state => None (no change)
    assert!(r.get_device_update(&EventType::Exists, &d).is_none());
}

#[test]
fn update_in_place() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(1, 10));
    r.update_device(&make_device(2, 20));

    // Update volume 1 with new minor
    r.update_device(&make_device(1, 99));
    assert_eq!(r.devices.len(), 2);

    // The updated value should be observable: changing quorum triggers an update
    let probe = Device {
        volume: 1,
        minor: 99,
        quorum: true,
        ..Default::default()
    };
    let up = r.get_device_update(&EventType::Exists, &probe).unwrap();
    match up {
        PluginUpdate::Device(du) => {
            assert_eq!(du.volume, 1);
            assert!(!du.old.quorum);
            assert!(du.new.quorum);
        }
        _ => panic!("expected device update"),
    }
}

#[test]
fn delete_existing() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(0, 10));
    r.update_device(&make_device(1, 20));
    r.update_device(&make_device(2, 30));

    r.delete_device(1);
    assert_eq!(r.devices.len(), 2);

    // Volume 1 is gone: inserting it again should show up as new (old has defaults)
    let d = Device {
        volume: 1,
        minor: 20,
        quorum: true,
        ..Default::default()
    };
    let up = r.get_device_update(&EventType::Exists, &d).unwrap();
    match up {
        PluginUpdate::Device(du) => {
            assert_eq!(du.volume, 1);
            // old state should be default since it was deleted
            assert!(!du.old.quorum);
            assert!(du.new.quorum);
        }
        _ => panic!("expected device update"),
    }
}

#[test]
fn delete_nonexistent() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(0, 10));

    r.delete_device(99);
    assert_eq!(r.devices.len(), 1);
}

#[test]
fn get_device_update_no_change() {
    let mut r = Resource::with_name("foo");
    let d = make_device(0, 10);
    r.update_device(&d);

    assert!(r.get_device_update(&EventType::Exists, &d).is_none());
}

#[test]
fn get_device_update_with_change() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(0, 10));

    let modified = Device {
        volume: 0,
        minor: 10,
        disk_state: DiskState::UpToDate,
        ..Default::default()
    };
    let up = r.get_device_update(&EventType::Change, &modified).unwrap();
    match up {
        PluginUpdate::Device(du) => {
            assert_eq!(du.event_type, EventType::Change);
            assert_eq!(du.volume, 0);
            assert_eq!(du.old.disk_state, DiskState::DUnknown);
            assert_eq!(du.new.disk_state, DiskState::UpToDate);
        }
        _ => panic!("expected device update"),
    }
}

#[test]
fn get_device_update_destroy() {
    let mut r = Resource::with_name("foo");
    let d = make_device(0, 10);
    r.update_device(&d);

    // Destroy always generates an update, even with no state change
    let up = r.get_device_update(&EventType::Destroy, &d).unwrap();
    match up {
        PluginUpdate::Device(du) => {
            assert_eq!(du.event_type, EventType::Destroy);
            assert_eq!(du.volume, 0);
        }
        _ => panic!("expected device update"),
    }

    // Device should be removed after destroy
    assert_eq!(r.devices.len(), 0);
}

#[test]
fn get_device_update_new_device() {
    let mut r = Resource::with_name("foo");
    let d = Device {
        volume: 0,
        minor: 10,
        quorum: true,
        ..Default::default()
    };

    // First time seeing this device => should report an update with default old state
    let up = r.get_device_update(&EventType::Exists, &d).unwrap();
    match up {
        PluginUpdate::Device(du) => {
            assert_eq!(du.event_type, EventType::Exists);
            assert_eq!(du.volume, 0);
            assert!(!du.old.quorum);
            assert!(du.new.quorum);
        }
        _ => panic!("expected device update"),
    }
    assert_eq!(r.devices.len(), 1);
}

#[test]
fn serde_round_trip() {
    let mut r = Resource::with_name("foo");
    r.update_device(&make_device(2, 20));
    r.update_device(&make_device(0, 10));

    let json = serde_json::to_string(&r).unwrap();
    let r2: Resource = serde_json::from_str(&json).unwrap();
    assert_eq!(r, r2);
}
