# Benchmark comparison summary

Date: 2026-03-05
Node target: benchmark-example
Module target: benchmark-workload

## Throughput and write-latency summary

| Scenario             | Throughput (tx/s) | Mean write latency (µs) | P99 write latency (µs) | Failed |
| -------------------- | ----------------: | ----------------------: | ---------------------: | -----: |
| vm-noop              |         450626.80 |                    7.81 |                  16.83 |      0 |
| durability-ephemeral |         466539.05 |                    6.87 |                  22.66 |      0 |
| durability-stateful  |         450376.60 |                    7.09 |                  11.89 |      0 |
| durability-logged    |         446751.55 |                    7.11 |                  16.90 |      0 |
| transport-persistent |         452018.85 |                    7.16 |                  16.92 |      0 |
| transport-reconnect  |           2690.90 |                  667.67 |                2634.88 |   6653 |
| fanout-emit          |         457091.75 |                    6.96 |                  11.66 |      0 |

## Key comparisons

- Logged vs ephemeral durability throughput: -4.24%
- Logged vs stateful durability throughput: -0.80%
- Fanout throughput vs vm-noop: 101.43% of baseline
- Reconnect throughput vs persistent: 0.60% of persistent (about 167.98x slower)

## Notes

- The worker-identity fix removed duplicate-peer collisions for persistent transport. Persistent profiles now show failed = 0.
- Reconnect mode still shows failures under high pressure because each operation establishes/closes sockets; this profile intentionally stresses connection churn.

## Raw result files

- benchmarks/results/vm-noop.json
- benchmarks/results/durability-ephemeral.json
- benchmarks/results/durability-stateful.json
- benchmarks/results/durability-logged.json
- benchmarks/results/transport-persistent.json
- benchmarks/results/transport-reconnect.json
- benchmarks/results/fanout-emit.json
