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
or auto-rebalance resource placement, or have complex resource dependency tree, you still want to use your
favorite cluster manager (Pacemaker).

But if we can take away the cluster manager, and get away with it (in "relevant" scenarios), that would be a
win for some setups.

# Configuration
By default the plugin generates a series of systemd service overrides and a systemd target unit that contains
dependencies on these generated services. The services, and their order, is defined via the list specified via
`start = []`. The plugin generates two implicit extra units:

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

If a resource looses "quorum", it stops the systemd `drbd-resource@` target and therefore all the dependencies.

The plugin's configuration can contain an action that is executed if a stop action fails (e.g., triggering a
reboot). Start actions in `start` are interpreted as systemd units and have to have an according postfix (i.e.
`.service`, `.mount`,...). `ocf` resource agents are supported via the `ocf.ra@` service, see
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
"p_iscsi_demo1" for a DRBD resource "foo" would be `ocf.ra@p_iscsi_demo1_foo`. `name`/`value` pairs are passed
to the unit as environment variables prefixed with `OCF_RESKEY_`.

In a concrete example using the "heartbeat:IPaddr2" agent this could look like this:

```
start = [
	"foo.service",
	"ocf:heartbeat:IPaddr2 p_iscsi_demo1_ip ip=10.43.7.223 cidr_netmask=16 arp_sender=iputils_arping",
	"bar.service"
]
```

OCF agents are expected in `/usr/lib/ocf/resource.d/`

## DRBD resource configuration

Make sure the resource has the following options set:

```
options {
   auto-promote no;
   quorum majority;
   on-no-quorum io-error;
}
```

# Handled (failure-) scenarios

## Promotion and Service Start
All nodes that see the "promotable" will race for the promotion, DRBD state change handling will arbitrate,
one will win.  The others will fail to promote, no longer see the "promotable" (as some peer is already
promoted), and wait for further state changes.  The winning node continues to start the defined services
in order.  If a start failure occurs, they will be stopped again in order, drbd will be demoted, the peers
will see it as "promotable".  The process repeats.

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

If service failure is detected by the service itself, by a monitoring loop in the `ocf.ra` wrapper service, or
by systemd, the `drbd-services@.target` instance will be stopped by systemd, resulting in a "promotable"
resource again.

It is very important to know that the promoter plugin does not do any service monitoring at all! So in order
to make `drbd-serivices@.target` restart (i.e., stop and start), one needs to make sure a service failure
gets propagated to `drbd-serivices@.target`. The `ocf.ra` service does that by setting `Restart=always`.
If in your configuration `ocf.ra` is not used, then it is up to you to make sure a service failure is propaged
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

If not, then no service is possible at this times.

Once quorum returns, either the local services have long since been stopped already (due to propagated
"io-errors"), DRBD reconnects and resyncs, or IO (and services) are still blocked.

In the "still blocked" case, DRBD may have to refuse the connection, we cannot join an other Primary while
still being Primary ourselves. But this even should trigger the local `drbd-reactor` to request an explicit
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

If a service fails to stop, we need to "escalate" the recovery.  One way could be to "force disconnect",
causing DRBD quorum loss, and have the services die by the resulting io-errors, to the point where we can
demote DRBD. Again, the before mentioned adjust on `ExecStopPost` will re-connect and sync.

If DRBD still can not be demoted, that resource is blocked and would need manual cleanup.  I don't think we
want to escalate to "hard-reboot the node", which would be an other option.

Meanwhile, the other peers would form a promotable partition and continue to provide service, unless we are in
a multiple failure scenario and it was already degraded.

| !! | Current implementation: |
| -- | ----------------------- |

A user can define whatever action via the `on-stop-failure` configuration option. A hard reboot for example
can be realized via:

```
on-stop-failure =  "echo b > /proc/sysrq-trigger"
```
