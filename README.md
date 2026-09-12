# sbnat (Sandbox Network Address Translation) experiment

FreeBSD provides container-like sandboxing functionality in the form of
[jails][jails]. When an operating system process is "jailed", it can only
interact with processes belonging to the same jail. There are some exceptions
to jail restrictions, like shared Unix sockets and networking. Networking
makes it easy to accidentally allow jailed processes to bypass jails' strong
process isolation features. `sbnat` is an experiment in implementing network
isolation for jails.

## Common approaches to jail networking

There are several approaches to configuring networking for jails and
they all come with trade offs between isolation and manageability.
Here is a short, non-exhaustive summary of the common approaches:

1. Host-based networking - The host's network stack (network interfaces,
   routing tables, process network state) are shared with the jail. No
   management overhead, but provides zero network namespace isolation
2. Interface IP restriction / interface passthrough - The jail's processes
   are restricted to using the specified IP addresses or network interfaces,
   usually requires creating a dedicated loopback interface and complex
   firewall rules. So kinda-sorta partial network namespace isolation with
   gotchas (for example: unjailed processes listening on all addresses are
   still reachable from the jail)
3. [VNET(9)][vnet] isolated network namespace - Creates a jail-specific
   network namespace with a dedicated loopback network interface and
   routing table. Requires another interface be passed through
   (usually, [epair(4)][epair]) which requires bridging on the
   host side, address planning, NAT, and all the fun that comes
   with that

[jails]: https://man.freebsd.org/cgi/man.cgi?query=jail&apropos=0&sektion=2
[vnet]: https://man.freebsd.org/cgi/man.cgi?query=VNET&sektion=9&format=html
[epair]: https://man.freebsd.org/cgi/man.cgi?query=epair

## How sbnat works

Isolating jailed processes' networking is important for both security
reasons (e.g., to prevent sandbox escapes) and for resource conservation
(e.g., running multiple instances of the same process that listen on
the same TCP port for connections).

I wanted to experiment with building something on top of VNET's strong
networking namespacing functionality that was also easy to maintain.
My take on this was sbnat (sandbox NAT) - a Rust-based client library
(`libsbnat`) that proxies calls to `connect(2)` and sends the client's
desired socket address and socket over a Unix socket to a daemon running
outside the jail (`sbnatd`). The daemon then decides if the socket should
be connected and returns a new socket descriptor from the global namespace
back to the client running in the jail.

Here is a visualization of that approach:

```
#################################
#    Jail namespace with        #          Global namespace
#    isolated network stack     #
#                               #
#  +-------------------------+  #
#  |     netcat process      |  #
#  |                         |  #
#  |  0. libsbnat is loaded  |  #
#  |     through LD_PRELOAD  |  #
#  |     or other mischief   |  #
#  |  1. socket(2)           |  #
#  |  2. connect(2)          |  #
#  |  3. connect(libsbnat)   |  #  +----------------------------------+
#  |  4. Connect to shared   |  #  |        sbnatd process            |
#  |     sbnatd Unix socket  |  #  |                                  |
#  |  5. Send socket + addr==|==#==|=>6.  Get socket's type, domain,  |
#  |                         |  #  |      and protocol                |
#  |                         |  #  |  7.  Check if socket attributes  |
#  |                         |  #  |      and desired address are     |
#  |                         |  #  |      allowed                     |
#  |                         |  #  |  8.  Close socket and create     |
#  |                         |  #  |      a new one (because FreeBSD  |
#  |                         |  #  |      knows the original came     |
#  |                         |  #  |      from a jail with isolated   |
#  |                         |  #  |      network stack)              |
#  |                         |  #  |  9.  Call connect(2) on new      |
#  |                         |  #  |      socket                      |
#  |   11. Replace fd of  <==|==#==|==10. Send new socket to client   |
#  |       socket we sent    |  #  |                                  |
#  |       with new one      |  #  +----------------------------------+
#  |       using dup2(2)     |  #
#  |   12. Return success    |  #
#  |       to code that      |  #
#  |       called connect(2) |  #
#  |   13. nc can get some   |  #
#  |       ice cream c:      |  #
#  |                         |  #
#  +-------------------------+  #
#                               #
#################################
```

## Usage

1. In a FreeBSD VM you do not care about, create a jail with an isolated
   network stack on top of the root file system (or setup a dedicated
   file system if you like):

```console
# cat /etc/jail.conf.d/testsbnat.conf
testsbnat {
  path = "/";
  host.hostname = "${name}";
  vnet;
  persist;
}
# service jail start testsbnat
Starting jails: testsbnat.
```

2. Outside the jail, compile everything by cd'ing to the root of the
   repository and running: `cargo build`

3. Outside the jail, start the daemon as root:

```console
# ./target/debug/sbnatd -s /tmp/sbnat.sock
```

4. Start a shell in the jail and run ifconfig to confirm there is
   only one interface (loopback). Run `nc` to confirm we cannot
   reach anything (note: 185.52.176.84 is fishtank.openbsd.amsterdam):

```console
# jexec testsbnat
root@testsbnat:/ # ifconfig
lo0: flags=8008<LOOPBACK,MULTICAST> metric 0 mtu 16384
        options=680003<RXCSUM,TXCSUM,LINKSTATE,RXCSUM_IPV6,TXCSUM_IPV6>
        groups: lo
        nd6 options=21<PERFORMNUD,AUTO_LINKLOCAL>
root@testsbnat:/ # nc -v 185.52.176.84 22
nc: connect to 185.52.176.84 port 22 (tcp) failed: Network is unreachable
```

5. Rerun `nc`, making sure to preload the library and specify the
   magic libsbnat environment variable that specifies the sbnatd
   Unix socket path:

```console
root@testsbnat:/ # export SBNAT_SOCKET_PATH=/tmp/sbnat.sock
root@testsbnat:/ # LD_PRELOAD=/path/to/sbnat/target/debug/liblibsbnat.so
root@testsbnat:/ # nc 185.52.176.84 22
SSH-2.0-OpenSSH_10.3
```


6. From outside the jail, we can see the `nc` process has connected
   to the SSH server we specified above (note, there are two file
   descriptors because the client uses `dup2(2)` to replace the
   original which also creates a second file descriptor):

```console
# sockstat
USER   COMMAND      PID FD PROTO   LOCAL ADDRESS         FOREIGN ADDRESS
root   nc         25667  3 tcp4    10.0.2.15:44353       185.52.176.84:22
root   nc         25667  5 tcp4    10.0.2.15:44353       185.52.176.84:22
(...)
```

## Was this a good idea?

In short, I do not think it was a *terrible* idea - but the biggest
shortcoming is that programs that skip libc and implement `connect(2)`
using system calls (like Go programs) are not compatible with this
approach. It was still a fun experiment and Rust made it easy (as
long as you remember to add `#[repr(C)]` to struct definitions).

## Project status

This was an experiment. It may be a useful reference, but I only
ever made it work with `nc` (netcat).
