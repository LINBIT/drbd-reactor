# agentx

This plugin serves a [AgentX](https://en.wikipedia.org/wiki/Agent_Extensibility_Protocol) subagent for SNMP
monitoring of DRBD resources exposing various DRBD metrics.

The plugin is enabled by adding a `agentx` section to drbd-reactor's configuration:

```toml
[[agentx]]
address = "localhost:705"
cache-max = 60 # seconds
agent-timeout = 60 # seconds snmpd waits for an answer
peer-states = true # include peer connection and disk states
```

## SNMPD configuration

The easiest way to configure `net-snmp` is to add a few extra lines to the default configuration in
`/etc/snmp/snmpd.conf`:

```
...
view    systemview    included   .1.3.6.1.2.1.25.1.1
# add LINBIT ID to the system view and enable agentx
view    systemview    included   .1.3.6.1.4.1.23302
master agentx
agentXSocket tcp:127.0.0.1:705
...
```

## Metrics

```
$ snmptranslate -Tp -IR -mALL linbit 
+--linbit(23302)
   |
   +--drbdData(1)
      |
      +--drbdMeta(1)
      |  |
      |  +-- -R-- String    ModuleVersion(1)
      |  |        Textual Convention: DisplayString
      |  |        Size: 0..255
      |  +-- -R-- String    UtilsVersion(2)
      |           Textual Convention: DisplayString
      |           Size: 0..255
      |
      +--drbdTable(2)
         |  Index: drbdMinor
         |
         +--drbdEntry(1)
            |  Index: drbdMinor
            |
            +-- -R-- INTEGER   Minor(1)
            +-- -R-- String    ResourceName(2)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- String    ResourceRole(3)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- EnumVal   ResourceSuspended(4)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- String    ResourceWriteOrdering(5)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- EnumVal   ResourceForceIOFailures(6)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- EnumVal   ResourceMayPromote(7)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- INTEGER   ResourcePromotionScore(8)
            +-- -R-- INTEGER   Volume(9)
            +-- -R-- String    DiskState(10)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- String    BackingDev(11)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- EnumVal   Client(12)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- EnumVal   Quorum(13)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- Unsigned  Size(14)
            +-- -R-- Unsigned  SizeUnits(15)
            +-- -R-- Counter64 Read(16)
            +-- -R-- Counter64 Written(17)
            +-- -R-- Counter64 AlWrites(18)
            +-- -R-- Counter64 BmWrites(19)
            +-- -R-- Unsigned  UpperPending(20)
            +-- -R-- Unsigned  LowerPending(21)
            +-- -R-- EnumVal   AlSuspended(22)
            |        Textual Convention: TruthValue
            |        Values: true(1), false(2)
            +-- -R-- String    Blocked(23)
            |        Textual Convention: DisplayString
            |        Size: 0..255
            +-- -R-- INTEGER   PeerNumberOfPeers(24)
            +-- -R-- INTEGER   PeerDiskDiskless(25)
            +-- -R-- INTEGER   PeerDiskAttaching(26)
            +-- -R-- INTEGER   PeerDiskDetaching(27)
            +-- -R-- INTEGER   PeerDiskFailed(28)
            +-- -R-- INTEGER   PeerDiskNegotiating(29)
            +-- -R-- INTEGER   PeerDiskInconsistent(30)
            +-- -R-- INTEGER   PeerDiskOutdated(31)
            +-- -R-- INTEGER   PeerDiskUnknown(32)
            +-- -R-- INTEGER   PeerDiskConsistent(33)
            +-- -R-- INTEGER   PeerDiskUpToDate(34)
            +-- -R-- INTEGER   PeerReplOff(35)
            +-- -R-- INTEGER   PeerReplEstablished(36)
            +-- -R-- INTEGER   PeerReplStartingSyncS(37)
            +-- -R-- INTEGER   PeerReplStartingSyncT(38)
            +-- -R-- INTEGER   PeerReplWFBitMapS(39)
            +-- -R-- INTEGER   PeerReplWFBitMapT(40)
            +-- -R-- INTEGER   PeerReplWFSyncUUID(41)
            +-- -R-- INTEGER   PeerReplSyncSource(42)
            +-- -R-- INTEGER   PeerReplSyncTarget(43)
            +-- -R-- INTEGER   PeerReplVerifyS(44)
            +-- -R-- INTEGER   PeerReplVerifyT(45)
            +-- -R-- INTEGER   PeerReplPausedSyncS(46)
            +-- -R-- INTEGER   PeerReplPausedSyncT(47)
            +-- -R-- INTEGER   PeerReplAhead(48)
            +-- -R-- INTEGER   PeerReplBehind(49)
```

## Cache behavior

As it seems SNMP is more equipped for static data than dynamic one we try to present a consistent view. For
example consider a `snmpwalk`, that is actually a series of SNMP `GETNEXT` requests, which result in a series
of AgentX GetNext requests. While AgentX headers have a "transaction ID", it can not be used to actually
correlate for example all GetNext requests for a single `snmpwalk`. This also means that while we are in the
middle of processing the MIB and while we sent some values for a particular DRBD `minor`, the corresponding
DRBD resource might vanish (e.g., `drbdadm down`).

We already use an internal cache so that we don't have to calculate the MIB values on every request. If we
receive a GetNext message, we also arm a second timer (currently 15 seconds), and if we receive a new GetNext
request within that time, we keep serving values from the cache to present a consistent view. This continues
until we send a `EndOfMibView` where we reset that timer (i.e., our notion of a "transaction") or until there
are more than 15 seconds between GetNext requests (e.g., the `snmpwalk` command was aborted somewhere in the
middle). What this means is that during such GetNext bursts the cache might be slightly older than the defined
`cache-max` value.
