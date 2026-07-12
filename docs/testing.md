# Testing Strategy

## Unit Tests

- Protocol encode/decode round trips
- CRC rejection
- Fragment reassembly
- Capability negotiation
- Input event mapping

## Integration Tests

- ADB compatibility stream loop
- Native USB bulk stream loop
- Decoder recovery after dropped frames
- Auto reconnect after unplug
- Suspend and resume

## System Tests

- 1080p60 on USB 2
- 1440p120 on USB 3
- Multi-monitor topology
- Multi-tablet topology
- Rotation and resolution switching
- Windows duplicate and extend modes

## Performance Gates

- USB 2 target: 1080p60 under 35 ms glass-to-glass latency
- USB 3 target: 1440p120 under 20 ms glass-to-glass latency
- Host CPU under 10 percent during steady state
- Host GPU under 15 percent during steady state
- Host memory under 250 MB

