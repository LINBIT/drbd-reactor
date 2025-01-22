# promoter

The promoter plugin monitors events on resources an executes systemd units. This plugin can be used for simple
high-availability.

# Design

> It is beautiful when there is nothing left to take away. -  someone, about HA-clustering

If your HA failover cluster solution depends on DRBD for persisting your data, the state of DRBD should
determine if, and where, that data should best be used.  If we add a cluster manager, the cluster manager
decides where services should run, but depending on cluster communication, membership and quorum, and other
factors, that may or may not agree with where DRBD has "best access to good data".

If you need to base decisions on other factors, like external connectivity, or other "environmental health",
or auto-rebalance resource placement, or have a complex resource dependency tree, you still want to use your
favorite cluster manager (Pacemaker).

But if we can take away the cluster manager, and get away with it (in "relevant" scenarios), that would be a
win for some setups.

# Configuration
By default the plugin generates a series of systemd service overrides (i.e., what systemd calls "Drop-In") and
a systemd target unit that contains dependencies on these generated services. The services, and their order,
is defined via the list specified via `start = []`. The plugin generates two implicit extra units:

- a `drbd-promote@` override that promotes the DRBD resource (i.e., switches it to Primary). This is a
dependency for all the other units from `start` (according overrides are generated).
- a `drbd-resource@` target that subsumes all the generated dependencies from the `start` list.

Let's look at a simple example to see which overrides get generated from a dummy start list like this:

```
[[promoter]]
[promoter.resources.foo]
start = [ "a.service", "b.service", "c.service" ]
```

- `/var/run/systemd/system/drbd-promote@foo.d/reactor.conf` containing the necessary pieces to wait for the
  backing devices of DRBD resource "foo" and to promote it to Primary.
- `/var/run/systemd/system/a.service.d/reactor.conf` containing a dependency on `drbd-promote@foo`
- `/var/run/systemd/system/b.service.d/reactor.conf` containing dependencies on `drbd-promote@foo` and on
  `a.service`.
- `/var/run/systemd/system/c.service.d/reactor.conf` containing dependencies on `drbd-promote@foo` and on
  `b.service`.
- `/var/run/systemd/system/drbd-resource@foo.target.d/reactor.conf` containing dependencies on
  `a.service`, `b.service`, and `c.service`.

If a DRBD resource changes its state to "may promote", the plugin (i.e., all plugins on all nodes in the cluster)
start the generated systemd target (e.g., `drbd-resource@foo.target`). All will try to start the
`drbd-promote@` unit first, but only one will succeed and continue to start the rest of the services. All the
others will fail intentionally.

