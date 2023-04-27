# Notes to self

## The UDP protocol in general

UDP datagrams by nature should not be very large, much less messages spanning multiple
datagrams.
If a datagram is larger than what fits in the Ethernet MTU of 1500 bytes, then they
are fragmented by the IP protocol, and if any fragment is lost, the whole datagram is
lost.
UDP datagrams work better if they are small in nature.
Furthermore, by default platforms like Linux don't even allow fragmenting UDP datagrams, and will
just fail instead <https://man7.org/linux/man-pages/man7/udp.7.html>:

> By default, Linux UDP does path MTU (Maximum Transmission Unit)
> discovery. This means the kernel will keep track of the MTU to a
> specific target IP address and return EMSGSIZE when a UDP packet
> write exceeds it. When this happens, the application should
> decrease the packet size. Path MTU discovery can be also turned
> off using the IP_MTU_DISCOVER socket option or the
> /proc/sys/net/ipv4/ip_no_pmtu_disc file; see ip(7) for details.
> When turned off, UDP will fragment outgoing UDP packets that
> exceed the interface MTU. However, disabling it is not
> recommended for performance and reliability reasons.

The UDP protocol over IPv4 may (but is not required to) include checksums in UDP
headers. On IPv6 it is mandatory.
