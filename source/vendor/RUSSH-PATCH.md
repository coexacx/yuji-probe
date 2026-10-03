# russh 0.63.3 local patch

Vendored from the published russh 0.63.3 crate. Original archive SHA-256:
036204edbd199552a5b3832f63c60dcdf395dc44c7f06b4af1c0e8139cc11bce.

Upstream: https://github.com/warp-tech/russh
The original licenses, tests and public test keys are retained.

Changes:
- Suspend client global keepalive requests during key exchange; always re-arm the timer.
- Bound rekeying separately with Config::key_exchange_timeout (default 60 seconds).
- Report Error::KeyExchangeTimeout separately from Error::KeepaliveTimeout.
- Gate the decompression-only constant with the existing compression feature.

No cipher, key agreement, host-key verification, or authentication algorithm is changed.
Regression coverage: source/controller-rust/tests/ssh_liveness.rs.