If a resource loses "quorum", it stops the systemd `drbd-services@` target and therefore all the dependencies.
Stopping services on the node that lost quorum is the standard behavior one would expect from a cluster manger.
There might be scenarios where it is preferable to freeze the started service until quorum is gained
again. As this requires multiple prerequisites to hold true, freezing a resource on quorum loss is described
in [its own section](promoter.md#freezing-resources).

The plugin's configuration can contain an action that is executed if a stop action fails (e.g., triggering a
reboot). Start actions in `start` are interpreted as systemd units and have to have an according postfix (i.e.
`.service`, `.mount`,...). `ocf` resource agents are supported via the `ocf.rs@` service, see
[this section](promoter.md#ocf-resource-agents) for details.

The configuration can contain a setting that specifies that resources are stopped whenever the plugin exits
(e.g., on `drbd-reactor` service restart, or plugin restart).

The configuration also contains a `runner` that can be set to `shell`. Then the items in `start` are interpreted as shell
scripts and started in order (no explicit targets or anything) and stopped in reverse order or as defined via
`stop`. This can be used on systems without systemd and might be useful for Windows systems in the future. If
you can, use the default systemd method, it is the preferred one.

## Service dependencies
Let's get back to our simple example with `start = [ "a.service", "b.service", "c.service" ]`. As we noted in
the previous section we generate a dependency chain for these services (i.e., all depend on `drbd-promote@`
as well as on the previous services). The strictness of these dependencies can be set via `dependencies-as`,
where the default is `Requires` (see `systemd.unit(5)` for details).

We also generate the mentioned `drbd-services@.target`, which lists all the services from `start`. The
dependencies for that are generated via the value set in `target-as`.

Especially when one debugs services it might make sense to lower these defaults to for example `Wants`.
Otherwise a failed service might prohibit a successful start of the `drbd-services@.target`, which then
triggers a stop of the target and its dependencies, which might again trigger a start because the resource is
DRBD promotable again and so on.

It is really up to you and how strict/hard you want your dependencies and what their outcome should be.
`Requires` should be a good default, you might lower or increase the strictness depending on the scenario.

## OCF resource agents
It is possible to use [resource agents](https://github.com/ClusterLabs/resource-agents) in the `start` list of
services via `ocf:$vendor:$agent instance-id name=value ...`. The `instance-id` is user defined and gets
postfixed with `_$resourcename`. For example the generated systemd unit for an `instance-id` of
"p_iscsi_demo1" for a DRBD resource "foo" would be `ocf.rs@p_iscsi_demo1_foo`. `name`/`value` pairs are passed
to the unit as environment variables prefixed with `OCF_RESKEY_`.

In a concrete example using the "heartbeat:IPaddr2" agent this could look like this:

```
start = [
	"foo.service",
	"ocf:heartbeat:IPaddr2 p_iscsi_demo1_ip ip=10.43.7.223 cidr_netmask=16",
	"bar.service"
]
```

OCF agents are expected in `/usr/lib/ocf/resource.d/`. Please make sure to check for `resource-agents`
packages provided by your distribution or use the packages provided by LINBIT (customers only).

## Freezing resources
The default behavior when a DRBD Primary loses quorum is to immediately stop the generated target unit and
hope that other nodes still having quorum will successfully start the service. This works well if
services can be failed over/started on another node in reasonable time. Unfortunately there are services that
take a very long time to start, for example huge data bases.

When a DRBD Primary loses its quorum we basically have two possibilities:
- the rest of the nodes, or at least parts of it still have quorum: Then these have to start the service, they
are the only ones with quorum, but still we could keep the old Primary in a frozen state. And then, when the
nodes with quorum come into contact with the old Primary, then it should stop the service and its storage
should become in sync with the other nodes.
- the rest of the nodes are not able to form a partition with quorum. In such a scenario there are no
alternatives anyways, we would need to keep the Primary frozen. But if the nodes eventually join the old
Primary again, and quorum would be restored, we could just unfreeze/thaw the old Primary (which is also the
new Primary).

There are several requirements for this to work properly:
- A system with unified cgroups. If the file `/sys/fs/cgroup/cgroup.controllers` exists you should be fine.
That requires a relatively "new" kernel. Note that "even" RHEL8 for example needs the addition of
`systemd.unified_cgroup_hierarchy` on the kernel command line.
- a service that can tolerate to be frozen
- DRBD option `on-suspended-primary-outdated` set to `force-secondary`
- DRBD option `on-no-quorum` set to `suspend-io`
- DRBD option `on-no-data-accessible` set to `suspend-io`
- DRBD net option `rr-conflict` set to `retry-connect`

If these requirements are fulfilled, then one can set the promoter option `on-quorum-loss` to `freeze`.

## DRBD resource configuration

Make sure the resource has the following options set:

```
options {
   auto-promote no;
   quorum majority;
   on-suspended-primary-outdated force-secondary;
   on-no-quorum io-error; # for the default drbd-reactor on-quorum-loss policy (i.e., Shutdown)
   # on-no-quorum suspend-io; # for freezing resources
   on-no-data-accessible io-error # always set this to the value of on-no-quorum!
   # on-no-data-accessible suspend-io # for freezing, always set this to the value of on-no-quorum!
}
# net { rr-conflict retry-connect; } # for freezing resources
```

`drbd-reactor` itself is pretty relaxed about these settings, don't expect too much hand holding or even
auto-configuration, you as the admin are the one that should understand your system, but it checks properties
and writes warnings to the log (file/journal) if misconfiguration is detected.

A note on LINSTOR: LINSTOR created resources obviously can be used with `drbd-reactor`, but one should always
make sure to create a LINSTOR resource group, set all required options on the resource group, and then spawn
DRBD resource from that resource group. If a LINSTOR resource is created manually (i.e., `linstor resource
create ...` and friends) it implicitly gets assigned to LINSTOR's default resource group. If later properties
of that resource group change, they are passed down to the DRBD resources, which might have unforeseen
consequences. It is always a good idea to create dedicated LINSTOR resource groups for reactor controlled DRBD
resources. This could for example be one resource group for DRBD resources that should allow freezing, one for
DRBD resources that don't, and one for LINSTOR controller HA.

# Handled (failure-) scenarios

## Promotion and Service Start
All nodes that see the "promotable" will race for the promotion, DRBD state change handling will arbitrate,
one will win.  The others will fail to promote, no longer see the "promotable" (as some peer is already
promoted), and wait for further state changes.  The winning node continues to start the defined services
in order.  If a start failure occurs, they will be stopped again in order, drbd will be demoted, the peers
will see it as "promotable".  The process repeats.

In order to prefer nodes with a favorable disk state, actual promotion will be delayed based on the worst
case of local disk/volume states as below:

| DiskState      | Sleep time in seconds |
| -------------- | --------------------- |
| `Diskless`     | 6                     |
| `Attaching`    | 6                     |
| `Detaching`    | 6                     |
| `Failed`       | 6                     |
| `Negotiating`  | 6                     |
| `Unknown`      | 6                     |
| `Inconsistent` | 3                     |
| `Outdated`     | 2                     |
| `Consistent`   | 1                     |
| `UpToDate`     | 0                     |

The configuration can contain a `sleep-before-promote-factor` that can be used to scale the sleep time.

There should be some max retry or backoff delay to avoid busy loops for services that continuously fail to
start. It is up to the user to set these if the systemd defaults do not fit, systemd provides
`StartLimitIntervalSec=` and `StartLimitBurst=`.

To have the "best" (according to the drbd "promotion-score") peer be the most likely to win the promotion
race, there may be some heuristics and delays before taking action. Such a heuristic is currently not
implemented, plugins just race to promote the resource.

## Cluster start up

systemd will start the `drbd-reactor.service`.  It may bring up some pre-defined DRBD resource(s).  systemd or
`drbd-reactor` may start up the LINSTOR controller, if it is used in the setup, which will bring up other DRBD
resources.

DRBD tries to establish replication connections. Once DRBD gains "quorum", i.e. has access to known good data
without any Primary peer present, it becomes "promotable".

Once `drbd-reactor` sees in the DRBD event stream a DRBD resource claiming to be "promotable", it will try to
start the list of services defined for this resource. See [Configuration](promoter.md#configuration) for more
details.

## Node failure

The peers will see replication links go down, the resource becomes promotable. See above.

## Service failure

If service failure is detected by the service itself, by a monitoring loop in the `ocf.rs` wrapper service, or
by systemd, the `drbd-services@.target` instance will be stopped by systemd, resulting in a "promotable"
resource again.

It is very important to know that the promoter plugin does not do any service monitoring at all! So in order
to make `drbd-services@.target` restart (i.e., stop and start), one needs to make sure a service failure
gets propagated to `drbd-services@.target`. The `ocf.rs` service does that by setting `Restart=always`.
If in your configuration `ocf.rs` is not used, then it is up to you to make sure a service failure is propaged
to the target. This can for example be done setting `Restart=always` in your service (e.g., via a systemd
override).

## Replication Link Failure

| !! | The following is a design draft |
| -- | ------------------------------- |

If DRBD retains quorum, that is: knows the unreachable peers cannot form a promotable partition, services just
keep running.

If DRBD lost quorum, depending on chosen policy, any IO on the volume may block, or may show IO errors.
Dynamically configuring for "on-no-quorum suspend-io", and reconfigure for "on-no-quorum io-error" on
stop of the target can be a solution.

If the other peers form a promotable partition, they will claim the resource and start services.

If not, then no service is possible at this time.

Once quorum returns, either the local services have long since been stopped already (due to propagated
"io-errors"), DRBD reconnects and resyncs, or IO (and services) are still blocked.

In the "still blocked" case, DRBD may have to refuse the connection, we cannot join an other Primary while
still being Primary ourselves. But this event should trigger the local `drbd-reactor` to request an explicit
stop, which would reconfigure for io-error, and finally demote the resource. As last `ExecStopPost` action, we
call drbdadm adjust, which should cause DRBD to reconnect again, this time as Secondary, and finally sync up.

This may need some thought, possibly `drbd-reactor` calling `drbdadm adjust` every so often if there are
"StandAlone" connections.

| !! | Current implementation: |
| -- | ----------------------- |

Currently `drbd-reactor` does not do any of the described reconfiguration and you as the admin should
configure the resource for "io-error". If you want to, drbd-utils starting from 9.18.0 include a
`drbd-reconfigure-suspend-or-error@.service` than can be included in your `start = []` list.

## Service Stop Failure

If a service fails to stop, we need to "escalate" the recovery. We expect that services propagate failures to
the systemd target, which then restarts the services.

This also demotes the DRBD device and another peer might promote the device and start the services.

What we are interested in is when the demotion of the DRBD device fails on a node. Then we have to react with
power off/reboot/...

A user can define a `systemd` `OnFailure` action via the `on-drbd-demote-failure` configuration option. A hard reboot for example
can be realized via:

```
on-drbd-demote-failure =  "reboot-immediate"
```

By default the promoter will try to demote the DRBD device first via `drbdsetup secondary`, and if that fails
as fallback via `drbdsetup secondary --force`. This has the advantage that demote failures are handled more
benign. For example imagine a mount unit that still has openers. A plain `secondary` would fail and eventually
trigger the `OnFailure` action. By using `secondary --force` the operation will most likely succeed and not
escalate to the `OnFailure` action because DRBD will be temporarily reconfigured to report errors on device
access, causing suspended units to resume with shutdown. If your service can't handle temporary errors during
service shutdown, you can set `secondary-force` to false. One major advantage of `secondary --force` and its
benign behavior is that you don't need to reboot a node with maybe hundreds of active resources just because
one (maybe even not so important) resource blocks.

# HA involving File System Mount Points
Almost all relevant scenarios include a file system mount. For example to realize a highly available LINSTOR
controller, a file system containing LINSTOR's database would be mounted right before the LINSTOR controller
service gets started. In these scenarios where another service is on top of a mount point, one should use
systemd mount units (`systemd.mount(5)`). On systemd target shutdown (e.g., quorum loss), systemd has all means
to `SIGTERM/SIGKILL` all processes that use the mount point. For example systemd can stop the LINSTOR
controller and all processes it has spawned that might use the file system cleanly.

If the highly-available file system mount point is the end goal (i.e., the mount unit would be the last
service that is started), one should *not*
use a systemd mount unit. Why is that? If that mount point is then in use, per definition there are processes
that have files opened systemd does not know about (e.g., your editor editing a file on the HA file system
mount). On target stop the unmount will fail, which by itself would be fine, but the situation would never
improve, not even after a `secondary --force`. There needs to be something that removes processes that "idle
around" but keep the file system from being unmounted. Again, if the mount point would not have been the last
service, but some other service, then systemd would have made sure that all users are terminated. In our case
something else must make sure this happens. Fortunately that component already exists: The file system
resource-agent, which does all kinds of magic tricks to get rid of processes blocking a file system from being
unmounted. So, to conclude: if the mount point would be the last service to start, don't use a systemd mount
unit, but use the file system resource-agent instead. In the most simple case this could look like this:

```
start = ["ocf:heartbeat:Filesystem fs_test device=/dev/drbd1000 directory=/mnt/test fstype=ext4 run_fsck=no"]
```

# Preferred Nodes
While in a HA cluster that deserves the name every node needs to be able to run all services, some users like
to add preferences for nodes. This can be done by setting a list of `preferred-nodes`.  On resource startup a
delay based on the node's position in the list is added.  Nodes with a lower preference will sleep longer. If
a node joins on DRBD level, and that peer's disk becomes `UpToDate`, and the peer has a higher preference, then
the active node stops the services locally. As it will then have a higher sleep penalty as the preferred
node, the preferred one will take over the service (if it can).
