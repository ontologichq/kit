# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`ontologic-kit`: the API shared by the ontologic engine and its clients. This repository is
public: no server addresses, keys, customer names, prompts or engine internals go in it, in
code, tests, comments or commit messages.

- `proto/ontologic.proto` (package `ontologic.v1`, service `Ontologic`): the only source of the
  API. `build.rs` parses it with `protox` and generates code with `tonic_prost_build`, so no
  `protoc` is needed anywhere.
- `src/lib.rs`: `pb` (generated), `DEFAULT_PORT`, the sign-in metadata names and the engine's
  sign-in failure message.
- `src/client.rs` (feature `client`): `connect`, `SignIn`, `endpoint_url`, `describe`,
  `TRUST_DIR` (`.ontologic/tls`: `engine.pem` for localhost, `<host>.pem` otherwise).
- `src/fake.rs` (feature `fake`): `FakeEngine`, scripted replies per `Rpc` (the last one
  repeats), streams, `sign_in` to check credentials, `calls()` to read what came in.

## Rules

- Consumers pin a tag (`vX.Y.Z`, the version in `Cargo.toml`). Adding a field or an RPC is a
  minor bump; renaming, removing or retyping a field is a major bump. Never reuse a field
  number.
- Code moves into kit when a second repository needs it.
- Branches `<issue>/<slug>`, Conventional Commits, no direct commits to `main`, no AI
  co-author trailers, no em dashes in docs.

## Commands

```bash
make check                          # the gate
cargo test --all-features <name>    # one test
```
