# prometheus

This plugin serves a [prometheus.io](https://prometheus.io) compatible endpoint
for DRBD resources exposing various DRBD metrics.

The plugin is enabled by adding a `prometheus` section to drbd-reactor's configuration:

```
[[prometheus]]
enums = true
```

## Metrics

- `drbdreactor_up gauge`: Boolean indicating whether or not drbdreactor is running. Always 1
- `drbd_kernel_version gauge`: Version of the loaded DRBD kernel module and DRBD utils
- `drbd_connection_apinflight_bytes gauge`: Number of application requests in flight (not completed)
- `drbd_connection_congested gauge`: Boolean whether the TCP send buffer of the data connection is more than 80% filled
- `drbd_connection_rsinflight_bytes gauge`: Number of resync requests in flight
- `drbd_connection_state gauge`: DRBD connection state
- `drbd_device_alsuspended gauge`: Boolean whether the Activity-Log is suspended
- `drbd_device_alwrites_total counter`: Number of updates of the activity log area of the meta data
- `drbd_device_bmwrites_total counter`: Number of updates of the bitmap area of the meta data
- `drbd_device_client gauge`: Boolean whether this device is a client (i.e., intentional diskless)
- `drbd_device_lowerpending gauge`: Number of open requests to the local I/O sub-system issued by DRBD
- `drbd_device_quorum gauge`: Boolean if this device has DRBD quorum
- `drbd_device_read_bytes_total counter`: Net data read from local hard disk
- `drbd_device_size_bytes gauge`: Device size in bytes
- `drbd_device_state gauge`: DRBD device state
- `drbd_device_unintentionaldiskless gauge`: Boolean whether the devices is unintentional diskless
- `drbd_device_upperpending gauge`: Number of block I/O requests forwarded to DRBD, but not yet answered by DRBD.
- `drbd_device_written_bytes_total counter`: Net data written on local disk
- `drbd_peerdevice_outofsync_bytes gauge`: Number of bytes currently out of sync with this peer, according to the bitmap that DRBD has for it
- `drbd_peerdevice_sent_bytes`: Number of bytes currently sent to this peer
- `drbd_peerdevice_received_bytes`: Number of bytes currently received from this peer
- `drbd_resource_maypromote gauge`: Boolean whether the resource may be promoted to Primary
- `drbd_resource_promotionscore gauge`: The promotion score (higher is better) for the resource
- `drbd_resource_resources gauge`: Number of resources
- `drbd_resource_role gauge`: DRBD role of the resource
- `drbd_resource_suspended gauge`: Boolean whether the resource is suspended

## Grafana Dashboard

With its Prometheus plugin, drbd-reactor exports a powerful set of Prometheus
metrics which can be used to optimally monitor a DRBD deployment.

In this repository, we provide an [example dashboard](/example/grafana-dashboard.json)
that showcases some of the generic use cases that can be accomplished with
these metrics.

The dashboard is published in the Grafana dashboard registry: [DRBD Grafana Dashboard](https://grafana.com/grafana/dashboards/14339-drbd/)

---

![dashboard](/example/grafana-dashboard.png)
