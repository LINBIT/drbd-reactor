# promoter

The promoter plugin monitors events on resources an executes systemd units. This plugin can be used for simple
high-availability.

If a resource changes its state to "may promote", the plugin (i.e., all plugins on all nodes in the cluster)
start the defined systemd units. If this is a mount unit, and DRBD auto-promote is enabled (the default), one
of the plugins will succeed and promote the DRBD resource to DRBD Primary. All the others will fail.

If a resource looses "quorum", it stops all the systemd units in reverse order.

The plugin's configuration can contain an action that is executed if a stop action fails (e.g., triggering a
reboot). Start/stop actions are interpreted as systemd units and handled as such. Other service
mangers/scripts are supported by starting the action with an absolute path.

## DRBD resource configuration

Make sure the resource has the following options set:

```
options {
   quorum majority;
   on-no-quorum io-error;
}
```
