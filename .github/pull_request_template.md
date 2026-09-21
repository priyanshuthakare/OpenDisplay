<!-- Thanks for contributing! Keep it focused and describe how you tested it. -->

## What changed

<!-- One or two sentences. What problem does this solve? -->

## Component(s)

<!-- Tick what applies. Not all components are needed for most changes. -->

- [ ] Rust (`protocol/`, `transport/`, `host/`)
- [ ] Driver (`driver/idd/`)
- [ ] Android (`android/`)
- [ ] Control Center (`control-app/`)
- [ ] Docs / CI / packaging

## How I tested it

<!-- Exact commands, and what you observed. -->

```
cargo test --workspace
dotnet test control-app/USBDisplay.sln
```

## Checklist

- [ ] `cargo test --workspace` passes (if Rust changed)
- [ ] `dotnet test control-app/USBDisplay.sln` passes (if the Control Center changed)
- [ ] Wire-format changes (`protocol/`, `transport/`, input events) update **both** Rust and Kotlin and their reference-byte tests
- [ ] CLI `key=value` output changes update the Control Center parsers and the README interface tables
- [ ] Docs updated for any changed interface or behavior
- [ ] No new capability strings advertised unless actually implemented
