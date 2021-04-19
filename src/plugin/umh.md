# umh

The user mode helper is a plugin that allows users to specify filters matched
on DRBD state changes. If a filter rule matches, a user defined script gets
executed. Information is passed to scripts via environment variables. For every
event all rules are evaluated, so if multiple rules match, multiple actions are
executed (concurrently).

There are 4 different types a rule can be defined for:

- [resource](umh.md#resource-rules)
- [device](umh.md#device-rules)
- [peerdevice](umh.md#peer-device-rules)
- [connection](umh.md#connection-rules)

Before diving into a more formal specification of the rules, let's start with a hello world example. Let's
assume you want to call a script whenever resource `foo` changes from `Secondary` to `Primary`.

```
[[umh]]
[[umh.resource]]
name = "resource now primary"
command = "/usr/local/bin/primary.sh"
resource-name = "foo"
old.role = "Secondary"
new.role = "Primary"
```

It is important to note that fields not specified do not restrict the filter. Think of them as "don't care".
Make sure to read and understand the implied [caveats](./umh.md#caveats).

IMPORTANT: backing device information needs

- drbd-utils `>= 9.17.0`
- kernel module `>= 9.1.1` if using `9.1.z` or `>= 9.0.28` if using `9.0.z`

Otherwise it will be set to "none".


# Common fields
Every rule type (e.g., resource, device,...) has a set of common fields

| Common fields | Description                            | Type   | Mandatory |
| ------------- | -------------------------------------- | ------ | --------- |
| `name`        | Name of the rule written to logs       | String | no        |
| `command`     | Command/script to execute via `sh -c`  | String | yes       |
| `env`         | User defined env variables             | Map    | no        |

Every type also has a common set of filters that can be used for matching.

| Common filters  | Description               | Type                            |
| --------------- | --------------------------| ------------------------------- |
| `resource-name` | Name of the DRBD resource | String                          |
| `event-type`    | Type of the event         | [EventType](umh.md#event-types) |

# State changes
Every field shown in this section marked as state change can be set
on the `old` and/or `new` section of a rule. This can be used to filter state changes (e.g., from
`Secondary` to `Primary`).

## Resource rules
Besides the [common fields](umh.md#common-fields), one can match the following fields in a `resource` rule:

| Fields        | Description                       | Type                          | State change |
| ------------- | --------------------------------- | ----------------------------- | ------------ |
| `role`        | Role of the resource              | [Role](umh.md#resource-roles) | yes          |
| `may-promote` | If the resource may be promoted   | Boolean                       | yes          |

A match on such a rule calls the specified `command` and sets the following environment variables:

| Variable                     | Description                                   |
| ---------------------------- | --------------------------------------------- |
| `DRBD_RES_NAME`              | Name of the DRBD resource                     |
| `DRBD_{OLD,NEW}_ROLE`        | [Role](umh.md#resource-roles) of the resource |
| `DRBD_{OLD,NEW}_MAY_PROMOTE` | If the resource may be promoted               |


## Device rules
Besides the [common fields](umh.md#common-fields), one can match the following fields in a `device` rule:

| Fields        | Description                       | Type                            | State change |
| ------------- | --------------------------------- | ------------------------------- | ------------ |
| `volume`      | Volume number                     | Integer                         | no           |
| `disk-state`  | Disk state of the device          | [DiskState](umh.md#disk-states) | yes          |
| `client`      | Device is a DRBD client           | Boolean                         | yes          |
| `quorum`      | Device has DRBD quorum            | Boolean                         | yes          |

A match on such a rule calls the specified `command` and sets the following environment variables:

| Variable                    | Description                                               |
| --------------------------- | --------------------------------------------------------- |
| `DRBD_RES_NAME`             | Name of the DRBD resource                                 |
| `DRBD_MINOR`                | Minor number of the device                                |
| `DRBD_MINOR_$volume`        | Minor number of the device by `volume`                    |
| `DRBD_VOLUME`               | `volume` (number) of the device                           |
| `DRBD_BACKING_DEV`          | Block device path to backing device or "none"             |
| `DRBD_BACKING_DEV_$volume`  | Block device path to backing device or "none" by `volume` |
| `DRBD_{OLD,NEW}_DISK_STATE` | [DiskState](umh.md#disk-states) of the device             |
| `DRBD_{OLD,NEW}_CLIENT`     | Device was/is a DRBD client                               |
| `DRBD_{OLD,NEW}_QUORUM`     | Device had/has DRBD qourum                                |

## Peer device rules
Besides the [common fields](umh.md#common-fields), one can match the following fields in a `peerdevice` rule:

| Fields              | Description                       | Type                                          | State change |
| ------------------- | --------------------------------- | --------------------------------------------- | ------------ |
| `volume`            | Volume number                     | Integer                                       | no           |
| `peer-node-id`      | Node ID of the Peer               | Integer                                       | no           |
| `peer-disk-state`   | Disk state of the peer-device     | [DiskState](umh.md#disk-states)               | yes          |
| `peer-client`       | Peer-device is a DRBD client      | Boolean                                       | yes          |
| `resync-suspended`  | DRBD resync is suspended          | Boolean                                       | yes          |
| `replication-state` | Replication state                 | [ReplicationState](umh.md#replication-states) | yes          |

A match on such a rule calls the specified `command` and sets the following environment variables:

| Variable                                | Description                                               |
| --------------------------------------- | --------------------------------------------------------- |
| `DRBD_RES_NAME`                         | Name of the DRBD resource                                 |
| `DRBD_MINOR`                            | Minor number of the device                                |
| `DRBD_MINOR_$volume`                    | Minor number of the device by `volume`                    |
| `DRBD_VOLUME`                           | `volume` (number) of the device                           |
| `DRBD_BACKING_DEV`                      | Block device path to backing device or "none"             |
| `DRBD_BACKING_DEV_$volume`              | Block device path to backing device or "none" by `volume` |
| `DRBD_PEER_NODE_ID`                     | Node ID of the peer                                       |
| `DRBD_{OLD,NEW}_PEER_DISK_STATE`        | [DiskState](umh.md#disk-states) of the peer-device        |
| `DRBD_{OLD,NEW}_PEER_CLIENT`            | Peer-device was/is a DRBD client                          |
| `DRBD_{OLD,NEW}_PEER_RESYNC_SUSPENDED`  | Resync was/is suspended                                   |
| `DRBD_{OLD,NEW}_PEER_REPLICATION_STATE` | [ReplicationState](umh.md#replication-states)             |

A note on `DRBD_BACKING_DEV*`: DRBD does not know the backing device path of its peer, so the device set in
these variables is the *local* backing device path! Usually the backing device names on all peers are the same
for diskful nodes, but it is not strictly required. This was not invented by `drbd-reactor`, this is how these
variables always have been set when DRBD kernel called user mode helpers from kernel space. So this might be
unexpected, but that is what it always was.

## Connection rules
Besides the [common fields](umh.md#common-fields), one can match the following fields in a `connection` rule:

| Fields             | Description             | Type                                        | State change |
| ------------------ | ----------------------- | ------------------------------------------- | ------------ |
| `peer-node-id`     | Node ID of the Peer     | Integer                                     | no           |
| `conn-name`        | Name of the connection  | String                                      | yes          |
| `connection-state` | Connection state        | [ConnectionState](umh.md#connection-states) | yes          |
| `peer-role`        | Peer role               | [Role](umh.md#resource-roles)               | yes          |
| `congested`        | Connection is congested | Boolean                                     | yes          |

A match on such a rule calls the specified `command` and sets the following environment variables:

| Variable                    | Description                                    |
| --------------------------- | ---------------------------------------------- |
| `DRBD_RES_NAME`             | Name of the DRBD resource                      |
| `DRBD_PEER_NODE_ID`         | Node ID of the peer                            |
| `DRBD_CSTATE`               | [Connection state](./umh.md#connection-states) |
| `DRBD_{OLD,NEW}_CONN_NAME`  | Conneciton name                                |
| `DRBD_{OLD,NEW}_CONN_STATE` | [Connection state](./umh.md#connection-states) |
| `DRBD_{OLD,NEW}_PEER_ROLE`  | Peer role                                      |
| `DRBD_{OLD,NEW}_CONGESTED`  | Connection was/is congested                    |

# Operators
Currently filters that are set are compared for equality with the value received in a state update. One handy
operator is "not equal", meaning everthing except the given value. We have to play within the boundaries of
toml, and we did not want to sacrifice type safety we get for free from the parser by inventing our own
"filter language".

The default comparison operator is "Equals":

```
old.role = "Primary"  # compares for equality
```

If another operator should be used, one has to specify the `value` *and* the `operator`:

```
old.role = { operator = "NotEquals", value = "Primary" }
# which is toml equivalent to these two lines:
old.role.operator = "NotEquals"
old.role.value = "Primary"
```

It is not possible to mix and match these two notations:

```
old.role.operator = "NotEquals"
old.role = "Primary"  # fails. it requires a .value in this case
```

The allowed operators are:

- `Equals` (the default)
- `NotEquals`

# Caveats
As it was mentioned before, fields that are not set are not taken into consideration when matching the filter.
Let's look at how one might write a filter:

```
[[umh.resource]]
command = "/usr/local/bin/primary.sh"
resource-name = "foo"
new.role = "Primary"
```

What this means is that this filter does *not* care about the state of the old role. So if the resource
changes for whatever reason, not related to it's role, an update is sent and the current state is matched
against the rule. In this case it would trigger as the the role "changes" from the old state "don't care"
(i.e., already `Primary`) to current/new state `Primary`.

Most rules are written to match specific state changes anyways, so a natural fit would be:

```
[[umh.resource]]
command = "/usr/local/bin/primary.sh"
resource-name = "foo"
old.role = "Secondary"
new.role = "Primary"
```

Another possibility for more complex fields than a resource's role, capturing everthing besides a given target
value look like this. See section [operators](./umh.md#operators) for details:

```
[[umh.resource]]
command = "/usr/local/bin/primary.sh"
resource-name = "foo"
old.role = { operator = "NotEquals", value = "Primary" }
new.role = "Primary"
```

# Types
## Event types

- `Exists`
- `Create`
- `Destroy`
- `Change`

## Resource roles

- `Unknown`
- `Primary`
- `Secondary`

## Disk states

- `Diskless`
- `Attaching`
- `Detaching`
- `Failed`
- `Negotiating`
- `Inconsistent`
- `Outdated`
- `DUnknown`
- `Consistent`
- `UpToDate`

## Replication states
- `Off`
- `Established`
- `StartingSyncS`
- `StartingSyncT`
- `WFBitMapS`
- `WFBitMapT`
- `WFSyncUUID`
- `SyncSource`
- `SyncTarget`
- `VerifyS`
- `VerifyT`
- `PausedSyncS`
- `PausedSyncT`
- `Ahead`
- `Behind`

## Connection states
- `StandAlone`
- `Disconnecting`
- `Unconnected`
- `Timeout`
- `BrokenPipe`
- `NetworkFailure`
- `ProtocolError`
- `TearDown`
- `Connecting`
- `Connected`
