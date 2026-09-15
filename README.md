# kit

The ontologic engine's API, shared by the engine, the [CLI](https://github.com/ontologichq/cli)
and anything else that talks to an engine.

- `pb`: the gRPC messages and the `Ontologic` service, generated from
  [`proto/ontologic.proto`](proto/ontologic.proto). Building needs no `protoc`.
- `client` (feature): a client that signs in on every call (`user` and `key` metadata), trusts
  the engine's certificate, and says in words why an engine cannot be reached.
- `fake` (feature): a fake engine served over TLS in process, answering each call from a
  script and remembering the calls it got, for testing clients without an engine.

```toml
[dependencies]
ontologic-kit = { git = "https://github.com/ontologichq/kit", tag = "v0.1.0", features = ["client"] }
```

To work on kit and an app together, point the app at a local checkout in its gitignored
`.cargo/config.toml`:

```toml
[patch."https://github.com/ontologichq/kit"]
ontologic-kit = { path = "../kit" }
```

## Versions

Tags are `vX.Y.Z` and match `Cargo.toml`. A new field or call is a minor version; a change
that breaks the wire (a renamed or removed field, a changed type) is a major one.

## Checks

```bash
make check   # fmt, clippy with every feature, a build with none, tests
```

Licensed under the Apache License, Version 2.0.
