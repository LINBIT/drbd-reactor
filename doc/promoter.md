# promoter

The promoter plugin monitors events on resources an executes systemd units. This plugin can be used for simple
high-availability.

By default the plugin generates a series of systemd service overrides and a systemd target unit that contains
dependencies on these generated services. The services, and their order, is defined via the list specified in
`start`. The plugin generates two implicit extra units:

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

If a resource looses "quorum", it stops the systemd `drbd-resource@` target and all the dependencies.

The plugin's configuration can contain an action that is executed if a stop action fails (e.g., triggering a
reboot). Start actions in `start` are interpreted as systemd units and have to have an according postfix (i.e.
`.service`, `.mount`,...). `ocf` resource agents are supported via the `ocf.ra@` service, see
[this section](promoter.md#ocf-resource-agents) for details.

The configuration can contain a setting that specifies that resources are stopped whenever the plugin exits
(e.g., on service restart).

It also contains a `runner` that can be set to `shell`. Then the items in `start` are interpreted as shell
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
